// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: a client surface's declared layers (RFC-0109) as windowd keeps them, and the pure
//! geometry its compositor composes from: a scroll layer's visible window cut into its pieces
//! (never a position its band does not hold), a wash placed in its parent and clipped to it,
//! what a new structure changes on the screen, who may declare layers on a surface. Host-tested here; the OS-only runtime (`compositor::runtime::
//! surface_layers`) keeps the state per surface and encodes it (`surface_layers_encode`).
//! windowd runs no scroll physics and decides no hover (ADR-0076).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `mod tests` below

use crate::motion_geometry::{carry, stamp, FrameNote, MotionState, Said};
use nexus_display_proto::client_surface::{LayerDesc, MAX_SURFACE_LAYERS};
use nexus_display_proto::surface_layers::{
    self as proto, MotionLayerDesc, ScrollLayerDesc, SurfaceMotion, SurfaceTree, Wash,
    AXIS_VERTICAL, MAX_MOTION_LAYERS,
};

/// A surface's declared layers.
#[derive(Clone, Copy)]
pub(crate) struct SurfaceLayers {
    /// The structure in effect.
    pub(crate) tree: SurfaceTree,
    /// A structure waiting for the surface's next present (the pixels it names).
    pending: Option<SurfaceTree>,
    /// The motion in effect.
    pub(crate) motion: SurfaceMotion,
    /// A refusal was said for this surface (once per surface: a broken client must not flood).
    refusal_said: bool,
    /// Each motion layer's motion — its segment (start, direction, turn, backdrop) and a flip
    /// waiting for its frame — carried by id across structures (`motion_geometry::carry`).
    pub(crate) motion_states: [MotionState; MAX_MOTION_LAYERS],
    /// The shown time of the last frame that drew these layers (`motion_geometry::frame_owed`).
    pub(crate) drawn_at: u64,
}

/// The most rectangles a structure change names before it counts as the whole surface: every
/// region, every motion layer and every scroll layer of the structure before it and after it.
pub(crate) const CHANGED_MAX: usize =
    2 * (MAX_SURFACE_LAYERS + MAX_MOTION_LAYERS + proto::MAX_SCROLL_LAYERS);

// `unmatched` marks the second list's entries in one table: every list it pairs fits in it.
const _: () = assert!(proto::MAX_SCROLL_LAYERS <= MAX_SURFACE_LAYERS + MAX_MOTION_LAYERS);
// `TreeChange::bands` holds one bit per scroll layer.
const _: () = assert!(proto::MAX_SCROLL_LAYERS <= u8::BITS as usize);

/// How far a region's drop shadow reaches past its rectangle (`material_glass`: blur 24, 8 down).
const SHADOW_REACH: u32 = 32;

/// What a present's structure changed on the screen: the whole surface (viewports, bands' shapes,
/// patches — or more rectangles than it names), the rectangles that changed (a region moved or
/// re-dressed: where it was and is — TASK-0351 P15, a panel sliding under the bar damages its own
/// columns, not every pixel of the desktop; a region or a motion layer that came or went: where
/// it is or was — TASK-0354 P2), and the bands whose content changed (bit `k`: scroll layer
/// `k`'s valid range).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TreeChange {
    pub(crate) whole: bool,
    rects: [Rect; CHANGED_MAX],
    /// Per rectangle: the region of the new structure that draws the change, when one does.
    regions: [Option<u8>; CHANGED_MAX],
    count: usize,
    pub(crate) bands: u8,
}

impl TreeChange {
    pub(crate) const NONE: Self = Self {
        whole: false,
        rects: [(0, 0, 0, 0); CHANGED_MAX],
        regions: [None; CHANGED_MAX],
        count: 0,
        bands: 0,
    };
    pub(crate) const STRUCTURE: Self = Self { whole: true, ..Self::NONE };

    /// The rectangles that changed (surface pixels).
    pub(crate) fn rects(&self) -> &[Rect] {
        &self.rects[..self.count]
    }

    /// The region of the structure now in effect that draws rectangle `i`'s change — one that
    /// moved within its level or came (the damage's origin, RFC-0101 §8). `None`: no region
    /// that is still there draws it (one that went or changed its material, a scroll layer's
    /// viewport, a motion layer) — the change is the surface's.
    pub(crate) fn region_of(&self, i: usize) -> Option<usize> {
        self.regions[..self.count].get(i).copied().flatten().map(usize::from)
    }

