// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: a motion layer's store is a CACHE of its subtree's rest image (RFC-0109 §motion
//! layers, package B 1a). Each motion slot keeps a `Snapshot` of what its store rows HOLD: the
//! rest rect, the store place, what the painter reads that the layout does not hold (the theme,
//! the backdrop's colour, an animated node, the focus, the scroll views), and the subtree's boxes
//! and text runs numbered from 1 — a re-emit that renumbers the nodes keeps it equal. Where the
//! app used to throw the store away and paint it again (a ghost revived, a re-resolve that only
//! renumbered, a new place), it compares instead: the same subtree at the same place is KEPT,
//! changed rows are painted again (the P5 path), anything not comparable is painted WHOLE. A
//! ghost that is dropped PARKS its place and snapshot for a re-enter of the same identity and
//! size. Same one-cfg shape as `motion_exit`: the probe (`probe/motion_layers.rs`,
//! `probe/store.rs`) acts, the decisions live here where host tests reach them.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `motion_reuse_tests.rs` (host): the 200-sequence oracle against a fresh paint,
//!   the rejects (a changed subtree, a compaction, a theme), a reopen that paints nothing, the
//!   order of giving way
//! INVARIANTS: a valid snapshot describes what its store rows hold; Keep and Rows only between
//!   two comparable snapshots at the same rect and place; dropping a snapshot costs a repaint,
//!   never a wrong pixel

extern crate alloc;

use crate::layout_diff::{changed_rows, TextRun};
use crate::motion_math::Span;
use crate::store_alloc::Outcome;
use alloc::vec::Vec;
use nexus_layout::LayoutBox;

/// The motion slots (the wire's bound).
pub(crate) const MAX_MOTION_LAYERS: usize = nexus_display_proto::surface_layers::MAX_MOTION_LAYERS;
/// The parked stores a surface keeps; past it the oldest goes.
pub(crate) const MAX_PARKED: usize = MAX_MOTION_LAYERS;
/// The scroll roots a snapshot names; a subtree that names more is not comparable.
const MAX_VIEWS: usize = 8;

/// How a scroll root the subtree's boxes name was painted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum View {
    /// No layer: its content paints at its layout place.
    #[default]
    Plain,
    /// An uncovered layer: its content is drawn in its band, never in the store.
    Band,
    /// A covered layer or a flat view: its content paints clipped and moved.
    Shown { clip: Span, dx: i32, dy: i32 },
}

/// What the painter reads for a subtree that the layout does not hold.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Inputs {
    pub(crate) theme: u8,
    /// The backdrop's colour (a scrim root the compositor draws from a patch).
    pub(crate) backdrop: Option<[u8; 4]>,
    /// A node of the subtree paints at a transform off identity.
    pub(crate) animated: bool,
    /// The focus (a caret, a selection, a field's scroll) is inside.
    pub(crate) focus: bool,
    /// The scroll roots the boxes name (numbered from the subtree's first node) and their views.
    views: [(isize, View); MAX_VIEWS],
    nv: usize,
    overflow: bool,
}

impl Inputs {
    pub(crate) fn new(theme: u8, backdrop: Option<[u8; 4]>) -> Self {
        Self { theme, backdrop, ..Self::default() }
    }

    /// Scroll root `root` (numbered from the subtree's first node) painted as `view`, once.
    pub(crate) fn view(&mut self, root: isize, view: View) {
        if self.views[..self.nv].iter().any(|v| v.0 == root) {
            return;
        }
        match self.views.get_mut(self.nv) {
            Some(slot) => {
                *slot = (root, view);
                self.nv += 1;
            }
            None => self.overflow = true,
        }
    }

    fn comparable(&self) -> bool {
        !self.animated && !self.focus && !self.overflow
    }
}

/// What a motion slot's store rows hold (or would hold, painted now).
#[derive(Clone, Debug, Default)]
pub(crate) struct Snapshot {
    /// It describes the rows: the subtree's boxes were dense in pre-order, and nothing wrote the
    /// rows since it was taken.
    valid: bool,
    rect: Span,
    place: (u16, u16),
    inputs: Inputs,
    /// The subtree's boxes and runs, node ids numbered from 1 (`boxes[id - 1]`).
    boxes: Vec<LayoutBox>,
    texts: Vec<TextRun>,
    /// Scratch: a subtree node's new id (0: not painted by this pass), reused between takes.
    ids: Vec<usize>,
}

