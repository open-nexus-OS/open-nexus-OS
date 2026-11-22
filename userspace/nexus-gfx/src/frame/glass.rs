// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The glass cache (moved from the QEMU path's backend, TASK-0215 5c-3 P4): a glass layer is
//! blurred again only when its backdrop changed.
//!
//! WHY: every frame composes the whole set, so every glass layer would pay its snapshot and two
//! blur passes on every pointer move and keystroke — windowd's own `BackdropCache` never reaches
//! a GPU executor (its fields stay in the CPU path's commands).
//!
//! MODEL: a glass layer's blurred backdrop is a function of what is composited below it within
//! its reach. An FNV-1a hash folds each layer drawn before it whose extent reaches it, as drawn
//! (its transform applied); a glass layer's key is that fold and its own rectangle and radii
//! (RFC-0109: a layer moving elsewhere — a scrolled slice in another window — re-keys nothing).
//! The same key: the cached rows are read (one draw). Otherwise the blur is written into its
//! rows and read from them. A slot follows its layer's identity from one plan to the next
//! ([`GlassCache::carry`]) and keeps its rows while the glass keeps its height: a set
//! reshuffled, grown or shrunk moves no other glass's rows (before, rows packed in walk order
//! and every glass behind a new one was blurred again).
//!
//! ROOM: the cache is planes of `plane_rows` half-resolution rows, one behind the other in one
//! row space; a glass's rows lie inside one plane (a plane is one render target). A glass that
//! finds no rows is drawn without its backdrop this frame and counted in [`GlassCache::short`]:
//! the executor adds a plane ([`GlassCache::add_plane`]) and the next plan places it — the
//! cache grows with the glasses on the screen (TASK-0351: twelve windows had rows for five on
//! the board, the others lost their blur and damaged their whole rectangle every frame).
//!
//! PRINTS (TASK-0351 P19, W5 — a counter, no rule): a slot keeps three prints of what lay below
//! its glass when its rows were written (`GlassCache::printed`); a key miss at rest is counted
//! by the first that differs (`WritesBy::misses`, `gpud: glass rest misses`).

use alloc::vec::Vec;

use super::damage::Area;
use super::layers::{FrameLayer, MAX_LAYERS};
use super::plan::{Ident, LayerStep};

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// One FNV-1a fold step.
pub fn fold(h: u64, v: u64) -> u64 {
    (h ^ v).wrapping_mul(FNV_PRIME)
}

/// A frame's seed: the display's size and the wallpaper's generation (what every frame starts
/// from).
pub fn seed(display_w: u32, display_h: u32, wallpaper_epoch: u32) -> u64 {
    fold(
        fold(FNV_OFFSET, (u64::from(display_w) << 32) | u64::from(display_h)),
        u64::from(wallpaper_epoch),
    )
}

/// Folds one drawn layer (as drawn: overrides applied) into the destination so far. Every layer
/// folds — an opaque one changes what a glass above it blurs as much as a glass does. The
/// layer's geometry alone: a change of its content reaches a glass above it as the present's
/// damage (`Frame::plan`, TASK-0215 5c-3 P5), never through the key.
pub fn fold_layer(h: u64, l: &FrameLayer) -> u64 {
    let a = (u64::from(l.src_row_abs) << 32) | u64::from(l.src_x);
    let b = (u64::from(l.width) << 32) | u64::from(l.height);
    let c = (u64::from(l.dst_x) << 32) | u64::from(l.dst_y);
    let d = (u64::from(l.opacity) << 32) | u64::from(l.corner_radius);
    // The source, never its slot (RFC-0114 §6): a flip of a buffer's slots is new content, which
    // reaches the glass as damage — the key stays.
    fold(fold(fold(fold(fold(h, a), b), c), d), u64::from(l.source))
}

/// A glass layer's key: the destination below it, its rectangle, its blur and corner radius.
pub fn glass_key(below: u64, l: &FrameLayer) -> u64 {
    fold(
        fold(below, (u64::from(l.dst_x) << 32) | u64::from(l.dst_y)),
        (u64::from(l.width) << 40)
            | (u64::from(l.height) << 16)
            | (u64::from(l.backdrop_blur) << 8)
            | u64::from(l.corner_radius),
    )
}