    /// One more changed rectangle; past the bound the change is the whole surface.
    fn push(&mut self, r: Rect) {
        self.push_from(r, None);
    }

    /// One more changed rectangle, drawn by region `region` of the new structure.
    fn push_from(&mut self, r: Rect, region: Option<usize>) {
        match self.rects.get_mut(self.count) {
            Some(slot) => {
                *slot = r;
                self.regions[self.count] = region.and_then(|i| u8::try_from(i).ok());
                self.count += 1;
            }
            None => self.whole = true,
        }
    }

    #[cfg(test)]
    const fn bands(bands: u8) -> Self {
        Self { bands, ..Self::NONE }
    }
}

/// A rectangle in surface pixels.
type Rect = (u32, u32, u32, u32);

/// Region `l`'s rectangle on the screen, its drop shadow included.
fn region_rect(l: &LayerDesc) -> Rect {
    let (x, y, w, h) = (u32::from(l.x), u32::from(l.y), u32::from(l.w), u32::from(l.h));
    if l.shadow_alpha == 0 {
        return (x, y, w, h);
    }
    let (x0, y0) = (x.saturating_sub(SHADOW_REACH), y.saturating_sub(SHADOW_REACH));
    (x0, y0, x + w + SHADOW_REACH - x0, y + h + SHADOW_REACH - y0)
}

/// The least rectangle holding `a` and `b`.
fn span(a: Rect, b: Rect) -> Rect {
    let (x0, y0) = (a.0.min(b.0), a.1.min(b.1));
    let (x1, y1) = ((a.0 + a.2).max(b.0 + b.2), (a.1 + a.3).max(b.1 + b.3));
    (x0, y0, x1 - x0, y1 - y0)
}

impl SurfaceLayers {
    pub(crate) const EMPTY: Self = Self {
        tree: SurfaceTree::EMPTY,
        pending: None,
        motion: SurfaceMotion {
            offsets: [0; proto::MAX_SCROLL_LAYERS],
            offset_count: 0,
            washes: [Wash { parent: 0, patch: 0, radius: 0, opacity: 0, x: 0, y: 0, w: 0, h: 0 };
                proto::MAX_WASHES],
            wash_count: 0,
        },
        refusal_said: false,
        motion_states: [MotionState::EMPTY; MAX_MOTION_LAYERS],
        drawn_at: 0,
    };

    /// A new surface: nothing declared yet (fail-closed — no region, no band, no wash).
    pub(crate) fn clear(&mut self) {
        *self = Self::EMPTY;
    }

    /// The present: the waiting structure takes effect — its motion layers keep their motion by
    /// id, a flipped `exit` waits for the next frame (`motion_geometry::carry`). Says what
    /// changed on the screen. Only the owner's present reaches here (`owns`).
    pub(crate) fn activate(&mut self) -> TreeChange {
        let Some(next) = self.pending.take() else { return TreeChange::NONE };
        let change = structure_change(&self.tree, &next);
        self.motion_states = carry(&self.tree, &self.motion_states, &next);
        self.tree = next;
        change
    }

    /// The frame shown at `at` (it redraws `note`) draws these layers: a flip waiting for it
    /// turns, a motion drawn for the first time starts, one that ended is said once
    /// (`motion_geometry::stamp`) — what each motion layer did, by index.
    #[cfg(test)]
    pub(crate) fn stamp(&mut self, at: u64, note: FrameNote) -> [Said; MAX_MOTION_LAYERS] {
        self.stamp_by(at, |_| note)
    }

    /// [`Self::stamp`] with each motion layer's own note (`note(k)`: what the frame redraws of
    /// layer `k` — the damage whose origin is the layer's).
    pub(crate) fn stamp_by(
        &mut self,
        at: u64,
        note: impl Fn(usize) -> FrameNote,
    ) -> [Said; MAX_MOTION_LAYERS] {
        self.drawn_at = at;
        let mut said = [Said::Nothing; MAX_MOTION_LAYERS];
        for (k, m) in self.tree.motions().iter().enumerate() {
            said[k] = stamp(m, &mut self.motion_states[k], at, note(k));
        }
        said
    }