impl Snapshot {
    /// The subtree `[first, end)` of `boxes`/`texts` (pre-order, `boxes[id - 1]`; runs sorted by
    /// node) resting at `rect`, its pixels at `place`, with `inputs`. Not valid when the boxes
    /// are not dense there (the diff indexes runs' boxes by node id).
    #[cfg(test)]
    pub(crate) fn take(
        &mut self,
        lists: (&[LayoutBox], &[TextRun]),
        span: (usize, usize),
        at: (Span, (u16, u16)),
        inputs: Inputs,
    ) {
        self.take_painted(lists, span, at, inputs, &|_| true);
    }

    /// `take` of only the boxes `painted` names — what the layer's own pass paints into the
    /// store (a scroll band's content is drawn in its band, never in the store: comparing it
    /// refused every re-open of a layer holding a band, board 2026-11-21T07-37-47), renumbered
    /// densely from 1 in pre-order; runs of a box left out go with it.
    pub(crate) fn take_painted(
        &mut self,
        (boxes, texts): (&[LayoutBox], &[TextRun]),
        (first, end): (usize, usize),
        (rect, place): (Span, (u16, u16)),
        inputs: Inputs,
        painted: &dyn Fn(&LayoutBox) -> bool,
    ) {
        let base = first.saturating_sub(1);
        let span = (first >= 1 && end > first).then(|| boxes.get(base..end - 1)).flatten();
        let span = span.filter(|s| s.iter().enumerate().all(|(i, b)| b.node_id == first + i));
        let src = span.unwrap_or(&[]);
        self.ids.clear();
        let mut next = 0;
        self.ids.extend(src.iter().map(|b| {
            next += usize::from(painted(b));
            if painted(b) {
                next
            } else {
                0
            }
        }));
        let new_id = |ids: &[usize], id: usize| -> Option<usize> {
            id.checked_sub(first).and_then(|i| ids.get(i)).copied().filter(|&n| n != 0)
        };
        self.boxes.clear();
        for b in src.iter().filter(|b| painted(b)) {
            let mut c = b.clone();
            c.node_id = new_id(&self.ids, b.node_id).unwrap_or(0);
            c.scroll_root = b.scroll_root.map(|r| new_id(&self.ids, r).unwrap_or(r));
            self.boxes.push(c);
        }
        let (t0, t1) =
            (texts.partition_point(|t| t.node < first), texts.partition_point(|t| t.node < end));
        self.texts.clear();
        for t in texts.get(t0..t1).unwrap_or(&[]) {
            if let Some(n) = new_id(&self.ids, t.node) {
                let mut c = t.clone();
                c.node = n;
                self.texts.push(c);
            }
        }
        (self.valid, self.rect, self.place, self.inputs) = (span.is_some(), rect, place, inputs);
    }

    /// Its rows were written: it no longer describes them.
    pub(crate) fn forget(&mut self) {
        self.valid = false;
    }

    pub(crate) fn is_valid(&self) -> bool {
        self.valid
    }

    fn comparable(&self) -> bool {
        self.valid && self.inputs.comparable()
    }
}

/// What a layer's store needs before it is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reuse {
    /// It holds the subtree: nothing to paint.
    Keep,
    /// These layout rows changed: they are painted again.
    Rows((i32, i32)),
    /// Not comparable: painted whole.
    Whole,
}

impl Reuse {
    /// Its name and the rows it paints, of a layer `h` rows high — for the layer's line.
    pub(crate) fn said(self, h: u32) -> (&'static str, u32) {
        match self {
            Reuse::Keep => ("keep", 0),
            Reuse::Rows((y0, y1)) => ("rows", (y1 - y0).max(0) as u32),
            Reuse::Whole => ("whole", h),
        }
    }
}

