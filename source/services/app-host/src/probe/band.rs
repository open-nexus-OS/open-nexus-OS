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
use super::surface_mem::SurfaceMem;
use super::*;
use nexus_display_proto::surface_layers as lw;

/// The wash patch's rows: one 16×16 patch at the store's first row.
pub(super) const PATCH_ROWS: u32 = lw::PATCH_SIDE as u32;
/// A fill pass's budget: the band never holds the physics tick longer than this.
const FILL_BUDGET_NS: u64 = 4_000_000;
/// Rows painted between two looks at the clock.
const FILL_BATCH: u32 = 8;

pub(super) use super::store::StorePlan;
use crate::band_math::{grow, next_fill, run_of};
pub(super) use crate::band_math::{Band, PresentDamage};

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
        // TASK-0351 P14: a band on a ground is opaque — windowd composes it without a blend.
        flags: if layer.ground.is_some() { lw::BAND_OPAQUE } else { 0 },
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
            || self.motion_paint_owed()
    }

    /// One fill pass: each uncovered layer's band grows toward its target — the visible window,
    /// then the margins — a run of the ring at a time, until the budget is spent. `None`: no
    /// budget, the visible windows complete and nothing more (the first frame after a content
    /// change). The rows drawn are added to `damage`. Returns whether a band changed what it
    /// holds.
    pub(super) fn fill_bands(
        &mut self,
        mem: &mut SurfaceMem,
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
                if !self.draw_band(k, q0, q1, mem, damage) {
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
        mem: &mut SurfaceMem,
        damage: &mut PresentDamage,
    ) -> bool {
        let Some(layer) = self.scroll_layers[k] else { return true };
        let desc = desc(&layer);
        let Some((sx, sy, _)) = desc.ring_place(p0) else { return true };
        let (vx0, vy0, vx1, vy1) = layer.viewport;
        let n = p1 - p0;
        let pass = super::paint::Pass::Band(layer.root);
        if layer.vertical() {
            self.prepare_picks(pass, (vy0 + p0 as i32, vy0 + p1 as i32), None);
            for i in 0..n {
                let y = vy0 + (p0 + i) as i32;
                if !self.paint_band_row(mem, y, 0, (vx0, vx1), (sy + i, sx), layer.ground) {
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
                let y = vy0 + r as i32;
                if !self.paint_band_row(mem, y, shift, (0, n as i32), (sy + r, sx), layer.ground) {
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
        mem: &mut SurfaceMem,
        span: Option<(i32, i32)>,
        damage: &mut PresentDamage,
    ) -> bool {
        let (w, h) = (self.w, self.h as i32);
        let Some((y0, y1)) = span else {
            if !self.render(mem) || !self.draw_patch(mem, damage) {
                return false;
            }
            damage.add((0, 0, w, h as u32));
            self.invalidate_bands();
            self.fill_bands(mem, None, damage);
            return self.paint_motion_layers(mem, damage, true);
        };
        let (b0, b1) = (y0.max(0), y1.min(h));
        if b1 > b0 {
            // TASK-0351 P15: the rows are painted whole, the damage names what changed in them.
            let rect = core::mem::take(&mut self.damage_cols).rect(w, (b0 as u32, b1 as u32));
            if !self.render_rows(mem, b0, b1) {
                return false;
            }
            self.motion_hops.damaged(u64::from(rect.2) * u64::from(rect.3));
            damage.add(rect);
        }
        // The rows of a motion layer the change reaches, in its store rows (TASK-0354 P2).
        if !self.redraw_motion_rows(mem, (y0, y1), damage) {
            return false;
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
                if !self.draw_band(k, q, q1, mem, damage) {
                    return false;
                }
                q = q1;
            }
        }
        true
    }

    /// One present: into the slot windowd released (RFC-0114 — the last present's rects first
    /// come forward), the model change redrawn (`change`: `Some(span)` — the span's layout rows,
    /// or everything), a fill pass of the bands within its budget, the structure when it changed,
    /// the present of the slot naming every row drawn. `Ok(true)`: a present is in flight (its
    /// ack — the release of the other slot — clears it).
    pub(super) fn present_frame(
        &mut self,
        client: &KernelClient,
        surface_id: u32,
        mem: &mut SurfaceMem,
        seq: &mut u32,
        change: Option<Option<(i32, i32)>>,
    ) -> Result<bool, &'static str> {
        let mut damage = PresentDamage::default();
        // A store re-created at the same size kept what it drew (`SurfaceMem::carry_from`): all
        // of it, named once — the other slot takes it from this present.
        if let Some(rows) = self.carried.take() {
            damage.add((0, 0, self.w, rows));
        }
        // A motion layer that went, or lost its place: its rows paint in the base again.
        let change = match (change, self.motion_repaint.take()) {
            (Some(Some((a, b))), Some((c, d))) => Some(Some((a.min(c), b.max(d)))),
            (None, Some(r)) => Some(Some(r)),
            (change, _) => change,
        };
        mem.begin_frame(change == Some(None));
        if let Some(span) = change {
            if !self.redraw(mem, span, &mut damage) {
                return Err("apphost: FAIL interactive render");
            }
        }
        // A change inside the motion layers: their store rows (TASK-0354 P5). A motion layer that
        // does not hold its subtree yet: painted once, here (P2).
        if let Some(span) = self.motion_rows.take() {
            if !self.redraw_motion_rows(mem, span, &mut damage) {
                return Err("apphost: FAIL interactive render");
            }
        }
        if !self.paint_motion_layers(mem, &mut damage, false) {
            return Err("apphost: FAIL interactive render");
        }
        self.fill_bands(mem, Some(FILL_BUDGET_NS), &mut damage);
        // Regions an animation moved (or let rest) are declared anew with this frame's pixels.
        if !self.tree_dirty && self.regions_moved() {
            self.tree_dirty = true;
        }
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
        // A present naming nothing would damage the whole surface: name the patch's first pixel.
        if damage.is_empty() {
            damage.add((u32::from(self.patch.0), u32::from(self.patch.1), 1, 1));
        }
        let slot = mem.end_frame(&damage);
        *seq = seq.wrapping_add(1);
        self.send_present(client, surface_id, (*seq, slot), &damage)?;
        self.note_input_answered();
        self.motion_answered();
        // RFC-0113 §6: the keys this present shows are answered; a field the focus left says
        // its session. §5: the caret this frame places, to windowd (it draws and blinks it).
        self.input_session.presented(nsec_now());
        self.track_input_session();
        self.sync_caret(client, surface_id);
        Ok(true)
    }

    /// `OP_SURFACE_PRESENT` of slot `slot` naming `damage`'s rows (waited: the queue's room,
    /// never a reply).
    pub(super) fn send_present(
        &self,
        client: &KernelClient,
        surface_id: u32,
        (seq, slot): (u32, u8),
        damage: &PresentDamage,
    ) -> Result<(), &'static str> {
        let mut rects = [wire::DamageRect { x: 0, y: 0, width: 0, height: 0 }; 4];
        let c = |v: u32| v.min(u32::from(u16::MAX)) as u16;
        for (o, r) in rects.iter_mut().zip(damage.rects()) {
            *o = wire::DamageRect { x: c(r.0), y: c(r.1), width: c(r.2), height: c(r.3) };
        }
        let n = damage.rects().len();
        let mut buf = [0u8; wire::SURFACE_PRESENT_MAX_LEN];
        let len = wire::encode_surface_present(surface_id, seq, slot, &rects[..n], &mut buf);
        super::boot::send_wait(client, &buf[..len]).map_err(|_| "apphost: FAIL interactive present")
    }

    /// Fills the wash patch with the theme's hover colour (opaque: the wash's opacity carries
    /// its alpha) and names it in `damage`.
    pub(super) fn draw_patch(&mut self, mem: &mut SurfaceMem, damage: &mut PresentDamage) -> bool {
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
        let (px, py) = (u32::from(self.patch.0), u32::from(self.patch.1));
        for r in 0..side as u32 {
            if !mem.write(py + r, px, &row) {
                return false;
            }
        }
        self.wash_opacity = c.a;
        damage.add((px, py, PATCH_ROWS, PATCH_ROWS));
        self.tree_dirty = true;
        true
    }

    /// Whether the regions the page declares now differ from those last sent (an animation
    /// moved a glass box, or let it rest).
    fn regions_moved(&self) -> bool {
        let mut now = [wire::LayerDesc::default(); wire::MAX_SURFACE_LAYERS];
        let n = self.regions(&mut now, &mut [0; wire::MAX_SURFACE_LAYERS]);
        let sent = &self.sent_tree;
        n != sent.region_count || now[..n] != sent.regions[..n]
    }

    /// The structure (`OP_SURFACE_TREE`): the regions, every layer with its band, the patch, the
    /// motion layers with what they claim (TASK-0354 P2) — in effect with the next present.
    pub(super) fn send_tree(&mut self, client: &KernelClient, surface_id: u32) {
        let mut tree = lw::SurfaceTree::EMPTY;
        let mut region_nodes = [0; wire::MAX_SURFACE_LAYERS];
        tree.region_count = self.regions(&mut tree.regions, &mut region_nodes);
        let mut roots = [0; super::scroll::MAX_SCROLL_LAYERS];
        let mut n = 0;
        for l in self.scroll_layers.iter().flatten() {
            if l.band.strips == 0 {
                continue; // no band: nothing to compose
            }
            tree.scrolls[n] = desc(l);
            roots[n] = l.root;
            n += 1;
        }
        tree.scroll_count = n;
        // The patch row: the hover wash's patch, then one per motion layer's backdrop.
        if !self.store_refused {
            for (j, p) in tree.patches.iter_mut().enumerate() {
                let x = self.patch.0 + PATCH_ROWS as u16 * j as u16;
                *p = lw::Patch { x, y: self.patch.1 };
            }
            tree.patch_count = lw::MAX_PATCHES;
        }
        let claims = (&region_nodes[..tree.region_count], &roots[..n]);
        tree.motion_count = self.motion_descs(claims.0, claims.1, &mut tree.motions);
        super::motion_layers::keep_claims_inside(&mut tree);
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
        self.motion_answered();
        // A scroll moved the field: its caret moves with it (RFC-0113 §5).
        self.sync_caret(client, surface_id);
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
        } else if let Some(k) = self.motion_of(anchor) {
            // A control of a motion layer: its wash rides the layer (TASK-0354 P2).
            let (rect, _) = self.motion_place(k)?;
            parent = lw::MOTION_PARENT | self.motion_wire_index(k)?;
            x -= rect.0;
            y -= rect.1;
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