    /// A structure for the surface's next present.
    pub(crate) fn stage(&mut self, tree: SurfaceTree) {
        self.pending = Some(tree);
    }

    /// The structure a motion is checked against: the one waiting for its present, else the one
    /// in effect (a motion may race the present of the structure it names).
    pub(crate) fn latest_tree(&self) -> SurfaceTree {
        self.pending.unwrap_or(self.tree)
    }

    /// Whether this is the surface's first refusal (said once: a broken client must not flood).
    pub(crate) fn first_refusal(&mut self) -> bool {
        !core::mem::replace(&mut self.refusal_said, true)
    }

    /// Whether these layers owe a frame for a motion layer: not drawn yet, or still moving in the
    /// last frame drawn (`motion_geometry::frame_owed`).
    pub(crate) fn motion_frame_owed(&self) -> bool {
        crate::motion_geometry::frame_owed(&self.tree, &self.motion_states, self.drawn_at)
    }

    /// The motion's offset of scroll layer `k` (0 when the motion names none yet).
    pub(crate) fn offset(&self, k: usize) -> i32 {
        self.motion.offsets().get(k).copied().unwrap_or(0)
    }

    /// The most layers this declaration composes in a frame — its motion layers' own pixels and
    /// backdrops, its regions, its scroll layers' pieces at their offsets, its washes: the
    /// surface's demand on the frame's bound.
    pub(crate) fn demand(&self) -> usize {
        let mut out = [Piece::default(); MAX_PIECES];
        let slices: usize = (0..self.tree.scrolls().len())
            .map(|k| pieces(&self.tree.scrolls()[k], self.offset(k), &mut out))
            .sum();
        let backdrops = self.tree.motions().iter().filter(|m| m.backdrop().is_some()).count();
        self.tree.motions().len()
            + backdrops
            + self.tree.regions().len()
            + slices
            + self.motion.washes().len()
    }
}

/// What `next` changes against `now`: the same regions moved or re-dressed → where each was and
/// is; regions or motion layers that came or went → where each is or was (a layer with a
/// backdrop: all — its backdrop spans the surface); a scroll layer that came, went or whose band
/// was re-placed → its viewport (TASK-0351 P15: the launcher bringing its grid's band is not a
/// full frame); a valid range alone → its band; patches → all.
fn structure_change(now: &SurfaceTree, next: &SurfaceTree) -> TreeChange {
    if now.patches() != next.patches() {
        return TreeChange::STRUCTURE;
    }
    let mut change = TreeChange::NONE;
    // A scroll layer is the same while everything but its valid range is: matched by that shape
    // (an index may shift when another comes or goes), a new range is its band's rows.
    let shape = |s: &ScrollLayerDesc| ScrollLayerDesc { valid_start: 0, valid_len: 0, ..*s };
    let viewport =
        |s: &ScrollLayerDesc| (u32::from(s.vx), u32::from(s.vy), u32::from(s.vw), u32::from(s.vh));
    let mut shapes = [ScrollLayerDesc::default(); 2 * proto::MAX_SCROLL_LAYERS];
    for (slot, s) in shapes.iter_mut().zip(now.scrolls().iter().chain(next.scrolls())) {
        *slot = shape(s);
    }
    let (old_shapes, new_shapes) = shapes.split_at(now.scrolls().len());
    unmatched(old_shapes, &new_shapes[..next.scrolls().len()], |s| change.push(viewport(s)));
    for (k, b) in next.scrolls().iter().enumerate() {
        let held = now.scrolls().iter().find(|a| shape(a) == shape(b));
        if held.is_some_and(|a| (a.valid_start, a.valid_len) != (b.valid_start, b.valid_len)) {
            change.bands |= 1 << k;
        }
    }
    if now.regions().len() == next.regions().len() {
        for (i, (a, b)) in now.regions().iter().zip(next.regions()).enumerate() {
            if a != b {
                // The region draws the change while it stays what it is: another material or
                // glass level may draw it in another pass, below where it was.
                let level = (a.material, a.glass_level) == (b.material, b.glass_level);
                change.push_from(span(region_rect(a), region_rect(b)), level.then_some(i));
            }
        }
    } else {
        // One that came is in the new structure; one that went is no layer any more.
        unmatched(now.regions(), next.regions(), |l| {
            change.push_from(region_rect(l), next.regions().iter().position(|n| n == l));
        });
    }
    // A motion layer is the same while its place, its pixels and what it claims are: its
    // durations falling to 0 (the app's note that the motion and its scrim's fade are over)
    // change nothing on screen, nor does its direction flipping (a close or a re-open: windowd
    // moves it — TASK-0354 P9).
    let key =
        |m: &MotionLayerDesc| MotionLayerDesc { duration_ms: 0, backdrop_ms: 0, exit: false, ..*m };
    let (old, new) = (now.motions(), next.motions());
    let mut keys = [MotionLayerDesc::EMPTY; 2 * MAX_MOTION_LAYERS];
    for (slot, m) in keys.iter_mut().zip(old.iter().chain(new)) {
        *slot = key(m);
    }
    let (old_keys, new_keys) = keys.split_at(old.len());
    unmatched(old_keys, &new_keys[..new.len()], |m| match m.backdrop() {
        Some(_) => change.whole = true,
        None => change.push(m.rest()),
    });
    change
}