/// Cache rows a glass's result takes: its height over its blur's scale, rounded up, and one
/// texel of margin above and below (the layer's bilinear fetches at the glass's edges read
/// blurred backdrop, never a neighbour's rows).
pub fn glass_rows(height: u32, scale: u32) -> u32 {
    height.div_ceil(scale.max(1)) + 2
}

/// Why a glass is written rather than read (TASK-0336 P0: the cause of every write, counted
/// per window in `gpud: glass writes`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteCause {
    /// The same backdrop in the same rows; only a band of it changed (TASK-0335 P1).
    Band,
    /// What lies below the glass changed (its key).
    Backdrop,
    /// No rows held this glass yet (or it changed its height).
    New,
    /// A full present: everything blurs again.
    Full,
}

/// What differs between the prints a glass's rows were written with and this plan's: the first
/// that does, in this order (TASK-0351 P19, W5 — why a key misses at rest; a counter, no rule).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Miss {
    /// The wallpaper epoch.
    Epoch,
    /// The layers below within its reach: one came, went or changed its place in the order — or
    /// the rows carry no prints.
    Set,
    /// Only the extents of the glass's own window's layers below it.
    Own,
    /// The extents of other windows' layers below it.
    Moved,
    /// None of these: something else the key folds (an opacity, a source, a corner radius, the
    /// glass's own rectangle or radii, the display).
    Drew,
}

/// Three prints of what lies below a glass within its reach, and the wallpaper epoch (W5): the
/// layers' identities (key, occurrence) in order; the identities and drawn extents of the other
/// windows' layers; the same of the glass's own window's layers. Kept with the rows at a write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Prints {
    set: u64,
    others: u64,
    own: u64,
    epoch: u32,
}

impl Prints {
    /// What differs from `was`, the prints the rows were written with.
    fn differs(self, was: Option<Self>) -> Miss {
        match was {
            None => Miss::Set,
            Some(w) if w.epoch != self.epoch => Miss::Epoch,
            Some(w) if w.set != self.set => Miss::Set,
            Some(w) if w.others != self.others => Miss::Moved,
            Some(w) if w.own != self.own => Miss::Own,
            Some(_) => Miss::Drew,
        }
    }
}

/// Folds an area into a print.
fn fold_area(h: u64, a: Area) -> u64 {
    match a {
        Area::Rect { x0, y0, x1, y1 } => fold(
            fold(h, (u64::from(x0) << 32) | u64::from(y0)),
            (u64::from(x1) << 32) | u64::from(y1),
        ),
        Area::Full => fold(h, u64::MAX),
        Area::None => fold(h, 0),
    }
}

/// What a glass layer does this frame. A row counts through the planes: plane `row /
/// plane_rows`, its row `row % plane_rows`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheStep {
    /// The backdrop is unchanged: one draw from the cache rows starting here.
    Read(u32),
    /// The backdrop changed: blurred into the cache rows here, then drawn from them.
    Write(u32),
    /// No rows left (or no cache): drawn without its backdrop, nothing kept.
    Off,
}

/// What one slot holds: its backdrop's key, its first row and how many (`h == 0`: none), what
/// its rows still owe — the backdrop changes a write was put off for — and whether the last
/// plan kept them through a motion. `last`: the key and base the last plan computed for its
/// glass; `now`: this plan's, handed on by [`GlassCache::carry`] — a glass the last plan did not
/// reach has none. `base`: the base its rows were blurred from, recorded at their write
/// ([`GlassCache::same_base`]). `displaced`: what changed below the glass outside its owe through
/// the plans that kept its rows ([`GlassCache::displace`]). `prints`: what lay below the glass
/// when its rows were last written (W5, a counter's input only). `full_by`: the rule that made
/// `owed` the whole glass, while it is (P19-e, a counter's input only).
#[derive(Clone, Copy, Default)]
struct Slot {
    key: u64,
    y: u32,
    h: u32,
    owed: Area,
    kept: bool,
    last: Option<(u64, u64)>,
    now: Option<(u64, u64)>,
    base: Option<u64>,
    displaced: Area,
    prints: Option<Prints>,
    full_by: Option<FullBy>,
}

/// The rule that made a slot's owe the whole glass (TASK-0351 P19-e: `gpud: glass rest misses
/// (… owed_by=put_off/lost/placed/displaced)`) — a counter's input, never a rule's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FullBy {
    /// A write put off on a key miss that nothing explained (`GlassCache::put_off`).
    PutOff,
    /// A hold on a key miss whose change list was lost (`GlassCache::hold`).
    Lost,
    /// A hold on a key hit whose change list was lost.
    Placed,
    /// A hold on a key miss owing a change while what changed below was not followed
    /// (`hold::displaced_owe`).
    Displaced,
}

