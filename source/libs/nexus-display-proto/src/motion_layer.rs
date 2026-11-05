// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0109 §motion layers (TASK-0354 P2) — a layer of a client surface that the
//! COMPOSITOR moves. The app paints a subtree once into its store (the base is painted without
//! it) and declares it with the structure (`OP_SURFACE_TREE`): where it rests in the base, where
//! its pixels are in the store, the motion it starts with (a curated token's curve, a duration,
//! the offset, opacity and scale it starts from — the scale about the layer's centre) and which
//! of the surface's regions and scroll layers are part of it. windowd evaluates the motion for the time each frame is shown and draws the
//! layer, its regions, its scroll layers' slices and its washes moved and faded — the app is out
//! of the frame loop. A motion starts with the present that first carries its id; the same id
//! declared again continues it. A layer may be drawn IN FRONT (`MOTION_FRONT`: on the desktop
//! above the windows and the shell's chrome — a modal, a toast) and may carry a BACKDROP (one
//! patch spread over the whole surface below it, fading with it: a modal's scrim is a texel of
//! colour, not a screen of pixels in the store). Layers may overlap at rest: they are drawn in
//! their order, which is the app's paint order. The codec and the bounds every receiver checks
//! live here; windowd resolves the token (the physics SSOT is the animation crate, ADR-0031).
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable (append-only)
//! TEST_COVERAGE: `surface_layers_tests.rs` (round trip, `test_reject_*` for every bound)

use crate::surface_layers::{SurfaceTree, TreeError};

/// Motion layers a surface may declare.
pub const MAX_MOTION_LAYERS: usize = 4;
/// The longest motion (ms): a transition is a few hundred ms (latency-reference.md); past this
/// bound a structure is refused.
pub const MAX_MOTION_MS: u16 = 1000;
/// A wash whose parent is motion layer `m` names `MOTION_PARENT | m` (its rectangle relative to
/// the layer's origin at rest): it moves with the layer.
pub const MOTION_PARENT: u8 = 0x80;
/// The flag of a layer drawn in front of everything else of its surface's place in the frame: on
/// the desktop above the windows, the shell's bar and its panels; in a window last in its body.
/// The flags' other bits are reserved (zero).
pub const MOTION_FRONT: u8 = 0x01;
/// A scale of one, in the 1/10000 steps a motion's scale is stated in.
pub const SCALE_ONE: u32 = 10_000;
/// The farthest a motion may start from its size: half of it, or twice it.
pub const SCALE_MIN: u16 = 5_000;
pub const SCALE_MAX: u16 = 20_000;

/// One motion layer (see the module header).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MotionLayerDesc {
    /// The layer's rectangle at rest, in base coordinates (its subtree's paint bounds).
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
    /// Where its pixels are in the store: a `w × h` rectangle from (`sx`, `sy`).
    pub sx: u16,
    pub sy: u16,
    /// The motion's identity: a new id starts the motion with the present that carries it.
    pub id: u16,
    /// The curated motion token whose curve the motion follows.
    pub token: u8,
    /// The motion's duration (ms; `0`: the layer rests, nothing moves).
    pub duration_ms: u16,
    /// Where the layer starts relative to its rest: an offset (px) and an opacity (`255` = drawn).
    pub from_dx: i16,
    pub from_dy: i16,
    pub from_opacity: u8,
    /// The regions that are part of the layer (bit `k`: region `k`): they move with it, and their
    /// pixels are the layer's.
    pub regions: u16,
    /// The scroll layers that are part of it (bit `k`: scroll layer `k`): their slices move with it.
    pub scrolls: u8,
    /// Drawn in front (`MOTION_FRONT`).
    pub front: bool,
    /// Its backdrop: patch `backdrop - 1` spread over the whole surface below the layer's pixels
    /// at `backdrop_opacity`, fading in over `backdrop_ms` on its own smooth curve (a scrim
    /// arrives in the theme's quick step while the sheet springs in its slow one), never moving
    /// with the layer; `0`: none.
    pub backdrop: u8,
    pub backdrop_opacity: u8,
    pub backdrop_ms: u16,
    /// The scale it starts from, in 1/10000 of its size, about its centre (`0` = unscaled: an
    /// entrance that only travels and fades).
    pub from_scale: u16,
}