/// What the store that `snapshot` describes needs to hold what `now` describes: the same rect,
/// place and inputs, both comparable, and the subtrees' diff (`layout_diff::changed_rows`, every
/// node a layer's) naming no row → keep; naming rows → those rows; anything else → whole.
pub(crate) fn reuse(snapshot: &Snapshot, now: &Snapshot) -> Reuse {
    let same =
        (snapshot.rect, snapshot.place, snapshot.inputs) == (now.rect, now.place, now.inputs);
    if !same || !snapshot.comparable() || !now.comparable() {
        return Reuse::Whole;
    }
    let all = |_: usize| true;
    let size = (now.rect.2.max(1), now.rect.3.max(1));
    let texts = (snapshot.texts.as_slice(), now.texts.as_slice());
    let changed = changed_rows(&snapshot.boxes, &now.boxes, texts, size, (&all, &all));
    // No base box takes part: a base change is the diff's doubt (no bound).
    if !changed.base.is_empty() {
        return Reuse::Whole;
    }
    match changed.layers.map(|(y0, y1)| (y0.max(now.rect.1), y1.min(now.rect.3))) {
        Some((y0, y1)) if y1 > y0 => Reuse::Rows((y0, y1)),
        _ => Reuse::Keep,
    }
}

/// A dropped ghost's store, kept for a re-enter of the same identity and size.
#[derive(Clone, Debug)]
pub(crate) struct Parked {
    identity: i32,
    size: (u32, u32),
    /// Its item's key in the store's places.
    key: u32,
    /// When it was parked (the oldest goes first).
    seq: u32,
    snapshot: Snapshot,
}

/// The parked entry a transition of `identity` entering at `size` takes.
pub(crate) fn parked_for(
    parked: &[Option<Parked>],
    identity: i32,
    size: (u32, u32),
) -> Option<usize> {
    parked.iter().position(|p| p.as_ref().is_some_and(|p| (p.identity, p.size) == (identity, size)))
}

/// What gives way when a plan of the store does not hold (`Outcome`): the parked stores first,
/// then the ghosts, then the motion layers, then a band.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GiveWay {
    /// Everything stays where it was placed.
    Placed,
    /// Every parked store goes (a compaction moved their places, or there is no room).
    Parked,
    /// Every ghost goes, for this reason.
    Ghosts(&'static str),
    /// The motion layers go (the app paints them in the base).
    Motions,
    /// A band goes flat.
    Band,
}

/// The order of giving way after a plan's `outcome`, with parked stores, ghosts and live motion
/// layers in it (`present`).
pub(crate) fn give_way(outcome: Outcome, (parked, ghosts, motions): (bool, bool, bool)) -> GiveWay {
    match outcome {
        // A compaction moved every place: what windowd still draws, or what a re-enter would
        // reuse, is no longer there.
        Outcome::Compacted if parked => GiveWay::Parked,
        Outcome::Compacted if ghosts => GiveWay::Ghosts("store"),
        Outcome::Kept | Outcome::Compacted => GiveWay::Placed,
        Outcome::Full if parked => GiveWay::Parked,
        Outcome::Full if ghosts => GiveWay::Ghosts("room"),
        Outcome::Full if motions => GiveWay::Motions,
        Outcome::Full => GiveWay::Band,
    }
}

/// The surface's cache: each motion slot's snapshot, the parked stores, a scratch for now.
#[derive(Debug, Default)]
pub(crate) struct Cache {
    held: [Snapshot; MAX_MOTION_LAYERS],
    parked: [Option<Parked>; MAX_PARKED],
    now: Snapshot,
    seq: u32,
}

impl Cache {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Slot `k`'s store rows are written, lost or taken by a new layer: unknown.
    pub(crate) fn forget(&mut self, k: usize) {
        if let Some(s) = self.held.get_mut(k) {
            s.forget();
        }
    }

    /// Whether slot `k` has no snapshot of its rows.
    pub(crate) fn wants(&self, k: usize) -> bool {
        self.held.get(k).is_none_or(|s| !s.is_valid())
    }

    /// The scratch the current state is taken into.
    pub(crate) fn now_mut(&mut self) -> &mut Snapshot {
        &mut self.now
    }

    /// Slot `k`'s rows hold what the scratch describes (they were just painted from it).
    pub(crate) fn hold_now(&mut self, k: usize) {
        if let Some(s) = self.held.get_mut(k) {
            core::mem::swap(s, &mut self.now);
        }
    }