impl Slot {
    /// The rows start over from this plan's backdrop: the base `computed` named (none for a
    /// write no plan computed), nothing displaced, nothing owed.
    fn written(&mut self) {
        (self.base, self.displaced) = (self.now.map(|(_, b)| b), Area::None);
        self.owe(Area::None, None);
    }

    /// The rows owe `owed`; when that makes the owe the whole glass, `by` is named the rule
    /// that did — one already whole keeps the rule that made it so.
    fn owe(&mut self, owed: Area, by: Option<FullBy>) {
        self.full_by = match owed {
            Area::Full if self.owed == Area::Full => self.full_by,
            Area::Full => by,
            _ => None,
        };
        self.owed = owed;
    }
}

/// The cache's bookkeeping, a slot per layer index — windowd encodes the scene back to front
/// deterministically, so a stable scene keeps its slots and a changed one changes its keys. The
/// slots grow with the plan (up to `MAX_LAYERS`), taken from the heap when a scene first needs
/// them.
pub struct GlassCache {
    slots: Vec<Slot>,
    /// The last plan's slots while `carry` hands them on.
    last: Vec<Slot>,
    /// A plane's height in half-resolution rows, and the planes the executor has.
    plane_rows: u32,
    planes: u32,
    /// Rows the glasses of the last plan found no place for.
    short: u32,
    /// Bumped whenever the wallpaper the frames start from is uploaded again.
    pub wallpaper_epoch: u32,
    /// The executor's cache exists (its one-shot setup succeeded).
    pub ready: bool,
}

impl GlassCache {
    /// A cache of one plane of `plane_rows` half-resolution rows, not ready yet.
    pub const fn new(plane_rows: u32) -> Self {
        Self {
            slots: Vec::new(),
            last: Vec::new(),
            plane_rows,
            planes: 1,
            short: 0,
            wallpaper_epoch: 0,
            ready: false,
        }
    }

    /// A plane's rows.
    pub const fn plane_rows(&self) -> u32 {
        self.plane_rows
    }

    /// The planes the cache has.
    pub const fn planes(&self) -> u32 {
        self.planes
    }

    /// Rows the last plan's glasses found no place for: what a further plane is asked for.
    pub const fn short(&self) -> u32 {
        self.short
    }

    /// The slots follow their layers into a new plan (RFC-0109): slot `i` takes what slot
    /// `carried[i]` held in the last plan — the same layer's key and rows — and nothing when the
    /// layer is new. There are as many slots as `carried` names: the others' rows are free.
    /// What the last plan computed for a glass becomes its `last`; this plan computes anew.
    pub fn carry(&mut self, carried: &[Option<usize>]) {
        core::mem::swap(&mut self.slots, &mut self.last);
        self.short = 0;
        self.slots.clear();
        let n = carried.len().min(MAX_LAYERS);
        let _ = self.slots.try_reserve(n);
        let room = self.slots.capacity().min(n);
        let last = &self.last;
        self.slots.extend(carried[..room].iter().map(|from| {
            let s = from.and_then(|j| last.get(j)).copied().unwrap_or_default();
            Slot { last: s.now, now: None, ..s }
        }));
    }

    /// Whether `slot`'s rows were blurred from `base` (TASK-0351 P6/P19, C-c and C-r): the
    /// frame's seed (display, wallpaper epoch) and the glass's own rest rectangle and radii
    /// (`glass_key(seed, rest)`) as they were at the write — then a key miss is a change of the
    /// layers below alone. Asked before the step that may write them.
    pub(super) fn same_base(&self, slot: usize, base: u64) -> bool {
        self.held(slot).base == Some(base)
    }

    /// What changed below `slot`'s glass outside its owe since its rows were blurred: the
    /// layers the plans that kept them moved, opened, closed or reordered (`displace`). Taken
    /// once a plan, with [`Self::rested`]: a plan that keeps the rows again hands it back. `Full`
    /// when the last plan did not compute the glass — nothing followed what changed there.
    pub(super) fn take_displaced(&mut self, slot: usize) -> Area {
        self.slots.get_mut(slot).map_or(Area::Full, |held| {
            let displaced = core::mem::take(&mut held.displaced);
            if held.last.is_some() {
                displaced
            } else {
                Area::Full
            }
        })
    }

