// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: app-host's STORE (RFC-0109): the surface's rows below its base — the wash patch and
//! each scroll layer's virtual band, a ring over content space. Planned from the page's layers
//! (`plan_store`), filled visible window first and then the margins ahead and behind, within a
//! time budget per pass so the physics tick never waits on a raster (`fill_bands`), and declared
//! to windowd: the STRUCTURE — regions, layers, the patch — with the present of the pixels it
//! names, the MOTION — offsets, the wash — at once. Content outside a band's valid range is never
//! shown: windowd composes only what the ring holds.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the ring and the bounds in `nexus-display-proto::surface_layers`; the QEMU lanes.

use super::scroll::ScrollLayer;
use super::*;
use nexus_display_proto::surface_layers as lw;

/// The wash patch's rows: one 16×16 patch at the store's first row.
pub(super) const PATCH_ROWS: u32 = lw::PATCH_SIDE as u32;
/// A fill pass's budget: the band never holds the physics tick longer than this.
const FILL_BUDGET_NS: u64 = 4_000_000;
/// Rows painted between two looks at the clock.
const FILL_BATCH: u32 = 8;
/// The least margin a band keeps beyond its viewport (content positions).
const MARGIN_MIN: u32 = 64;

use crate::band_math::{grow, next_fill, run_of};
pub(super) use crate::band_math::{Band, PresentDamage};

/// What a surface's store takes (RFC-0109): the surface's rows (base + store) and the store's
/// used width — the region windowd places for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct StorePlan {
    pub(super) rows: u32,
    pub(super) width: u16,
}

/// The wire description of `layer`'s band (a covered layer's shows nothing: its content paints
/// in the base).
pub(super) fn desc(layer: &ScrollLayer) -> lw::ScrollLayerDesc {
    let (x0, y0, x1, y1) = layer.viewport;
    let clamp = |v: i32| v.clamp(0, i32::from(u16::MAX)) as u16;
    let b = &layer.band;
    lw::ScrollLayerDesc {
        vx: clamp(x0),
        vy: clamp(y0),
        vw: clamp(x1 - x0),
        vh: clamp(y1 - y0),
        axis: if layer.vertical() { lw::AXIS_VERTICAL } else { lw::AXIS_HORIZONTAL },
        flags: 0,
        bx: b.bx,
        by: b.by,
        strip_len: b.strip_len,
        strips: b.strips,
        valid_start: b.valid_start,
        valid_len: if layer.covered { 0 } else { b.valid_len.min(u32::from(u16::MAX)) as u16 },
    }
}

/// What layer `l`'s band draws next (`band_math::next_fill` over its own numbers).
fn layer_fill(l: &ScrollLayer) -> Option<crate::band_math::Fill> {
    next_fill(&l.band, l.viewport_len().max(0) as u32, l.extent, l.offset, l.covered)
}

