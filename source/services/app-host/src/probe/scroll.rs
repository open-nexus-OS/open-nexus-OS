// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: app-host scroll layers (RFC-0109, ADR-0076). Every top-level `.scroll(...)`
//! container of the page is a scroll layer: its content is drawn once into a virtual band of the
//! surface's store (`probe/band.rs`) and windowd composes it at the offset this module's physics
//! commits — no re-raster per step, ever. One scroll physics per container (the shared
//! `animation::ScrollMomentum`: the wheel's eased impulse, the pager's glide and page lock), the
//! wheel aimed at the layer under the pointer, `EndReached` and the page triggers fired on the
//! container that scrolled (`View::fire_on_box`), the layer views the hit-test and the covered
//! paint read. A layer under a modal box is COVERED: its content is painted into the base at its
//! frozen offset (the box above it then shows), and it takes no wheel. Plus the retained
//! re-layout every model change runs.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the geometry in `nexus-layout` (scroll roots), the band math in
//!   `nexus-display-proto::surface_layers`, the pager decision in `pager_math`; the QEMU lanes.

use super::*;
use nexus_dsl_runtime::interact::ScrollLayerView;
use nexus_scene_raster::ScrollView;

/// Scroll layers a page may have (the wire's bound): a further container is laid out and hit as
/// a layer at rest, painted in the base.
pub(super) const MAX_SCROLL_LAYERS: usize = nexus_display_proto::surface_layers::MAX_SCROLL_LAYERS;
/// The wheel's fixed step per notch (px) — predictable: N notches scroll N steps.
const STEP_PX: i32 = 72;
/// The pager's wheel lock after a turn: one flick turns ONE page (spec: 360 ms).
const PAGE_LOCK_NS: u64 = 360_000_000;

/// A scroll layer's axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Axis {
    Vertical,
    Horizontal,
    /// Horizontal, the wheel turning whole pages (the launcher pager).
    Paged,
}

/// One scroll layer of the page.
#[derive(Clone, Copy)]
pub(super) struct ScrollLayer {
    /// The container's node id (its content's `LayoutBox::scroll_root`).
    pub(super) root: usize,
    /// The viewport on the surface: x0, y0, x1, y1 (exclusive).
    pub(super) viewport: (i32, i32, i32, i32),
    pub(super) axis: Axis,
    /// The content's length along the axis (positions `0..extent`).
    pub(super) extent: i32,
    /// The offset this layer is composed at (the physics' position, committed as motion).
    pub(super) offset: i32,
    physics: animation::ScrollMomentum,
    last_ns: u64,
    lock_until_ns: u64,
    /// `EndReached` fired for this approach to the end; re-armed by a re-layout.
    end_fired: bool,
    /// A modal box lies over it: painted into the base at its offset, no wheel.
    pub(super) covered: bool,
    /// Its virtual band in the store.
    pub(super) band: super::band::Band,
}

impl ScrollLayer {
    fn new(root: usize, viewport: (i32, i32, i32, i32), axis: Axis, extent: i32) -> Self {
        let config = match axis {
            // A slightly softer ease than the wheel's so a page turn reads as the handoff's
            // ~0.42 s glide, not a snap.
            Axis::Paged => animation::ScrollConfig { rate_per_s: 10.0, fling_friction_per_s: 4.0 },
            _ => animation::ScrollConfig::default(),
        };
        let mut layer = Self {
            root,
            viewport,
            axis,
            extent,
            offset: 0,
            physics: animation::ScrollMomentum::new(config),
            last_ns: 0,
            lock_until_ns: 0,
            end_fired: false,
            covered: false,
            band: super::band::Band::default(),
        };
        layer.physics.set_extent(layer.viewport_len() as f32, extent as f32);
        layer
    }

    pub(super) fn vertical(&self) -> bool {
        self.axis == Axis::Vertical
    }

    /// The viewport's length along the axis.
    pub(super) fn viewport_len(&self) -> i32 {
        let (x0, y0, x1, y1) = self.viewport;
        if self.vertical() {
            y1 - y0
        } else {
            x1 - x0
        }
    }

    pub(super) fn max_offset(&self) -> i32 {
        (self.extent - self.viewport_len()).max(0)
    }