/// Every entry of `a` with no equal entry in `b` and every entry of `b` with none in `a` — each
/// equal pair matched once (a list may hold the same entry twice).
fn unmatched<T: PartialEq>(a: &[T], b: &[T], mut each: impl FnMut(&T)) {
    let mut used = [false; MAX_SURFACE_LAYERS + MAX_MOTION_LAYERS];
    for x in a {
        match b.iter().enumerate().position(|(j, y)| !used[j] && y == x) {
            Some(j) => used[j] = true,
            None => each(x),
        }
    }
    for (j, y) in b.iter().enumerate() {
        if !used[j] {
            each(y);
        }
    }
}

/// One piece of a scroll layer's visible window: a rectangle of its band and where it lands, both
/// in surface coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Piece {
    pub(crate) src: (u32, u32),
    pub(crate) dst: (u32, u32),
    pub(crate) size: (u32, u32),
}

/// The most pieces a visible window takes: strips are at least half a viewport long, so a window
/// crosses at most two strip boundaries (the ring's wrap is one of them).
pub(crate) const MAX_PIECES: usize = 3;

/// The pieces of scroll layer `s` at `offset` (RFC-0109): the visible window
/// `[offset, offset + viewport)` intersected with the band's valid range, cut where a strip ends
/// (and where the ring wraps). Never a position the ring does not hold — the rest of the viewport
/// shows the base (the container's own background) until the band fills.
pub(crate) fn pieces(s: &ScrollLayerDesc, offset: i32, out: &mut [Piece; MAX_PIECES]) -> usize {
    let len = i64::from(s.viewport_len());
    let (valid0, valid1) =
        (i64::from(s.valid_start), i64::from(s.valid_start) + i64::from(s.valid_len));
    let o = i64::from(offset);
    let c0 = o.max(valid0).max(0);
    let c1 = (o + len).min(valid1);
    let mut p = c0;
    let mut n = 0;
    while p < c1 && n < MAX_PIECES {
        let Some((x, y, run)) = s.ring_place(p as u32) else { break };
        let take = (c1 - p).min(i64::from(run)) as u32;
        let along = (p - o) as u32;
        out[n] = if s.axis == AXIS_VERTICAL {
            Piece {
                src: (x, y),
                dst: (u32::from(s.vx), u32::from(s.vy) + along),
                size: (u32::from(s.vw), take),
            }
        } else {
            Piece {
                src: (x, y),
                dst: (u32::from(s.vx) + along, u32::from(s.vy)),
                size: (take, u32::from(s.vh)),
            }
        };
        n += 1;
        p += i64::from(take);
    }
    n
}