impl super::DslApp {
    /// Places the wash patch and every scroll layer's band in the store — shelves of bands below
    /// the patch, each band a ring of twice its viewport (half a viewport of margin each way,
    /// `MARGIN_MIN` at least), narrower when the store would pass the surface's rows. When even
    /// the narrowest set does not fit, the layer with the longest viewport goes flat (painted in
    /// the base unscrolled, clipped to its viewport — as past the layer bound) and the rest are
    /// placed again. A band whose place or size changed holds nothing (it fills again). Without
    /// a store (windowd placed none: `NO_STORE`) every layer goes flat. Returns the plan.
    pub(super) fn plan_store(&mut self) -> StorePlan {
        let (w, h) = (self.w, self.h);
        if self.store_refused {
            for k in 0..super::scroll::MAX_SCROLL_LAYERS {
                self.flatten(k);
            }
            self.tree_dirty = true;
            return StorePlan { rows: h, width: 0 };
        }
        let max_rows = u32::from(lw::MAX_SURFACE_ROWS);
        let fit = |layers: &[Option<ScrollLayer>],
                   margin_pct: u32,
                   out: &mut [Band; super::scroll::MAX_SCROLL_LAYERS]| {
            let (mut x, mut shelf_y, mut shelf_h) = (0u32, h + PATCH_ROWS, 0u32);
            let mut used_w = PATCH_ROWS; // the patch's columns
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
                let band = Band {
                    bx: x as u16,
                    by: shelf_y.min(u32::from(u16::MAX)) as u16,
                    strip_len: strip_len as u16,
                    strips: strips as u8,
                    valid_start: 0,
                    valid_len: 0,
                };
                out[k] = band;
                x += bw;
                used_w = used_w.max(x);
                shelf_h = shelf_h.max(bh);
            }
            let rows = shelf_y + shelf_h;
            (rows <= max_rows).then_some((rows, used_w))
        };
        let mut bands = [Band::default(); super::scroll::MAX_SCROLL_LAYERS];
        let mut layers = self.scroll_layers;
        let (rows, used_w) = loop {
            let fits = fit(&layers, 50, &mut bands)
                .or_else(|| fit(&layers, 20, &mut bands))
                .or_else(|| fit(&layers, 0, &mut bands));
            if let Some(r) = fits {
                break r;
            }
            let longest = (0..layers.len())
                .filter(|&k| layers[k].is_some())
                .max_by_key(|&k| layers[k].map_or(0, |l| l.viewport_len()));
            match longest {
                Some(k) => layers[k] = None,
                None => break (h + PATCH_ROWS, PATCH_ROWS),
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
    fn flatten(&mut self, k: usize) -> bool {
        let Some(l) = self.scroll_layers[k].take() else { return false };
        let view = nexus_scene_raster::ScrollView { root: l.root, clip: l.viewport, dx: 0, dy: 0 };
        self.flat_views.push(view);
        true
    }

    /// The create's answer for `plan` (RFC-0109): the store placed — or none (`NO_STORE`: no
    /// atlas room), and every scroll layer paints flat until the next create. Whether the page
    /// must be drawn again (its layers went flat).
    pub(super) fn created(&mut self, store: bool, plan: StorePlan) -> bool {
        self.store_rows = plan.rows;
        self.store_w = plan.width;
        self.store_refused = !store;
        if store {
            return false;
        }
        let _ = self.plan_store();
        if crate::proof_line::STORE_REFUSED.claim() {
            raw_marker("apphost: store refused (no atlas room: scroll layers flat)");
        }
        true
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

    /// The content changed: every band holds nothing until it fills again, the visible window
    /// first (a covered layer's content paints with the base).
    pub(super) fn invalidate_bands(&mut self) {
        for l in self.scroll_layers.iter_mut().flatten() {
            l.band.valid_start = l.offset.max(0) as u32;
            l.band.valid_len = 0;
        }
        self.tree_dirty = true;
    }

    /// Whether a band still has content to draw (the loop keeps the pulse armed, a pass at a
    /// time).
    pub(super) fn bands_pending(&self) -> bool {
        self.scroll_layers.iter().flatten().any(|l| layer_fill(l).is_some())
    }

    /// One fill pass: each uncovered layer's band grows toward its target — the visible window,
    /// then the margins — a run of the ring at a time, until the budget is spent. `None`: no
    /// budget, the visible windows complete and nothing more (the first frame after a content
    /// change). The rows drawn are added to `damage`. Returns whether a band changed what it
    /// holds.
    pub(super) fn fill_bands(
        &mut self,
        vmo: u32,
        budget: Option<u64>,
        damage: &mut PresentDamage,
    ) -> bool {
        let deadline = budget.map(|b| nsec_now().saturating_add(b));
        let mut changed = false;
        for k in 0..super::scroll::MAX_SCROLL_LAYERS {
            while let Some(layer) = self.scroll_layers[k] {
                let Some(fill) = layer_fill(&layer) else { break };
                if (budget.is_none() && !fill.visible) || deadline.is_some_and(|d| nsec_now() >= d)
                {
                    break;
                }
                let chunk = if budget.is_some() { FILL_BATCH } else { FILL_BATCH * 8 };
                // One run of the ring: no strip end (and no wrap) inside it.
                let (q0, q1) = run_of(&fill, layer.band.strip_len, chunk);
                if q1 <= q0 {
                    break;
                }
                if fill.restart {
                    if let Some(l) = self.scroll_layers[k].as_mut() {
                        l.band.valid_start = q0;
                        l.band.valid_len = 0;
                    }
                }
                if !self.draw_band(k, q0, q1, vmo, damage) {
                    return changed;
                }
                if let Some(l) = self.scroll_layers[k].as_mut() {
                    grow(&mut l.band, q0, q1);
                }
                changed = true;
                self.tree_dirty = true;
            }
        }
        changed
    }

    /// Draws content positions `[p0, p1)` of layer `k` into its ring (one run: no strip end
    /// inside) and names the rows in `damage`. `false` when the surface refused a write.
    fn draw_band(
        &mut self,
        k: usize,
        p0: u32,
        p1: u32,
        vmo: u32,
        damage: &mut PresentDamage,
    ) -> bool {
        let Some(layer) = self.scroll_layers[k] else { return true };
        let desc = desc(&layer);
        let Some((sx, sy, _)) = desc.ring_place(p0) else { return true };
        let (vx0, vy0, vx1, vy1) = layer.viewport;
        let n = p1 - p0;
        let stride = self.w as usize * 4;
        let pass = super::paint::Pass::Band(layer.root);
        if layer.vertical() {
            self.prepare_picks(pass, (vy0 + p0 as i32, vy0 + p1 as i32), None);
            for i in 0..n {
                let dst = (sy + i) as usize * stride + sx as usize * 4;
                if !self.paint_band_row(vmo, vy0 + (p0 + i) as i32, 0, (vx0, vx1), dst) {
                    return false;
                }
            }
            damage.add((sx, sy, (vx1 - vx0) as u32, n));
        } else {
            // A run of columns: every viewport row, the canvas shifted so content column `p0`
            // lands at the row's start.
            let shift = vx0 + p0 as i32;
            self.prepare_picks(pass, (vy0, vy1), Some((shift, shift + n as i32)));
            for r in 0..(vy1 - vy0) as u32 {
                let dst = (sy + r) as usize * stride + sx as usize * 4;
                if !self.paint_band_row(vmo, vy0 + r as i32, shift, (0, n as i32), dst) {
                    return false;
                }
            }
            damage.add((sx, sy, n, (vy1 - vy0) as u32));
        }
        true
    }

    /// Redraws what a model change named: layout rows `span` (`None`: everything) — the base's
    /// rows there and, in each band, the content rows there it holds (a horizontal band whose
    /// viewport the span crosses: all it holds). Everything is the base, the patch, and every
    /// band from its visible window out. The drawn rows are added to `damage`; `false` when the
    /// surface refused a write.
    pub(super) fn redraw(
        &mut self,
        vmo: u32,
        span: Option<(i32, i32)>,
        damage: &mut PresentDamage,
    ) -> bool {
        let (w, h) = (self.w, self.h as i32);
        let Some((y0, y1)) = span else {
            if !self.render(vmo) || !self.draw_patch(vmo, damage) {
                return false;
            }
            damage.add((0, 0, w, h as u32));
            self.invalidate_bands();
            self.fill_bands(vmo, None, damage);
            return true;
        };
        let (b0, b1) = (y0.max(0), y1.min(h));
        if b1 > b0 {
            if !self.render_rows(vmo, b0, b1) {
                return false;
            }
            damage.add((0, b0 as u32, w, (b1 - b0) as u32));
        }
        for k in 0..super::scroll::MAX_SCROLL_LAYERS {
            let Some(l) = self.scroll_layers[k] else { continue };
            if l.covered || l.band.valid_len == 0 {
                continue;
            }
            let (_, vy0, _, vy1) = l.viewport;
            let (c0, c1) = if l.vertical() {
                ((y0 - vy0).max(0) as u32, (y1 - vy0).max(0) as u32)
            } else if y0 < vy1 + l.extent && y1 > vy0 {
                (0, u32::MAX)
            } else {
                continue;
            };
            // The rows it holds there, redrawn in place: a run of the ring at a time.
            let (mut q, end) = (c0.max(l.band.valid_start), c1.min(l.band.valid_end()));
            let strip = u32::from(l.band.strip_len).max(1);
            while q < end {
                let q1 = end.min(q + (strip - q % strip));
                if !self.draw_band(k, q, q1, vmo, damage) {
                    return false;
                }
                q = q1;
            }
        }
        true
    }

    /// One present: the model change redrawn (`change`: `Some(span)` — the span's layout rows,
    /// or everything), a fill pass of the bands within its budget, the structure when it changed,
    /// the present of every row drawn. `Ok(true)`: a present is in flight (its ack clears it).
    pub(super) fn present_frame(
        &mut self,
        client: &KernelClient,
        surface_id: u32,
        vmo: u32,
        seq: &mut u32,
        change: Option<Option<(i32, i32)>>,
    ) -> Result<bool, &'static str> {
        let mut damage = PresentDamage::default();
        if let Some(span) = change {
            if !self.redraw(vmo, span, &mut damage) {
                return Err("apphost: FAIL interactive render");
            }
        }
        self.fill_bands(vmo, Some(FILL_BUDGET_NS), &mut damage);
        if damage.is_empty() && !self.tree_dirty {
            // Nothing to show: an input that asked for a repaint alone was answered with nothing
            // (no hop); one that moved a layer is answered by the motion.
            if !self.motion_dirty {
                self.input_rx_ns = None;
            }
            return Ok(false);
        }
        if self.tree_dirty {
            self.send_tree(client, surface_id);
        }
        // A present naming nothing would copy every row: name the patch's first pixel.
        if damage.is_empty() {
            damage.add((u32::from(self.patch.0), u32::from(self.patch.1), 1, 1));
        }
        *seq = seq.wrapping_add(1);
        self.send_present(client, surface_id, *seq, &damage)?;
        self.note_input_answered();
        Ok(true)
    }

    /// `OP_SURFACE_PRESENT` of `damage`'s rows (waited: the queue's room, never a reply).
    pub(super) fn send_present(
        &self,
        client: &KernelClient,
        surface_id: u32,
        seq: u32,
        damage: &PresentDamage,
    ) -> Result<(), &'static str> {
        let mut rects = [wire::DamageRect { x: 0, y: 0, width: 0, height: 0 }; 4];
        let c = |v: u32| v.min(u32::from(u16::MAX)) as u16;
        for (o, r) in rects.iter_mut().zip(damage.rects()) {
            *o = wire::DamageRect { x: c(r.0), y: c(r.1), width: c(r.2), height: c(r.3) };
        }
        let n = damage.rects().len();
        let mut buf = [0u8; wire::SURFACE_PRESENT_MAX_LEN];
        let len = wire::encode_surface_present(surface_id, seq, &rects[..n], &mut buf);
        super::boot::send_wait(client, &buf[..len]).map_err(|_| "apphost: FAIL interactive present")
    }

    /// Fills the wash patch with the theme's hover colour (opaque: the wash's opacity carries
    /// its alpha) and names it in `damage`.
    pub(super) fn draw_patch(&mut self, vmo: u32, damage: &mut PresentDamage) -> bool {
        use nexus_dsl_runtime::theme_tokens::ColorToken;
        if self.store_refused {
            return true; // no store: no patch, no wash
        }
        let c = tokens_for(self.theme_mode).color(ColorToken::GlassHover);
        let side = PATCH_ROWS as usize;
        let mut row = [0u8; PATCH_ROWS as usize * 4];
        for px in row.chunks_exact_mut(4) {
            px.copy_from_slice(&[c.b, c.g, c.r, 255]);
        }
        let (px, py) = (usize::from(self.patch.0), usize::from(self.patch.1));
        for r in 0..side {
            if vmo_write(vmo, ((py + r) * self.w as usize + px) * 4, &row).is_err() {
                return false;
            }
        }
        self.wash_opacity = c.a;
        damage.add((px as u32, py as u32, PATCH_ROWS, PATCH_ROWS));
        self.tree_dirty = true;
        true
    }

    /// The structure (`OP_SURFACE_TREE`): the regions, every layer with its band, the patch — in
    /// effect with the next present.
    pub(super) fn send_tree(&mut self, client: &KernelClient, surface_id: u32) {
        let mut tree = lw::SurfaceTree::EMPTY;
        tree.region_count = self.regions(&mut tree.regions);
        let mut n = 0;
        for l in self.scroll_layers.iter().flatten() {
            if l.band.strips == 0 {
                continue; // no band: nothing to compose
            }
            tree.scrolls[n] = desc(l);
            n += 1;
        }
        tree.scroll_count = n;
        if !self.store_refused {
            tree.patches[0] = lw::Patch { x: self.patch.0, y: self.patch.1 };
            tree.patch_count = 1;
        }
        let mut buf = [0u8; lw::SURFACE_TREE_MAX_LEN];
        let len = lw::encode_surface_tree(surface_id, &tree, &mut buf);
        let _ = client.send(&buf[..len], Wait::Blocking);
        self.sent_tree = tree;
        self.tree_dirty = false;
    }

    /// The motion (`OP_SURFACE_MOTION`): every layer's offset, the wash of the hovered control —
    /// in effect at once. Checked against the structure last sent, by the rule windowd applies:
    /// windowd refuses a motion whole, offsets and all, so a wash naming what it does not know
    /// yet waits for the structure (a scroll never stalls on a wash).
    pub(super) fn send_motion(&mut self, client: &KernelClient, surface_id: u32) {
        let mut motion = lw::SurfaceMotion::default();
        let mut n = 0;
        for l in self.scroll_layers.iter().flatten().filter(|l| l.band.strips > 0) {
            motion.offsets[n] = l.offset;
            n += 1;
        }
        motion.offset_count = n;
        if let Some(w) = self.wash_motion() {
            motion.washes[0] = w;
            motion.wash_count = 1;
        }
        if lw::validate_motion(&motion, &self.sent_tree).is_err() {
            motion.wash_count = 0;
        }
        let mut buf = [0u8; lw::SURFACE_MOTION_MAX_LEN];
        let len = lw::encode_surface_motion(surface_id, &motion, &mut buf);
        let _ = client.send(&buf[..len], Wait::Blocking);
        self.motion_dirty = false;
        self.note_input_answered();
    }

    /// An input arrived at `rx_ns` (RFC-0108 §hops): the input chain's hop in the app starts — for
    /// an input that asked for work (a repaint, a motion), the first one since the last answer.
    pub(super) fn note_input_rx(&mut self, rx_ns: u64, dirty: bool) {
        if (dirty || self.motion_dirty) && self.input_rx_ns.is_none() {
            self.input_rx_ns = Some(rx_ns);
        }
    }

    /// The answer left — a present or a motion: the hop ends; its line every `INPUT_WINDOW`.
    fn note_input_answered(&mut self) {
        let Some(rx) = self.input_rx_ns.take() else { return };
        self.hop_input.note(nsec_now().saturating_sub(rx) / 1000);
        if self.hop_input.count() >= nexus_ux_budgets::INPUT_WINDOW {
            let (n, avg, max) = self.hop_input.take();
            raw_marker(&alloc::format!("apphost: hops input (rx>sent={avg}/{max}/{n})"));
        }
    }

    /// The wash of the hovered control (RFC-0109, materials rule 3): the patch spread over the
    /// control its hover anchors, in its layer's content coordinates — no raster, ever.
    fn wash_motion(&self) -> Option<lw::Wash> {
        if self.store_refused {
            return None;
        }
        let anchor = crate::hover_wash::hover_wash_anchor(&self.layout.boxes, self.hovered?)?;
        let b = self.layout.boxes.iter().find(|b| b.node_id == anchor)?;
        if b.rect.width.0 <= 0 || b.rect.height.0 <= 0 {
            return None; // nothing to wash: no area
        }
        let (mut x, mut y) = (b.rect.x.0, b.rect.y.0);
        let mut parent = 0u8;
        if let Some(root) = b.scroll_root {
            let (k, l) = self
                .scroll_layers
                .iter()
                .flatten()
                .filter(|l| l.band.strips > 0)
                .enumerate()
                .find(|(_, l)| l.root == root)?;
            if l.covered {
                return None; // under a modal box: nothing of its content takes a hover
            }
            parent = k as u8 + 1;
            x -= l.viewport.0;
            y -= l.viewport.1;
        }
        let clamp16 = |v: i32| v.clamp(0, i32::from(u16::MAX)) as u16;
        Some(lw::Wash {
            parent,
            patch: 0,
            radius: b.visual.corner_radius.top_left.0.clamp(0, 255) as u8,
            opacity: self.wash_opacity,
            x,
            y,
            w: clamp16(b.rect.width.0),
            h: clamp16(b.rect.height.0),
        })
    }
}