    /// The layer as the hit-test sees it.
    pub(super) fn hit_view(&self) -> ScrollLayerView {
        let (dx, dy) = if self.vertical() { (0, self.offset) } else { (self.offset, 0) };
        ScrollLayerView { root: self.root, viewport: self.viewport, dx, dy }
    }

    /// The layer painted where the page shows it (a covered layer, flattened into the base).
    pub(super) fn paint_view(&self) -> ScrollView {
        let (dx, dy) = if self.vertical() { (0, self.offset) } else { (self.offset, 0) };
        ScrollView { root: self.root, clip: self.viewport, dx, dy }
    }

    fn contains(&self, x: i32, y: i32) -> bool {
        let (x0, y0, x1, y1) = self.viewport;
        x >= x0 && x < x1 && y >= y0 && y < y1
    }
}

impl super::DslApp {
    /// Rebuilds the page's scroll layers from the retained layout — every top-level scroll
    /// container, in box order, up to the bound — keeping each one's offset, physics and band
    /// across the re-layout (matched by its container's node id), the offset clamped to the new
    /// content. A container past the bound is flattened: its content paints in the base at
    /// identity, clipped to its viewport. Returns whether the set's geometry changed (the store
    /// is planned again).
    pub(super) fn sync_layers(&mut self) -> bool {
        let mut next: [Option<ScrollLayer>; MAX_SCROLL_LAYERS] = [None; MAX_SCROLL_LAYERS];
        let mut n = 0;
        self.flat_views.clear();
        for b in &self.layout.boxes {
            let nexus_layout_types::Overflow::Scroll(axis) = b.overflow else { continue };
            if b.scroll_root.is_some() || b.rect.width.0 <= 0 || b.rect.height.0 <= 0 {
                continue; // nested in another's content: a clip, not a layer
            }
            let viewport = match b.content_clip {
                Some(c) => (c.x.0, c.y.0, c.x.0 + c.width.0, c.y.0 + c.height.0),
                None => (
                    b.rect.x.0,
                    b.rect.y.0,
                    b.rect.x.0 + b.rect.width.0,
                    b.rect.y.0 + b.rect.height.0,
                ),
            };
            // Inside the base layer (windowd refuses a viewport outside it).
            let (sw, sh) = (self.w as i32, self.h as i32);
            let viewport =
                (viewport.0.max(0), viewport.1.max(0), viewport.2.min(sw), viewport.3.min(sh));
            if viewport.2 <= viewport.0 || viewport.3 <= viewport.1 {
                continue;
            }
            if n >= MAX_SCROLL_LAYERS {
                let root = b.node_id;
                self.flat_views.push(ScrollView { root, clip: viewport, dx: 0, dy: 0 });
                continue;
            }
            let axis = match axis {
                nexus_layout_types::ScrollAxis::Vertical => Axis::Vertical,
                nexus_layout_types::ScrollAxis::Horizontal => Axis::Horizontal,
                nexus_layout_types::ScrollAxis::Paged => Axis::Paged,
            };
            let extent = self.content_extent(b.node_id, viewport, axis);
            let mut layer = match self.scroll_layers.iter().flatten().find(|l| l.root == b.node_id)
            {
                Some(old) if old.axis == axis => {
                    let mut l = *old;
                    l.viewport = viewport;
                    l.extent = extent;
                    l
                }
                _ => ScrollLayer::new(b.node_id, viewport, axis, extent),
            };
            layer.physics.set_extent(layer.viewport_len() as f32, extent as f32);
            layer.offset = layer.offset.clamp(0, layer.max_offset());
            layer.end_fired = false;
            // Content that left (the list got shorter) is held no more.
            let end = extent.max(0) as u32;
            layer.band.valid_len =
                layer.band.valid_len.min(end.saturating_sub(layer.band.valid_start));
            next[n] = Some(layer);
            n += 1;
        }
        if !self.flat_views.is_empty() && crate::proof_line::SCROLL_LAYERS_OVER.claim() {
            raw_marker(&alloc::format!(
                "apphost: scroll layers over the bound (dropped={})",
                self.flat_views.len()
            ));
        }
        let geometry = |s: &[Option<ScrollLayer>]| {
            let mut g = [(0usize, (0, 0, 0, 0)); MAX_SCROLL_LAYERS];
            for (slot, l) in g.iter_mut().zip(s.iter().flatten()) {
                *slot = (l.root, l.viewport);
            }
            g
        };
        let changed = geometry(&self.scroll_layers) != geometry(&next);
        self.scroll_layers = next;
        self.cover_layers();
        changed
    }

