// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A frame's plan (TASK-0215 5c-3 P4): the retained set walked back to front — every layer as
//! drawn, a glass layer's cache step decided against the destination below it — and whether the
//! atlas's content must be uploaded first. Executors draw the plan's steps in order: per layer
//! its glass, its shadow, its content. The plan is the frame's own storage, reused frame after
//! frame: its tables grow when a scene has more layers than any before it and are kept, so a
//! settled scene's frame costs the heap nothing.

use alloc::vec::Vec;

use super::damage::Area;
use super::glass::{fold_layer, glass_key, glass_rows, seed, CacheStep, GlassCache, WriteCause};
use super::hold::Hold;
pub use super::hold::Holds;
use super::layers::{FrameLayer, RetainedLayers, MAX_LAYERS};
use super::occlude::{self, Px};
use super::reach::{px, Changes, GlassAt, Touch, WritesBy, GLASS_REACH};

/// Whether `a` and `b` draw the same pixels at the same place — the layer as drawn, its content
/// generation and its buffer's slot left out (a content change, a slot flip included, reaches
/// the screen as the present's damage — RFC-0114 §6: a flip is never a backdrop change). Every
/// other field counts, so two layers of the same drawing fold alike into a glass's key
/// (`fold_layer` reads neither field left out) and cover the same `extent`: a key miss C-c
/// writes as a band is one the layers this says changed explain (`tests::the_drawing_*`).
fn same_drawing(a: &FrameLayer, b: &FrameLayer) -> bool {
    let bare = |l: &FrameLayer| FrameLayer { content_epoch: 0, slot: 0, ..*l };
    bare(a) == bare(b)
}

/// Whether a layer of a surface buffer must be uploaded before it draws (RFC-0114 §6): its
/// buffer's content generation, buffer or slot changed since the layer last drew — or it is new,
/// or the executor lost its uploads. A layout layer is the frame's `upload`, never this.
fn buffer_upload(layer: &FrameLayer, before: Option<FrameLayer>, lost: bool) -> bool {
    layer.source != 0
        && (lost
            || before.is_none_or(|b| {
                (b.source, b.slot, b.content_epoch)
                    != (layer.source, layer.slot, layer.content_epoch)
            }))
}

/// The screen a layer covers: its rectangle and its shadow's halo.
fn extent(l: &FrameLayer) -> Area {
    let pad = if l.shadow_blur > 0 { l.shadow_blur + l.shadow_offset_y.unsigned_abs() } else { 0 };
    Area::rect(l.dst_x, l.dst_y, l.width, l.height).grown(pad)
}

/// The cache rows a glass layer's result takes.
fn rows_of(l: &FrameLayer) -> u32 {
    glass_rows(l.height, super::board::blur_scale(super::board::blur_radius(l.backdrop_blur)))
}

/// What the layers did between the last plan and this one (TASK-0351 P6, C-c): the set as
/// drawn, the last plan's layers as drawn, the last plan's step each layer carries on from
/// (`carried`), the step each of the last plan's is carried to (`kept`), and where layers that
/// swapped their order overlap (`reorder`).
struct Moves<'a> {
    drawn: &'a [FrameLayer],
    prev: &'a [FrameLayer],
    carried: &'a [Option<usize>],
    kept: &'a [Option<usize>],
    reorder: Area,
}

impl Moves<'_> {
    /// Where the backdrop of glass `i` (`reach`: its rest rectangle grown by the blur's reach)
    /// changed through its layers below: one below in both plans drawn otherwise (where it was
    /// and where it is), one that came below it (new, or above it before: where it is), one
    /// that went (gone, or above it now: where it was), and layers that swapped their order.
    /// With the frame's seed and the glass's own rectangle and radii unchanged, these are all a
    /// key miss can come from (`same_drawing`). `None` for a glass new to the plan;
    /// `Area::None` when nothing within its reach changed.
    fn below(&self, i: usize, reach: Area) -> Option<Area> {
        let was = self.carried.get(i).copied().flatten().filter(|w| *w < self.prev.len())?;
        let mut band = self.reorder;
        for (l, from) in self.drawn.iter().zip(self.carried).take(i) {
            let before = from.filter(|j| *j < was).and_then(|j| self.prev.get(j));
            band = band.union(match before {
                Some(b) if same_drawing(b, l) => Area::None,
                Some(b) => extent(b).union(extent(l)),
                None => extent(l),
            });
        }
        for (j, b) in self.prev.iter().enumerate().take(was) {
            if self.kept.get(j).copied().flatten().is_none_or(|now| now >= i) {
                band = band.union(extent(b));
            }
        }
        Some(band.intersect(reach))
    }
}