    /// `slot`'s rows are kept through a plan whose layers below changed in `area` — where they
    /// will differ from the backdrop once the hold ends (C-r).
    pub(super) fn displace(&mut self, slot: usize, area: Area) {
        if let Some(held) = self.slots.get_mut(slot) {
            held.displaced = held.displaced.union(area);
        }
    }

    /// The key and base this plan computed for `slot`'s glass (the write records the base).
    pub(super) fn computed(&mut self, slot: usize, key: u64, base: u64) {
        if let Some(held) = self.slots.get_mut(slot) {
            held.now = Some((key, base));
        }
    }

    /// The prints of a glass of window `own` whose reach is `reach` (W5): the plan's `steps`
    /// below it with their identities `ids`, those whose `extent` touches the reach — the
    /// layers its key folds.
    pub(super) fn prints(
        &self,
        steps: &[LayerStep],
        ids: &[Ident],
        reach: Area,
        own: u32,
        extent: fn(&FrameLayer) -> Area,
    ) -> Prints {
        let mut p = Prints {
            set: FNV_OFFSET,
            others: FNV_OFFSET,
            own: FNV_OFFSET,
            epoch: self.wallpaper_epoch,
        };
        for (s, &(key, n)) in steps.iter().zip(ids) {
            let at = extent(&s.layer);
            if !at.touches(reach) {
                continue;
            }
            let id = (u64::from(key) << 16) | u64::from(n);
            p.set = fold(p.set, id);
            let side = if s.layer.layer_id == own { &mut p.own } else { &mut p.others };
            *side = fold_area(fold(*side, id), at);
        }
        p
    }

    /// `slot`'s glass took `step` with this plan's prints `now`: kept with rows written, or
    /// with none yet (`Off`, placed later); for a write, what differs from the prints its rows
    /// were last written with. A read keeps the rows' prints.
    pub(super) fn printed(&mut self, slot: usize, now: Prints, step: CacheStep) -> Option<Miss> {
        if matches!(step, CacheStep::Read(_)) {
            return None;
        }
        let held = self.slots.get_mut(slot)?;
        let was = core::mem::replace(&mut held.prints, Some(now));
        matches!(step, CacheStep::Write(_)).then(|| now.differs(was))
    }

    /// What `slot` holds: nothing for a slot the plan has not reached.
    fn held(&self, slot: usize) -> Slot {
        self.slots.get(slot).copied().unwrap_or_default()
    }

    /// `slot` to write: the slots grow to it; `None` past the bound or when the heap refuses.
    fn slot_mut(&mut self, slot: usize) -> Option<&mut Slot> {
        if slot >= MAX_LAYERS {
            return None;
        }
        if slot >= self.slots.len() {
            self.slots.try_reserve(slot + 1 - self.slots.len()).ok()?;
            self.slots.resize(slot + 1, Slot::default());
        }
        self.slots.get_mut(slot)
    }

    /// Why `slot` would be written for `key` in `h` rows rather than read — `Band` when it
    /// would hit (then only a touched band writes).
    pub fn miss(&self, slot: usize, key: u64, h: u32) -> WriteCause {
        let held = self.held(slot);
        if h == 0 || held.h != h {
            WriteCause::New
        } else if held.key != key {
            WriteCause::Backdrop
        } else {
            WriteCause::Band
        }
    }

    /// The rows `slot` holds kept as they are, whatever they show (TASK-0336 P3: a glass in
    /// motion keeps its last blur until it rests): a read when the slot holds `h` rows, the key
    /// left for the write that follows the motion; `None` when it holds none of that height
    /// (then the glass is written like any other).
    pub fn keep(&self, slot: usize, h: u32) -> Option<CacheStep> {
        let held = self.held(slot);
        (self.ready && h != 0 && held.h == h).then_some(CacheStep::Read(held.y))
    }

    /// `slot`'s rows are kept through a motion this plan ([`Self::keep`] was taken): the key
    /// stays the one they were blurred for, and `owed` is everything they miss of it by now —
    /// the write that ends the hold pays it (`hold`); `by`: the rule named should that make the
    /// owe the whole glass.
    pub(super) fn hold(&mut self, slot: usize, owed: Area, by: FullBy) {
        if let Some(held) = self.slots.get_mut(slot) {
            held.kept = true;
            held.owe(owed, Some(by));
        }
    }