    /// Marks each layer covered when an overlay box drawn after it — not its own content —
    /// lies over its viewport (a menu, a sheet, a modal's backdrop): its content then paints in
    /// the base, under the overlay, and the band waits.
    fn cover_layers(&mut self) {
        let overlays = self.view.overlays();
        for layer in self.scroll_layers.iter_mut().flatten() {
            let (vx0, vy0, vx1, vy1) = layer.viewport;
            layer.covered = overlays.entries().iter().any(|e| {
                e.box_id > layer.root
                    && self.layout.boxes.iter().find(|b| b.node_id == e.box_id).is_some_and(|b| {
                        let (bx0, by0) = (b.rect.x.0, b.rect.y.0);
                        let (bx1, by1) = (bx0 + b.rect.width.0, by0 + b.rect.height.0);
                        b.scroll_root != Some(layer.root)
                            && bx0 < vx1
                            && vx0 < bx1
                            && by0 < vy1
                            && vy0 < by1
                    })
            });
        }
    }

    /// The content's length along `axis`: how far the boxes of the layer reach past the
    /// viewport's leading edge.
    fn content_extent(&self, root: usize, viewport: (i32, i32, i32, i32), axis: Axis) -> i32 {
        let mut end = 0;
        for b in self.layout.boxes.iter().filter(|b| b.scroll_root == Some(root)) {
            let reach = if axis == Axis::Vertical {
                b.rect.y.0 + b.rect.height.0 - viewport.1
            } else {
                b.rect.x.0 + b.rect.width.0 - viewport.0
            };
            end = end.max(reach);
        }
        end
    }

    /// The layers as the hit-test sees them.
    pub(super) fn hit_views(&self) -> ([ScrollLayerView; MAX_SCROLL_LAYERS], usize) {
        let mut out =
            [ScrollLayerView { root: 0, viewport: (0, 0, 0, 0), dx: 0, dy: 0 }; MAX_SCROLL_LAYERS];
        let mut n = 0;
        for l in self.scroll_layers.iter().flatten() {
            out[n] = l.hit_view();
            n += 1;
        }
        (out, n)
    }

    /// The views the BASE paints scroll content through, into `out`: each covered layer's (its
    /// content under an overlay, at its offset) and each flattened container's (at identity,
    /// inside its viewport — as the hit-test places it).
    pub(super) fn base_views(&self, out: &mut alloc::vec::Vec<ScrollView>) {
        out.clear();
        out.extend(
            self.scroll_layers.iter().flatten().filter(|l| l.covered).map(|l| l.paint_view()),
        );
        out.extend_from_slice(&self.flat_views);
    }

    /// Where a box's top-left shows on the surface: its layout place, moved by its scroll
    /// layer's offset.
    pub(super) fn on_surface(&self, b: &nexus_layout::LayoutBox) -> (i32, i32) {
        let (x, y) = (b.rect.x.0, b.rect.y.0);
        match b.scroll_root.and_then(|r| self.scroll_layers.iter().flatten().find(|l| l.root == r))
        {
            Some(l) if l.vertical() => (x, y - l.offset),
            Some(l) => (x - l.offset, y),
            None => (x, y),
        }
    }

    /// Whether a box is drawn in a band, not the base: the content of an uncovered layer.
    pub(super) fn in_band(&self, b: &nexus_layout::LayoutBox) -> bool {
        b.scroll_root
            .is_some_and(|r| self.scroll_layers.iter().flatten().any(|l| l.root == r && !l.covered))
    }

    /// The layer under `(x, y)`, else the largest — where a wheel notch goes.
    fn wheel_target(&self) -> Option<usize> {
        let at = self.pointer.and_then(|(x, y)| {
            self.scroll_layers.iter().position(|l| l.is_some_and(|l| l.contains(x, y)))
        });
        at.or_else(|| {
            self.scroll_layers
                .iter()
                .enumerate()
                .filter_map(|(k, l)| l.map(|l| (k, l)))
                .max_by_key(|(_, l)| {
                    let (x0, y0, x1, y1) = l.viewport;
                    i64::from(x1 - x0) * i64::from(y1 - y0)
                })
                .map(|(k, _)| k)
        })
    }

