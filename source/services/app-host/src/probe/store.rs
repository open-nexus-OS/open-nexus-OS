// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: app-host's STORE plan (RFC-0109): what the surface's rows below its base hold — the
//! wash patch, each scroll layer's virtual band, each motion layer's pixels (TASK-0354 P2) —
//! placed on shelves, and what a create asks windowd for. Split out of `band.rs` (module-size
//! ratchet): the bands' filling and drawing stay there, the motion layers' raster is
//! `motion_layers.rs`. Arithmetic only: a plan that outgrows the surface's rows waits for the
//! loop's re-create.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the bounds in `nexus-display-proto::surface_layers`; the QEMU lanes.

use super::band::{Band, PATCH_ROWS};
use super::motion_layers::MAX_MOTION_LAYERS;
use super::*;
use nexus_display_proto::surface_layers as lw;

/// The least margin a band keeps beyond its viewport (content positions).
const MARGIN_MIN: u32 = 64;
/// The patch row's columns: the hover wash's patch and one per motion layer's backdrop.
const PATCH_COLS: u32 = PATCH_ROWS * lw::MAX_PATCHES as u32;

/// What a surface's store takes (RFC-0109): the surface's rows (base + store) and the store's
/// used width — the region windowd places for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct StorePlan {
    pub(super) rows: u32,
    pub(super) width: u16,
}

/// Where each motion layer's pixels go in the store (`None`: not placed).
type Places = [Option<(u16, u16)>; MAX_MOTION_LAYERS];

impl super::DslApp {
    /// Places the wash patch, every scroll layer's band and every motion layer's pixels in the
    /// store — shelves below the patch: each band a ring of twice its viewport (half a viewport
    /// of margin each way, `MARGIN_MIN` at least), narrower when the store would pass the
    /// surface's rows, then the motion layers at their size. When even the narrowest set does not
    /// fit, the motion layers go first (the app paints them in the base again), then the layer
    /// with the longest viewport goes flat (painted in the base unscrolled, clipped to its
    /// viewport — as past the layer bound) and the rest are placed again. A band or a motion
    /// layer whose place changed holds nothing (it fills, or paints, again). Without a store
    /// (windowd placed none: `NO_STORE`) every layer goes flat. Returns the plan.
    pub(super) fn plan_store(&mut self) -> StorePlan {
        let (w, h) = (self.w, self.h);
        if self.store_refused {
            for k in 0..super::scroll::MAX_SCROLL_LAYERS {
                self.flatten(k);
            }
            self.place_motions([None; MAX_MOTION_LAYERS]);
            self.tree_dirty = true;
            return StorePlan { rows: h, width: 0 };
        }
        let max_rows = u32::from(lw::MAX_SURFACE_ROWS);
        let mut sizes = [None; MAX_MOTION_LAYERS];
        for (s, m) in sizes.iter_mut().zip(self.motion_layers.iter()) {
            *s = m.map(|m| m.size());
        }
        let fit = |layers: &[Option<super::scroll::ScrollLayer>],
                   sizes: &[Option<(u32, u32)>; MAX_MOTION_LAYERS],
                   margin_pct: u32,
                   out: &mut [Band; super::scroll::MAX_SCROLL_LAYERS],
                   places: &mut Places| {
            *places = [None; MAX_MOTION_LAYERS];
            let (mut x, mut shelf_y, mut shelf_h) = (0u32, h + PATCH_ROWS, 0u32);
            let mut used_w = PATCH_COLS; // the patches' columns
            for (k, l) in layers.iter().enumerate() {
                let Some(l) = l else { continue };
                let (vx0, vy0, vx1, vy1) = l.viewport;
                let (vw, vh) = ((vx1 - vx0).max(1) as u32, (vy1 - vy0).max(1) as u32);
                let len = l.viewport_len().max(1) as u32;
                let margin = (len * margin_pct / 100).max(MARGIN_MIN);
                let cap = len + 2 * margin;
                // Vertical: one strip of `cap` rows, the viewport wide. Horizontal: strips of
                // `strip_len` columns (no wider than the surface) under each other.
                let (strip_len, strips, bw, bh) = if l.vertical() {
                    (cap, 1u32, vw, cap)
                } else {
                    let strip_len = w.min(cap).max(len.div_ceil(2));
                    let strips = cap.div_ceil(strip_len);
                    (strip_len, strips, strip_len, strips * vh)
                };
                if bw > w || strip_len > u32::from(u16::MAX) || strips > u32::from(u8::MAX) {
                    return None;
                }
                if x + bw > w {
                    shelf_y += shelf_h;
                    x = 0;
                    shelf_h = 0;
                }
                out[k] = Band {
                    bx: x as u16,
                    by: shelf_y.min(u32::from(u16::MAX)) as u16,
                    strip_len: strip_len as u16,
                    strips: strips as u8,
                    valid_start: 0,
                    valid_len: 0,
                };
                x += bw;
                used_w = used_w.max(x);
                shelf_h = shelf_h.max(bh);
            }
            // The motion layers' pixels at their size, on the same shelves (TASK-0354 P2).
            for (k, s) in sizes.iter().enumerate() {
                let Some((mw, mh)) = *s else { continue };
                if mw > w {
                    return None;
                }
                if x + mw > w {
                    shelf_y += shelf_h;
                    x = 0;
                    shelf_h = 0;
                }
                places[k] = Some((x as u16, shelf_y.min(u32::from(u16::MAX)) as u16));
                x += mw;
                used_w = used_w.max(x);
                shelf_h = shelf_h.max(mh);
            }
            let rows = shelf_y + shelf_h;
            (rows <= max_rows).then_some((rows, used_w))
        };
        let mut bands = [Band::default(); super::scroll::MAX_SCROLL_LAYERS];
        let mut places: Places = [None; MAX_MOTION_LAYERS];
        let mut layers = self.scroll_layers;
        let (rows, used_w) = loop {
            let fits = fit(&layers, &sizes, 50, &mut bands, &mut places)
                .or_else(|| fit(&layers, &sizes, 20, &mut bands, &mut places))
                .or_else(|| fit(&layers, &sizes, 0, &mut bands, &mut places));
            if let Some(r) = fits {
                break r;
            }
            // The motion layers give way first: the app paints them in the base again.
            if sizes.iter().any(Option::is_some) {
                sizes = [None; MAX_MOTION_LAYERS];
                continue;
            }
            let longest = (0..layers.len())
                .filter(|&k| layers[k].is_some())
                .max_by_key(|&k| layers[k].map_or(0, |l| l.viewport_len()));
            match longest {
                Some(k) => layers[k] = None,
                None => break (h + PATCH_ROWS, PATCH_COLS),
            }
        };
        let mut flattened = 0;
        for (k, kept) in layers.iter().enumerate() {
            if kept.is_none() && self.flatten(k) {
                flattened += 1;
            }
        }
        // Named, never a slow path in disguise: these containers do not scroll.
        if flattened > 0 && crate::proof_line::STORE_FULL.claim() {
            raw_marker(&alloc::format!("apphost: store full (flattened={flattened})"));
        }
        for (slot, band) in self.scroll_layers.iter_mut().zip(bands) {
            if let Some(l) = slot.as_mut() {
                let same = (l.band.bx, l.band.by, l.band.strip_len, l.band.strips)
                    == (band.bx, band.by, band.strip_len, band.strips);
                if !same {
                    l.band = band;
                }
            }
        }
        self.place_motions(places);
        self.patch = (0, h as u16);
        self.tree_dirty = true;
        let n = self.scroll_layers.iter().flatten().count();
        if crate::proof_line::SCROLL_LAYERS.raise(n) {
            raw_marker(&alloc::format!(
                "apphost: scroll layers (n={n} band_rows={})",
                rows - h - PATCH_ROWS
            ));
        }
        StorePlan { rows, width: used_w.min(w) as u16 }
    }