/// A glass layer's step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Glass {
    pub cache: CacheStep,
    /// Half-resolution rows its result takes: those of `at`.
    pub rows: u32,
    /// What a write blurs again (TASK-0335 P1): the damage below the glass within its reach
    /// when its rows already hold this backdrop — `Full` for a new backdrop or new rows.
    pub band: Area,
    /// The rectangle the rows hold the blur of (`x, y, w, h`): the layer's at REST, its
    /// window's transform not applied (RFC-0109, `hold`). A write blurs what lies below this
    /// rectangle; the step's layer is drawn where its transform puts it, with these rows spread
    /// over it. Without a transform it is the layer's own rectangle.
    pub at: Px,
}

/// One layer of the plan.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayerStep {
    /// The layer as drawn (its window's transform applied).
    pub layer: FrameLayer,
    /// Its glass, when it is a glass layer the executor blurs.
    pub glass: Option<Glass>,
    /// Its surface buffer's rows changed since it last drew (RFC-0114 §6): an executor that keeps
    /// a copy of the buffer (virgl's host texture) transfers the layer's rows first.
    pub upload: bool,
}

/// What the walk decided for the frame as a whole.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Planned {
    /// Layers in the plan (`Frame::step(0..layers)`).
    pub layers: usize,
    /// The glass writes of this frame by cause (TASK-0336 P0).
    pub writes: Writes,
    /// The same writes by what reached the glass (TASK-0351 P6).
    pub by: WritesBy,
    /// The atlas's content changed since its last upload: upload before the layers draw.
    pub upload: bool,
    /// What this frame changes on the screen (RFC-0101 §8): a plane shown this frame must be
    /// redrawn here at least — and wherever it lags earlier frames (`damage::History`).
    pub damage: Area,
    /// What the hold decided for the frame's glasses (TASK-0373, `gpud: glass holds (`).
    pub holds: Holds,
}

/// The glass writes of a frame by cause (`WriteCause`): what made each glass blur again.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Writes {
    pub band: u16,
    pub backdrop: u16,
    pub new: u16,
    pub full: u16,
    /// Of the band writes, those a change of the base reached (origin 0: the desktop's own
    /// surface); the others were reached by a window below the glass.
    pub band_base: u16,
    /// Of the backdrop writes, those written as a band (C-c): the key changed only by layers
    /// below that this plan moved, opened, closed or reordered — their extents are the band.
    pub backdrop_band: u16,
    /// Not writes: the glasses whose rows owe a change this frame because it lies under the
    /// layers above them (TASK-0351 P6) — counted every frame until one shows and pays it.
    pub hidden: u16,
}

impl Writes {
    fn note(&mut self, cause: WriteCause) {
        let n = match cause {
            WriteCause::Band => &mut self.band,
            WriteCause::Backdrop => &mut self.backdrop,
            WriteCause::New => &mut self.new,
            WriteCause::Full => &mut self.full,
        };
        *n = n.saturating_add(1);
    }

    /// Every write of the frame.
    pub fn total(self) -> u32 {
        u32::from(self.band) + u32::from(self.backdrop) + u32::from(self.new) + u32::from(self.full)
    }
}

/// A layer's identity in a plan (RFC-0109): its key and its occurrence among the layers of the
/// same key before it — unique in every set, so unnamed layers (key 0) still match by order.
pub type Ident = (u32, u16);