    /// Whether the last plan kept `slot`'s rows through a motion — asked once a plan: the mark
    /// is taken, so a write this plan is the one write that follows the motion.
    pub(super) fn rested(&mut self, slot: usize) -> bool {
        self.slots.get_mut(slot).is_some_and(|held| core::mem::take(&mut held.kept))
    }

    /// What `slot`'s rows still owe: the backdrop changes its writes were put off for.
    pub fn owed(&self, slot: usize) -> Area {
        self.held(slot).owed
    }

    /// The rule that made `slot`'s owe the whole glass, while it is (P19-e, a counter's input).
    pub(super) fn full_by(&self, slot: usize) -> Option<FullBy> {
        self.held(slot).full_by
    }

    /// A write put off (TASK-0351 P6): what it would change of the glass lies under the layers
    /// above it, so the rows are kept and drawn as they are — the key taken, the change `owed`
    /// kept for the frame that shows it (a band; on a key miss the band C-c explains, P19-e, or
    /// `Full` when nothing does). `None` when the slot holds no rows of `h`: a glass without
    /// rows is written like any other.
    pub fn put_off(&mut self, slot: usize, key: u64, h: u32, owed: Area) -> Option<CacheStep> {
        if !self.ready || h == 0 {
            return None;
        }
        let held = self.slots.get_mut(slot).filter(|s| s.h == h)?;
        // The owe now names what the rows miss of this plan's backdrop: nothing is displaced.
        (held.key, held.displaced) = (key, Area::None);
        held.owe(owed, Some(FullBy::PutOff));
        Some(CacheStep::Read(held.y))
    }

    /// The lowest `h` free rows inside one plane: at a plane's start or right behind a held
    /// run — the first place that touches no slot's rows.
    fn place(&self, h: u32) -> Option<u32> {
        let runs = || self.slots.iter().filter(|s| s.h != 0);
        let held = |y: u32| runs().any(|s| y < s.y + s.h && s.y < y + h);
        (0..self.planes).find_map(|plane| {
            let (start, end) = (plane * self.plane_rows, (plane + 1) * self.plane_rows);
            let behind = runs().map(|s| s.y + s.h).filter(|&y| y > start && y < end);
            core::iter::once(start).chain(behind).filter(|&y| y + h <= end && !held(y)).min()
        })
    }

    /// Read, write or off for glass layer `slot` with key `key` and `h` rows. A slot keeps its
    /// rows while its glass keeps its height; a hit needs the same key and nothing changed
    /// within the glass's reach (`touched`). A glass without rows takes the lowest free ones;
    /// when no plane has them it is off and its rows are counted short.
    pub fn step(&mut self, slot: usize, key: u64, h: u32, touched: bool) -> CacheStep {
        let (ready, plane_rows) = (self.ready, self.plane_rows);
        let Some(held) = self.slot_mut(slot) else { return CacheStep::Off };
        if !ready || h == 0 || h > plane_rows {
            (held.h, held.base) = (0, None);
            held.owe(Area::None, None);
            return CacheStep::Off;
        }
        if held.h == h {
            let hit = !touched && held.key == key;
            held.key = key;
            if hit {
                return CacheStep::Read(held.y);
            }
            // The write pays what the rows owed and blurs them from this plan's base.
            held.written();
            return CacheStep::Write(held.y);
        }
        (held.key, held.h) = (key, 0);
        held.owe(Area::None, None);
        match self.retry(slot, h) {
            Some(y) => CacheStep::Write(y),
            None => CacheStep::Off,
        }
    }

    /// Rows for `slot`'s glass of `h` rows, which holds none: the lowest free ones, under the
    /// key its step named — asked again after the executor added a plane, so the frame that
    /// found the cache short still draws its glass. `None` counts the rows short.
    pub fn retry(&mut self, slot: usize, h: u32) -> Option<u32> {
        if h == 0 || h > self.plane_rows || self.slots.get(slot).is_none_or(|s| s.h != 0) {
            return None;
        }
        let Some(y) = self.place(h) else {
            self.short = self.short.saturating_add(h);
            return None;
        };
        let held = self.slots.get_mut(slot)?;
        (held.y, held.h) = (y, h);
        held.written();
        Some(y)
    }

    /// The executor made one more plane: its rows follow the last one's, and nothing is short
    /// until a glass asks again.
    pub fn add_plane(&mut self) {
        self.planes += 1;
        self.short = 0;
    }
}