    /// A wheel notch (`INPUT_KIND_WHEEL`): an impulse into the physics of the layer under the
    /// pointer, a page turn on a pager. The offset is committed as motion — nothing re-rasters.
    /// Returns whether the model changed (a trigger dispatched: the caller repaints).
    pub(super) fn wheel_event(
        &mut self,
        client: &KernelClient,
        surface_id: u32,
        y_raw: u16,
        rx_markers: &mut u32,
    ) -> bool {
        let delta = wire::wheel_delta_from_wire(y_raw);
        if *rx_markers < 40 {
            *rx_markers += 1;
            raw_marker(&alloc::format!("APPHOST: wheel rx n={rx_markers} d={delta}"));
        }
        let Some(k) = self.wheel_target() else { return false };
        let Some(mut layer) = self.scroll_layers[k] else { return false };
        if layer.covered || delta == 0 {
            return false;
        }
        let now = nsec_now();
        let mut trigger = None;
        if layer.axis == Axis::Paged {
            let view = layer.viewport_len();
            let Some(turn) = crate::pager_math::page_turn(
                layer.physics.target() as i32,
                view,
                layer.max_offset(),
                delta,
                now < layer.lock_until_ns,
            ) else {
                return false;
            };
            let glide = turn.target_px as f32 - layer.physics.target();
            let _ = layer.physics.scroll_wheel(glide);
            layer.lock_until_ns = now + PAGE_LOCK_NS;
            trigger = Some(if turn.dir > 0 { "PageNext" } else { "PagePrev" });
        } else {
            if layer.max_offset() == 0 {
                return false;
            }
            // Linux REL_WHEEL: +1 = wheel UP (away from the user) ⇒ wheel DOWN grows the offset.
            let _ = layer.physics.scroll_wheel((-delta * STEP_PX) as f32);
        }
        if !layer.physics.is_animating() || layer.last_ns == 0 {
            layer.last_ns = now;
        }
        self.scroll_layers[k] = Some(layer);
        // The first eased step answers within this frame; the pulse ticks the rest.
        let mut changed = self.step_layer(k);
        if let Some(name) = trigger {
            changed |= self.fire_at(layer.root, name);
        }
        if self.momentum_active() {
            let _ = client.send(&wire::encode_surface_frame_req(surface_id), Wait::Blocking);
        }
        changed
    }

    /// The frame pulse: every moving layer's physics advances by real elapsed time and its
    /// offset is committed. Returns whether the model changed (`EndReached` loaded more).
    pub(super) fn momentum_tick(&mut self) -> bool {
        let now = nsec_now();
        let mut changed = false;
        for k in 0..MAX_SCROLL_LAYERS {
            let Some(mut layer) = self.scroll_layers[k] else { continue };
            if !layer.physics.is_animating() {
                continue;
            }
            let dt = now.saturating_sub(layer.last_ns).min(100_000_000);
            layer.last_ns = now;
            let _ = layer.physics.tick(dt);
            self.scroll_layers[k] = Some(layer);
            changed |= self.step_layer(k);
        }
        changed
    }

    /// Commits layer `k`'s physics position as its offset (motion), and fires its
    /// `EndReached` once per approach to the content end. Returns whether the model changed.
    fn step_layer(&mut self, k: usize) -> bool {
        let Some(mut layer) = self.scroll_layers[k] else { return false };
        let pos = (layer.physics.offset_px().max(0)).min(layer.max_offset());
        if pos != layer.offset {
            layer.offset = pos;
            self.motion_dirty = true;
        }
        let len = layer.viewport_len();
        let near_end = layer.axis != Axis::Paged
            && layer.max_offset() > 0
            && pos >= layer.max_offset() - len / 2;
        let fire = near_end && !layer.end_fired;
        if fire {
            layer.end_fired = true;
        }
        self.scroll_layers[k] = Some(layer);
        // Content moved under a still pointer: what it hovers may have changed.
        self.rehover();
        fire && self.fire_at(layer.root, "EndReached")
    }

    /// Whether any layer's physics still eases or glides (the loop keeps the pulse armed).
    pub(super) fn momentum_active(&self) -> bool {
        self.scroll_layers.iter().flatten().any(|l| l.physics.is_animating())
    }