impl MotionLayerDesc {
    /// The empty layer (no area, at rest).
    pub const EMPTY: Self = Self {
        x: 0,
        y: 0,
        w: 0,
        h: 0,
        sx: 0,
        sy: 0,
        id: 0,
        token: 0,
        duration_ms: 0,
        from_dx: 0,
        from_dy: 0,
        from_opacity: 0,
        regions: 0,
        scrolls: 0,
        front: false,
        backdrop: 0,
        backdrop_opacity: 0,
        backdrop_ms: 0,
        from_scale: 0,
    };

    /// The scale it starts from, in 1/10000 (`10000`: unscaled).
    #[must_use]
    pub fn start_scale(&self) -> u32 {
        if self.from_scale == 0 {
            SCALE_ONE
        } else {
            u32::from(self.from_scale)
        }
    }

    /// Its backdrop's patch and opacity, when it has one.
    #[must_use]
    pub fn backdrop(&self) -> Option<(usize, u8)> {
        self.backdrop.checked_sub(1).map(|p| (usize::from(p), self.backdrop_opacity))
    }

    /// Its rectangle at rest `(x, y, w, h)`.
    #[must_use]
    pub fn rest(&self) -> (u32, u32, u32, u32) {
        (u32::from(self.x), u32::from(self.y), u32::from(self.w), u32::from(self.h))
    }

    /// Where a base point at rest `(x, y)` inside the layer has its pixel in the store.
    #[must_use]
    pub fn store_of(&self, x: u32, y: u32) -> (u32, u32) {
        (
            u32::from(self.sx) + x.saturating_sub(u32::from(self.x)),
            u32::from(self.sy) + y.saturating_sub(u32::from(self.y)),
        )
    }

    /// Whether region `k` is part of the layer.
    #[must_use]
    pub fn claims_region(&self, k: usize) -> bool {
        k < 16 && self.regions & (1 << k) != 0
    }

    /// Whether scroll layer `k` is part of the layer.
    #[must_use]
    pub fn claims_scroll(&self, k: usize) -> bool {
        k < 8 && self.scrolls & (1 << k) != 0
    }
}

/// One motion layer on the wire.
pub(crate) const MOTION_BYTES: usize = 32;

/// Writes `m` at `at` of `out` (`MOTION_BYTES`; the flags' reserved bits zero).
pub(crate) fn put(out: &mut [u8], at: usize, m: &MotionLayerDesc) {
    let w16 = |out: &mut [u8], o: usize, v: u16| {
        out[at + o..at + o + 2].copy_from_slice(&v.to_le_bytes());
    };
    w16(out, 0, m.x);
    w16(out, 2, m.y);
    w16(out, 4, m.w);
    w16(out, 6, m.h);
    w16(out, 8, m.sx);
    w16(out, 10, m.sy);
    w16(out, 12, m.id);
    w16(out, 14, m.duration_ms);
    w16(out, 16, m.from_dx as u16);
    w16(out, 18, m.from_dy as u16);
    w16(out, 20, m.regions);
    out[at + 22] = m.token;
    out[at + 23] = m.from_opacity;
    out[at + 24] = m.scrolls;
    out[at + 25] = if m.front { MOTION_FRONT } else { 0 };
    out[at + 26] = m.backdrop;
    out[at + 27] = m.backdrop_opacity;
    w16(out, 28, m.backdrop_ms);
    w16(out, 30, m.from_scale);
}