    /// Layer `k` goes flat: no band — its content paints in the base unscrolled, clipped to its
    /// viewport (as a container past the layer bound). Whether there was a layer.
    pub(super) fn flatten(&mut self, k: usize) -> bool {
        let Some(l) = self.scroll_layers[k].take() else { return false };
        let view = nexus_scene_raster::ScrollView { root: l.root, clip: l.viewport, dx: 0, dy: 0 };
        self.flat_views.push(view);
        true
    }

    /// The surface was made for `plan` (RFC-0114: one slot of its buffer holds the base and the
    /// store) — it fits what was planned: nothing waits for a re-create (every surface was made
    /// twice at start, the second time for nothing).
    pub(super) fn created(&mut self, plan: StorePlan) {
        self.store_rows = plan.rows;
        self.store_w = plan.width;
        self.store_replan = false;
    }

    /// No memory for the store's buffer (the create answered `QUOTA`): every scroll layer paints
    /// flat until the next create asks again. Returns the flat plan to create with.
    pub(super) fn refuse_store(&mut self) -> StorePlan {
        self.store_refused = true;
        let plan = self.plan_store();
        if crate::proof_line::STORE_REFUSED.claim() {
            raw_marker("apphost: store refused (no memory for the bands: scroll layers flat)");
        }
        plan
    }

    /// The store a re-create asks for: one refused before is asked for again — its layers back
    /// from flat.
    pub(super) fn plan_for_create(&mut self) -> StorePlan {
        if core::mem::take(&mut self.store_refused) {
            let _ = self.sync_layers();
        }
        self.plan_store()
    }

    /// Whether `plan` needs more than the surface was created with (a re-create).
    pub(super) fn outgrows(&self, plan: StorePlan) -> bool {
        plan.rows > self.store_rows || plan.width > self.store_w
    }
}