/// The retained set, the glass cache and the plan they give.
pub struct Frame {
    pub layers: RetainedLayers,
    pub glass: GlassCache,
    steps: Vec<LayerStep>,
    /// The identity of every step of the last plan.
    idents: Vec<Ident>,
    /// The last plan's layers as drawn, kept while this plan overwrites its steps.
    prev: Vec<FrameLayer>,
    /// A plan's working tables, kept between plans: the set as drawn, its identities, the
    /// last plan's step each layer carries on from, and the step each of the last plan's steps
    /// is carried to (`None`: gone).
    drawn: Vec<FrameLayer>,
    ids: Vec<Ident>,
    carried: Vec<Option<usize>>,
    kept: Vec<Option<usize>>,
    /// The opaque rectangles above a glass that touch what its write would change — and, while
    /// a change is asked for, the covers between it and the glass (`reach`).
    above: Vec<Px>,
    /// The frame's changes by their place in the walk (`reach`).
    changes: Changes,
    /// The layers in motion and the glasses held through it (`hold`).
    hold: Hold,
}

/// `v` emptied with room for `n` entries; how many it has room for (all of them unless the
/// heap refused).
fn room<T>(v: &mut Vec<T>, n: usize) -> usize {
    v.clear();
    let _ = v.try_reserve(n);
    v.capacity().min(n)
}

impl Frame {
    /// A frame whose glass cache has planes of `glass_rows` half-resolution rows.
    pub const fn new(glass_rows: u32) -> Self {
        Self {
            layers: RetainedLayers::new(),
            glass: GlassCache::new(glass_rows),
            steps: Vec::new(),
            idents: Vec::new(),
            prev: Vec::new(),
            drawn: Vec::new(),
            ids: Vec::new(),
            carried: Vec::new(),
            kept: Vec::new(),
            above: Vec::new(),
            changes: Changes::new(),
            hold: Hold::new(),
        }
    }