/// Where wash `w` lands in surface coordinates `(x, y, w, h)`: its parent's content coordinates
/// moved by the parent's offset and clipped to the parent's viewport (the base: the base layer).
/// `None` when nothing of it is visible or its parent is not declared.
pub(crate) fn wash_place(w: &Wash, layers: &SurfaceLayers, base: (u32, u32)) -> Option<Piece> {
    let (x, y) = (i64::from(w.x), i64::from(w.y));
    let (rect, clip) = match (w.motion_parent(), usize::from(w.parent)) {
        // A motion layer's wash: relative to its rest, clipped to it (it moves with the layer).
        (Some(m), _) => {
            let (mx, my, mw, mh) = layers.tree.motions().get(m)?.rest();
            let (mx, my) = (i64::from(mx), i64::from(my));
            ((mx + x, my + y), (mx, my, i64::from(mw), i64::from(mh)))
        }
        (None, 0) => ((x, y), (0, 0, i64::from(base.0), i64::from(base.1))),
        (None, k) => {
            let s = layers.tree.scrolls().get(k - 1)?;
            let o = i64::from(layers.offset(k - 1));
            let (vx, vy) = (i64::from(s.vx), i64::from(s.vy));
            let at =
                if s.axis == AXIS_VERTICAL { (vx + x, vy + y - o) } else { (vx + x - o, vy + y) };
            (at, (vx, vy, i64::from(s.vw), i64::from(s.vh)))
        }
    };
    let x0 = rect.0.max(clip.0);
    let y0 = rect.1.max(clip.1);
    let x1 = (rect.0 + i64::from(w.w)).min(clip.0 + clip.2);
    let y1 = (rect.1 + i64::from(w.h)).min(clip.1 + clip.3);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let patch = layers.tree.patches().get(usize::from(w.patch))?;
    Some(Piece {
        src: (u32::from(patch.x) + 1, u32::from(patch.y) + 1),
        dst: (x0 as u32, y0 as u32),
        size: ((x1 - x0) as u32, (y1 - y0) as u32),
    })
}

/// Whether a frame's sender owns the surface it names: the kernel's sender id equals the one
/// captured at the surface's create, and the surface had a named creator at all (an unnamed
/// spawn's 0 owns nothing).
pub(crate) fn owns(owner_sid: u64, sender_sid: u64) -> bool {
    owner_sid != 0 && owner_sid == sender_sid
}

/// Screen rectangles one store rectangle of a present can damage: a band's viewport each, a motion
/// layer's rest each.
#[cfg(test)]
pub(crate) const STORE_DAMAGE_MAX: usize = proto::MAX_SCROLL_LAYERS + MAX_MOTION_LAYERS;

/// A layer of a surface whose pixels are rows of its store: motion layer `k`, scroll layer `k`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StoreLayer {
    Motion(usize),
    Scroll(usize),
}

/// The screen damage of a present's rectangle in the surface's STORE: the viewports of the scroll
/// layers whose band it touches and the rests of the motion layers whose pixels it touches, in
/// surface coordinates (a moving layer is redrawn where it is by the planner, TASK-0354 P2).
/// Patches change nothing on the screen by themselves (a wash using one is motion).
#[cfg(test)]
pub(crate) fn store_damage(
    layers: &SurfaceLayers,
    rect: (u32, u32, u32, u32),
    out: &mut [(u32, u32, u32, u32); STORE_DAMAGE_MAX],
) -> usize {
    let mut n = 0;
    store_damage_each(layers, rect, |_, view| {
        out[n] = view;
        n += 1;
    });
    n
}

/// [`store_damage`], each rectangle with the layer whose rows changed — the layer that draws it
/// (the damage's origin, RFC-0101 §8).
pub(crate) fn store_damage_each(
    layers: &SurfaceLayers,
    rect: (u32, u32, u32, u32),
    mut each: impl FnMut(StoreLayer, Rect),
) {
    let touches = |b: Rect| {
        rect.0 < b.0 + b.2 && b.0 < rect.0 + rect.2 && rect.1 < b.1 + b.3 && b.1 < rect.1 + rect.3
    };
    for (k, m) in layers.tree.motions().iter().enumerate() {
        if touches((u32::from(m.sx), u32::from(m.sy), u32::from(m.w), u32::from(m.h))) {
            each(StoreLayer::Motion(k), m.rest());
        }
    }
    for (k, s) in layers.tree.scrolls().iter().enumerate() {
        if touches(s.band_rect()) && s.valid_len > 0 {
            let viewport = (u32::from(s.vx), u32::from(s.vy), u32::from(s.vw), u32::from(s.vh));
            each(StoreLayer::Scroll(k), viewport);
        }
    }
}

#[cfg(test)]
#[path = "layer_geometry_tests.rs"]
mod tests;