    /// Why slot `k` against the scratch would be painted whole — asked before `decide`, for the
    /// layer's line (`apphost: reuse whole (… why=)`): no snapshot yet, one the rows outgrew, a
    /// moved rest or place, a node animated off identity, the focus inside, too many views,
    /// another theme or backdrop or view, else a box the diff could not place.
    #[cfg(any(test, nexus_env = "os"))]
    pub(crate) fn why_whole(&self, k: usize) -> &'static str {
        let Some(s) = self.held.get(k).filter(|s| s.valid) else { return "first" };
        let now = &self.now;
        if !now.valid {
            "sparse"
        } else if (s.rect, s.place) != (now.rect, now.place) {
            "moved"
        } else if now.inputs.animated {
            "animated"
        } else if now.inputs.focus {
            "focus"
        } else if now.inputs.overflow {
            "views"
        } else if s.inputs != now.inputs {
            "inputs"
        } else {
            "base"
        }
    }

    /// The decision for slot `k` against the scratch; a `Whole` forgets the slot.
    pub(crate) fn decide(&mut self, k: usize) -> Reuse {
        let Some(held) = self.held.get(k) else { return Reuse::Whole };
        let r = reuse(held, &self.now);
        if r == Reuse::Whole {
            self.forget(k);
        }
        r
    }

    /// Slot `k`'s ghost is dropped: its store at `place` parks under a new key — `rekey` moves
    /// the store item to it — with `identity` and `size`; past `MAX_PARKED` the oldest goes.
    /// Whether it parked (a slot without a valid snapshot of that place frees its place).
    pub(crate) fn park(
        &mut self,
        k: usize,
        (identity, size, place): (i32, (u32, u32), (u16, u16)),
        key_base: u32,
        rekey: impl FnOnce((u16, u16), u32) -> bool,
    ) -> bool {
        let Some(held) = self.held.get_mut(k) else { return false };
        if !held.is_valid() || held.place != place {
            held.forget();
            return false;
        }
        self.seq = self.seq.wrapping_add(1);
        let key = key_base | (self.seq & 0xFFFF);
        if !rekey(place, key) {
            held.forget();
            return false;
        }
        let at = match self.parked.iter().position(Option::is_none) {
            Some(free) => free,
            None => (0..MAX_PARKED)
                .min_by_key(|&j| self.parked[j].as_ref().map_or(0, |p| p.seq))
                .unwrap_or(0),
        };
        let snapshot = core::mem::take(held);
        self.parked[at] = Some(Parked { identity, size, key, seq: self.seq, snapshot });
        true
    }

    /// The parked stores (for `parked_for`).
    pub(crate) fn parked(&self) -> &[Option<Parked>] {
        &self.parked
    }

    /// Parked entry `j` is taken by slot `k`: its snapshot becomes the slot's. Its place (the
    /// caller rekeys the item there to the layer).
    pub(crate) fn unpark(&mut self, j: usize, k: usize) -> Option<(u16, u16)> {
        let held = self.held.get_mut(k)?;
        let p = self.parked.get_mut(j)?.take()?;
        let place = p.snapshot.place;
        *held = p.snapshot;
        Some(place)
    }

    /// Each parked store's item: its key and size.
    pub(crate) fn parked_items(&self, mut each: impl FnMut(u32, u32, u32)) {
        for p in self.parked.iter().flatten() {
            each(p.key, p.size.0, p.size.1);
        }
    }

    /// Every parked store goes (their places are freed by the next plan). How many went.
    pub(crate) fn drop_parked(&mut self) -> usize {
        let n = self.parked.iter().flatten().count();
        self.parked = Default::default();
        n
    }

    /// The parked stores painted under another theme go.
    pub(crate) fn drop_parked_off_theme(&mut self, theme: u8) {
        for p in self.parked.iter_mut() {
            if p.as_ref().is_some_and(|p| p.snapshot.inputs.theme != theme) {
                *p = None;
            }
        }
    }
}

#[cfg(test)]
#[path = "motion_reuse_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "motion_reuse_oracle.rs"]
mod oracle;