    /// Plans the frame on a display of `display` pixels. `blurs(radius)`: whether the executor
    /// blurs at that radius — a glass it cannot blur gets no step and takes no cache slot. The
    /// set counts as clean afterwards, its generation as uploaded when the plan asks for the
    /// upload: an executor that loses the upload says so (`RetainedLayers::invalidate_upload`).
    pub fn plan(&mut self, display: (u32, u32), blurs: &dyn Fn(u32) -> bool) -> Planned {
        let set_epoch = self.layers.set_epoch();
        let upload = if set_epoch != 0 {
            self.layers.dirty && set_epoch != self.layers.uploaded_epoch
        } else {
            self.layers.dirty
        };
        // What changed on the screen: the present's damage, every layer drawn otherwise than
        // in the last plan (where it was and where it is), every layer of the last plan that is
        // gone, and every glass blurred again this frame.
        let (named, named_count, full) = self.layers.take_damage();
        let named = &named[..named_count];
        let mut damage = named.iter().fold(Area::None, |a, (r, _)| a.union(*r));
        if full {
            damage = Area::Full;
        }
        // The last plan by identity (RFC-0109): a layer is compared with — and keeps the glass
        // rows of — the layer of its key in the last plan, wherever either stands in its list.
        let last = self.steps.len();
        let (mut drawn, mut ids, mut carried, mut kept, mut above) = (
            core::mem::take(&mut self.drawn),
            core::mem::take(&mut self.ids),
            core::mem::take(&mut self.carried),
            core::mem::take(&mut self.kept),
            core::mem::take(&mut self.above),
        );
        // The plan's tables grow to the set. A heap that refuses them plans no layer and
        // names the whole screen: nothing of the last plan can be compared.
        let want = self.layers.len().min(MAX_LAYERS);
        self.prev.clear();
        let fits = room(&mut drawn, want) == want
            && room(&mut ids, want) == want
            && room(&mut carried, want) == want
            && room(&mut kept, last) == last
            && room(&mut above, want) == want
            && self.changes.begin(want)
            && self.hold.begin(want)
            && self.prev.try_reserve(last).is_ok()
            && self.steps.try_reserve(want.saturating_sub(last)).is_ok()
            && self.idents.try_reserve(want.saturating_sub(last)).is_ok();
        let n = if fits {
            self.prev.extend(self.steps.iter().map(|s| s.layer));
            want
        } else {
            damage = Area::Full;
            0
        };
        drawn.extend((0..n).map(|i| self.layers.effective(i).unwrap_or(FrameLayer::EMPTY)));
        // A layer's identity: its key and its occurrence among the layers of that key before it.
        ids.extend(drawn.iter().enumerate().map(|(i, l)| {
            let before = drawn[..i].iter().filter(|b| b.key == l.key).count();
            (l.key, before.min(usize::from(u16::MAX)) as u16)
        }));
        kept.resize(self.prev.len(), None);
        for (i, id) in ids.iter().enumerate() {
            let from = self.idents.iter().position(|was| was == id);
            if let Some(j) = from.filter(|j| *j < kept.len()) {
                kept[j] = Some(i);
            }
            carried.push(from);
        }
        self.glass.carry(&carried);
        self.hold.carry(&carried);
        self.steps.clear();
        // A glass blurs its backdrop again for a change below it (`reach`): the present's
        // rectangles at the steps their origins name, the overlap of layers that swapped their
        // order — which changes the screen though neither is drawn otherwise — and, as the walk
        // goes up, every glass blurred again below it.
        let mut reorder = Area::None;
        if fits {
            self.changes.name(named, &self.layers, &drawn);
            reorder = self.changes.reorder(&carried, &drawn, extent);
            damage = damage.union(reorder);
        }
        let moves =
            Moves { drawn: &drawn, prev: &self.prev, carried: &carried, kept: &kept, reorder };
        let wallpaper = seed(display.0, display.1, self.glass.wallpaper_epoch);
        let (mut writes, mut by) = (Writes::default(), WritesBy::default());
        let mut motion = (0, false);
        for (i, &layer) in drawn.iter().enumerate() {
            let before = carried[i].map(|j| self.prev[j]);
            if before.is_none_or(|b| !same_drawing(&b, &layer)) {
                damage = damage.union(extent(&layer));
                if let Some(b) = before {
                    damage = damage.union(extent(&b));
                }
            }
            // A window's layers follow one another: its motion is asked once.
            if motion.0 != layer.layer_id {
                motion = (layer.layer_id, self.layers.in_motion(layer.layer_id));
            }
            if motion.1 {
                self.hold.mover(&layer, before.as_ref(), extent);
            }
            // A glass is planned from its layer at REST and drawn from the layer as drawn
            // (RFC-0109): its rows, its reach, its key and its band are the rest rectangle's,
            // so the rows of a panel's first frame are the rows it rests with.
            let rest = self.layers.rest(i).unwrap_or(layer);
            let glass = if rest.backdrop_blur == 0
                || rest.width == 0
                || rest.height == 0
                || !self.glass.ready
                || !blurs(rest.backdrop_blur)
            {
                None
            } else {
                let rows = rows_of(&rest);
                let rect = Area::rect(rest.dst_x, rest.dst_y, rest.width, rest.height);
                let seen = Area::rect(layer.dst_x, layer.dst_y, layer.width, layer.height);
                let reach = rect.grown(GLASS_REACH);
                // The changes below the glass within its reach: the band it blurs again
                // (TASK-0335 P1). A full present, a new backdrop (the key) or rows at a new
                // place blur the whole glass.
                let at = GlassAt { index: i, rect, id: layer.layer_id };
                let touch = if full {
                    Touch::FULL
                } else {
                    self.changes.touch(&at, &self.steps, &mut above)
                };
                let (rested, displaced) = (self.glass.rested(i), self.glass.take_displaced(i));
                // What the rows still owe — a write put off while nobody could see it, or
                // what a hold missed — is part of the band until a write pays it.
                let owed = self.glass.owed(i);
                let band = touch.band.union(owed);
                let touched = band != Area::None;
                // The backdrop is what lies below the glass within its reach (RFC-0109): a
                // layer drawn elsewhere — a scroll layer's slice in another window, a hover's
                // wash across the screen — changes nothing this glass blurs.
                let below = self
                    .steps
                    .iter()
                    .filter(|s| extent(&s.layer).touches(reach))
                    .fold(wallpaper, |h, s| fold_layer(h, &s.layer));
                // W5 (TASK-0351 P19, a counter): what the key folds, printed for a miss at rest.
                let prints = self.glass.prints(&self.steps, &ids, reach, at.id, extent);
                let (key, base) = (glass_key(below, &rest), glass_key(wallpaper, &rest));
                let miss = self.glass.miss(i, key, rows);
                // C-c and C-r (TASK-0351 P6/P19): a key miss of rows blurred from this base —
                // the same seed, the glass's own rectangle and radii — is a change of the layers
                // below alone; the plans that kept the rows since (`displaced`) and this one
                // (`Moves::below`) say where: a write blurs that band with the owe when it is a
                // rectangle — never for a glass new to the plan, a Full owe or displacement.
                let same = self.glass.same_base(i, base) && !full;
                self.glass.computed(i, key, base);
                let explained = (miss == WriteCause::Backdrop && same)
                    .then(|| moves.below(i, reach))
                    .flatten()
                    .map(|b| b.union(band).union(displaced))
                    .filter(|b| matches!(b, Area::Rect { .. }));
                let mut band = if miss == WriteCause::Band { band } else { Area::Full };
                // The hold (TASK-0351 P6, `hold`): a glass with rows whose window is in motion,
                // or with a layer in motion below it within its reach, keeps its last blur —
                // what nobody can see sharp is not computed. Asked before a write is put off:
                // a held glass's key stays the one its rows were blurred for.
                let kept = self.glass.keep(i, rows).filter(|_| !full);
                let radius = super::board::blur_radius(rest.backdrop_blur);
                let held = self.hold.holds(&at, kept.is_some(), motion.1, radius);
                // What a write changes on the screen: the glass where its backdrop did, grown
                // by the blur's reach — all of it where it is drawn while its rows are spread
                // over another rectangle than their own.
                let shows_of = |band: Area| match band {
                    _ if seen != rect => seen,
                    Area::Rect { .. } => band.grown(GLASS_REACH).intersect(rect),
                    _ => rect,
                };
                let mut shows = shows_of(band);
                let cache = if let Some(read) = kept.filter(|_| held) {
                    // What the held rows owe. While the layers below stand as the rows were
                    // blurred (the key), this frame's changes lie where they will lie at rest:
                    // the band. Displaced, a change lies and is covered as nothing says it
                    // will be once they are back: any makes the rows owe the whole glass.
                    let placed = miss == WriteCause::Band;
                    let foreign = !placed && self.changes.foreign(layer.layer_id, &drawn);
                    if foreign {
                        self.hold.foreign();
                    }
                    self.glass.hold(i, if placed || foreign { band } else { owed });
                    // C-r: where the layers below changed this plan is kept for the write
                    // that ends the hold (a glass not carried: anywhere).
                    let moved = moves.below(i, reach).map(|b| b.union(touch.band));
                    self.glass.displace(i, moved.map_or(Area::Full, |b| b.union(displaced)));
                    read
                } else {
                    // Cost follows what is seen (TASK-0351 P6): where opaque layers above hide
                    // all a write would change — a window under the windows opened over it —
                    // the write is put off, the rows owe it, and the frame that uncovers it
                    // pays. Twenty cascaded windows blurred three or four glasses a frame for
                    // a change one of them showed (`board--2026-11-18T13-29-53`).
                    let hidden = (touched || miss != WriteCause::Band)
                        && px(shows).is_some_and(|q| {
                            above.clear();
                            for (j, l) in drawn.iter().enumerate().skip(i + 1) {
                                // A glass above hides with its drawn back: one that holds its
                                // rows (it reads or writes them this frame, never draws
                                // without).
                                let back = l.backdrop_blur != 0
                                    && blurs(l.backdrop_blur)
                                    && self
                                        .glass
                                        .keep(j, rows_of(&self.layers.rest(j).unwrap_or(*l)))
                                        .is_some();
                                let hides = occlude::opaque_rect(l, back);
                                above.extend(hides.filter(|o| occlude::intersects(*o, q)));
                            }
                            occlude::covered(q, &above)
                        });
                    let put_off =
                        if hidden { self.glass.put_off(i, key, rows, band) } else { None };
                    match put_off {
                        Some(read) => {
                            writes.hidden = writes.hidden.saturating_add(1);
                            read
                        }
                        None => {
                            let step = self.glass.step(i, key, rows, touched);
                            // The write a key miss C-c explained blurs its band alone; one
                            // put off owes the whole glass as before.
                            if let (CacheStep::Write(_), Some(b)) = (step, explained) {
                                (band, shows) = (b, shows_of(b));
                                writes.backdrop_band = writes.backdrop_band.saturating_add(1);
                                // A rest miss written as a band smaller than the glass (C-r).
                                if rested && b.grown(GLASS_REACH).intersect(rect) != rect {
                                    by.misses.band = by.misses.band.saturating_add(1);
                                }
                            }
                            step
                        }
                    }
                };
                // What the glass shows changes where its blurred backdrop did — the frame's
                // damage, and a change of this step for the glasses above it; a glass blurred
                // live changes wherever it is drawn.
                let differs = self.glass.printed(i, prints, cache);
                let shown = match cache {
                    CacheStep::Read(_) => Area::None,
                    CacheStep::Write(_) => {
                        let cause = if full { WriteCause::Full } else { miss };
                        writes.note(cause);
                        by.note(&touch, cause, rested, differs);
                        if miss == WriteCause::Band && touch.base {
                            writes.band_base = writes.band_base.saturating_add(1);
                        }
                        shows
                    }
                    CacheStep::Off => seen,
                };
                damage = damage.union(shown);
                self.changes.written(i, shown);
                let at = (rest.dst_x, rest.dst_y, rest.width, rest.height);
                Some(Glass { cache, rows, band, at })
            };
            let upload = buffer_upload(&layer, before, self.layers.buffers_lost);
            self.steps.push(LayerStep { layer, glass, upload });
        }
        // A layer of the last plan no layer of this one carries is gone: where it was changes.
        for (gone, _) in self.prev.iter().zip(kept.iter()).filter(|(_, k)| k.is_none()) {
            damage = damage.union(extent(gone));
        }
        let count = self.steps.len();
        self.idents.clear();
        self.idents.extend_from_slice(&ids);
        (self.drawn, self.ids, self.carried, self.kept) = (drawn, ids, carried, kept);
        self.above = above;
        self.layers.dirty = false;
        self.layers.buffers_lost = false;
        if upload && set_epoch != 0 {
            self.layers.uploaded_epoch = set_epoch;
        }
        // A plan without room asked the hold nothing: no count of the last one is carried.
        let holds = if fits { self.hold.counts() } else { Holds::NONE };
        Planned { layers: count, writes, by, upload, damage, holds }
    }