    /// Glides the pager to an absolute page index (the store's `SetPage` dot tap, via the
    /// `svc.shell.scrollToPage` effect). No trigger fires — the store asked for the page.
    pub(super) fn pager_scroll_to(&mut self, page: i32) {
        let Some(k) =
            self.scroll_layers.iter().position(|l| l.is_some_and(|l| l.axis == Axis::Paged))
        else {
            return;
        };
        let Some(mut layer) = self.scroll_layers[k] else { return };
        let target =
            crate::pager_math::page_target_px(page, layer.viewport_len(), layer.max_offset());
        let glide = target as f32 - layer.physics.target();
        let _ = layer.physics.scroll_wheel(glide);
        layer.last_ns = nsec_now();
        self.scroll_layers[k] = Some(layer);
    }

    /// Dispatches the page's declarative `on WindowsChanged` handler (RFC-0086): the
    /// compositor's window set moved, so the shell re-runs its enumerate effect. BY NAME — "the
    /// windows changed" has no pixel to hit-test.
    pub(super) fn fire_windows_changed(&mut self) -> bool {
        self.fire_named("WindowsChanged")
    }

    /// Re-run layout for the CURRENT scene (model changed) and reconcile the scroll layers:
    /// offsets clamp to the new content, the end triggers re-arm, the store follows a new set.
    pub(super) fn relayout_retained(&mut self) {
        self.relayout_in_generation();
        // OUTSIDE the generation: the animation driver's Vecs are DURABLE state.
        // Synced inside it, their pushes landed in arena memory the layout two
        // frames later reset under a running fade (ADR-0065 amendment, 2026-10-06).
        self.anim_sync();
        // The store follows a new set at once (arithmetic only), so whatever presents next places
        // the bands where its structure says; a set that outgrew the surface's rows waits for
        // the loop's re-create.
        if self.sync_layers() {
            let plan = self.plan_store();
            self.store_replan |= self.outgrows(plan);
        }
        // What the bands hold is redrawn where the change's damage names it (`band::redraw`).
    }

    fn relayout_in_generation(&mut self) {
        // ADR-0065: THIS is the frame phase. Everything allocated from here to
        // the end of the function comes from the generation arena and is freed
        // wholesale two frames later.
        //
        // The guard sits inside the function rather than at its call sites: one
        // place cannot be forgotten. It also fixes the depth at TWO — a caller
        // may not hold a PREVIOUS layout's memory across more than one
        // `relayout_retained`. Exactly one caller holds one (the damage diff keeps
        // `old_boxes`/`old_texts`), across exactly one.
        #[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
        let _generation =
            nexus_service_entry::os::frame_generation(nexus_service_entry::os::Region::Layout);
        let engine = nexus_layout::LayoutEngine::new();
        let Ok(layout) = engine.layout_with_viewport(
            self.view.scene(),
            nexus_layout_types::FxPx::new(self.w as i32),
            Some(nexus_layout_types::FxPx::new(self.h as i32)),
            &nexus_text_baked::measure_text::BakedTextMeasure,
        ) else {
            return;
        };
        self.layout = layout;
        // FRESH, not `clear()`+refill: a buffer kept across frames would be
        // written in generation g+1 and reset underneath it in g+2 (ADR-0065).
        let mut texts = alloc::vec::Vec::new();
        collect_texts(self.view.scene(), &mut 0, &mut texts);
        self.texts = texts;
        // Store-window proof: with `tail(messages, 256)` the resident text run count stays
        // bounded no matter how many pages are loaded. Logged on a new high only (a
        // keystroke's relayout would put typing rhythm into the log).
        if crate::proof_line::SCROLL_TEXTS.raise(self.texts.len()) {
            raw_marker(&alloc::format!("apphost: scroll window texts={}", self.texts.len()));
        }
        // …and ONCE, the detail: content, resolved face and box per run.
        if core::mem::take(&mut self.text_dump_pending) {
            super::paint::collect_dump_text_runs(&self.texts, &self.layout.boxes);
        }
        self.report_frame_arena();
    }

    /// The lowest row the layout reaches (a layer's content runs far below the surface): the
    /// bound of a damage span in layout coordinates.
    pub(super) fn layout_bottom(&self) -> i32 {
        (self.layout.content_height.0).max(self.h as i32)
    }
}