/// The motion layer at `at` of `f` (`MOTION_BYTES`); `None` when a reserved flag bit is set.
pub(crate) fn get(f: &[u8], at: usize) -> Option<MotionLayerDesc> {
    let r16 = |o: usize| u16::from_le_bytes([f[at + o], f[at + o + 1]]);
    let flags = f[at + 25];
    if flags & !MOTION_FRONT != 0 {
        return None;
    }
    Some(MotionLayerDesc {
        x: r16(0),
        y: r16(2),
        w: r16(4),
        h: r16(6),
        sx: r16(8),
        sy: r16(10),
        id: r16(12),
        duration_ms: r16(14),
        from_dx: r16(16) as i16,
        from_dy: r16(18) as i16,
        regions: r16(20),
        token: f[at + 22],
        from_opacity: f[at + 23],
        scrolls: f[at + 24],
        front: flags & MOTION_FRONT != 0,
        backdrop: f[at + 26],
        backdrop_opacity: f[at + 27],
        backdrop_ms: r16(28),
        from_scale: r16(30),
    })
}

type Rect = (u32, u32, u32, u32);

fn inside(inner: Rect, outer: Rect) -> bool {
    inner.2 > 0
        && inner.3 > 0
        && inner.0 >= outer.0
        && inner.1 >= outer.1
        && inner.0 + inner.2 <= outer.0 + outer.2
        && inner.1 + inner.3 <= outer.1 + outer.3
}

/// The motion layers' share of `validate_tree`: each rests inside the base, keeps its pixels in
/// the store (below the base, within the declared columns), moves within the surface for at most
/// `MAX_MOTION_MS`, claims only regions and scroll layers that lie inside it and that no other
/// layer claims, and names only a declared patch as its backdrop. Layers may overlap at rest
/// (drawn in their order). Each one's store rectangle goes into `store` (from `n`) for the overlap
/// check with the bands and patches. Returns the new count.
pub(crate) fn check(
    tree: &SurfaceTree,
    (w, h, rows, store_w): (u32, u32, u32, u32),
    store: &mut [Rect],
    mut n: usize,
) -> Result<usize, TreeError> {
    let (mut regions, mut scrolls) = (0u16, 0u8);
    for m in tree.motions() {
        let rest = m.rest();
        if !inside(rest, (0, 0, w, h)) {
            return Err(TreeError::MotionOutside);
        }
        let stored = (u32::from(m.sx), u32::from(m.sy), rest.2, rest.3);
        if !inside(stored, (0, 0, store_w, rows)) || stored.1 < h {
            return Err(TreeError::MotionStoreOutside);
        }
        if m.duration_ms > MAX_MOTION_MS || m.backdrop_ms > MAX_MOTION_MS {
            return Err(TreeError::MotionTooLong);
        }
        let scale_ok = m.from_scale == 0 || (SCALE_MIN..=SCALE_MAX).contains(&m.from_scale);
        if u32::from(m.from_dx.unsigned_abs()) > w
            || u32::from(m.from_dy.unsigned_abs()) > h
            || !scale_ok
        {
            return Err(TreeError::MotionTooFar);
        }
        for (k, r) in tree.regions().iter().enumerate() {
            let rect = (u32::from(r.x), u32::from(r.y), u32::from(r.w), u32::from(r.h));
            if m.claims_region(k) && !inside(rect, rest) {
                return Err(TreeError::ClaimOutside);
            }
        }
        for (k, s) in tree.scrolls().iter().enumerate() {
            let viewport = (u32::from(s.vx), u32::from(s.vy), u32::from(s.vw), u32::from(s.vh));
            if m.claims_scroll(k) && !inside(viewport, rest) {
                return Err(TreeError::ClaimOutside);
            }
        }
        let known_regions =
            if tree.regions().len() >= 16 { u16::MAX } else { (1u16 << tree.regions().len()) - 1 };
        let known_scrolls = (1u8 << tree.scrolls().len().min(7)) - 1;
        if m.regions & !known_regions != 0 || m.scrolls & !known_scrolls != 0 {
            return Err(TreeError::ClaimOutside);
        }
        if m.regions & regions != 0 || m.scrolls & scrolls != 0 {
            return Err(TreeError::ClaimTwice);
        }
        regions |= m.regions;
        scrolls |= m.scrolls;
        if m.backdrop().is_some_and(|(p, _)| p >= tree.patches().len()) {
            return Err(TreeError::BackdropMissing);
        }
        let Some(slot) = store.get_mut(n) else { return Err(TreeError::Overlap) };
        *slot = stored;
        n += 1;
    }
    Ok(n)
}