    /// The cache grew by a plane after the last plan found it short (`GlassCache::short`): the
    /// plan's glasses without rows take theirs and are written whole this frame — their
    /// rectangles are in the plan's damage already. Returns how many were placed (writes of
    /// cause `New`); what still finds no rows is short again.
    pub fn place_short(&mut self) -> u16 {
        self.glass.add_plane();
        let mut placed = 0;
        for (i, step) in self.steps.iter_mut().enumerate() {
            let Some(g) = step.glass.as_mut().filter(|g| g.cache == CacheStep::Off) else {
                continue;
            };
            if let Some(y) = self.glass.retry(i, g.rows) {
                (g.cache, g.band) = (CacheStep::Write(y), Area::Full);
                placed += 1;
            }
        }
        placed
    }

    /// Step `i` of the last plan.
    pub fn step(&self, i: usize) -> Option<LayerStep> {
        self.steps.get(i).copied()
    }

    /// The cache step of step `i`'s glass, when it is one the executor blurs.
    pub fn cache_step(&self, i: usize) -> Option<CacheStep> {
        self.steps.get(i).and_then(|s| s.glass).map(|g| g.cache)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `same_drawing` compares every field but the content generation and the slot; neither
    /// moves a layer's fold or extent — so the same drawing folds alike (C-c's premise).
    #[test]
    fn the_drawing_covers_every_field_a_key_folds() {
        let l = FrameLayer { width: 40, height: 30, opacity: 255, source: 3, ..FrameLayer::EMPTY };
        let flipped = FrameLayer { content_epoch: 9, slot: 1, ..l };
        assert!(same_drawing(&l, &flipped));
        assert_eq!((fold_layer(1, &l), extent(&l)), (fold_layer(1, &flipped), extent(&flipped)));
        let folded: [fn(&mut FrameLayer); 9] = [
            |l| l.src_row_abs += 1,
            |l| l.src_x += 1,
            |l| l.width += 1,
            |l| l.height += 1,
            |l| l.dst_x += 1,
            |l| l.dst_y += 1,
            |l| l.opacity -= 1,
            |l| l.corner_radius += 1,
            |l| l.source += 1,
        ];
        for change in folded {
            let mut m = l;
            change(&mut m);
            assert!(fold_layer(1, &l) != fold_layer(1, &m) && !same_drawing(&l, &m));
        }
    }
}
