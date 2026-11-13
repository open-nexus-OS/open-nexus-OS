// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: app-host's MOTION LAYERS (RFC-0109 §motion layers, TASK-0354 P2): a `.transition`
//! node is a layer the compositor moves. Its enter is stated once — the layer's rest, its pixels
//! in the store, the token, its duration, where it starts — and the app paints the subtree ONCE,
//! at rest, into the store (the base is painted without it: what lies under a panel shows while
//! it fades in); windowd evaluates the motion for each shown frame and draws the layer, the
//! regions, scroll layers and washes it claims, moved and faded. The app is out of the frame
//! loop: no pulse, no raster per frame. The layer stays while its node is mounted (a change inside
//! it paints its store rows, never the base's — P5) and is only possible where painting it apart
//! composes to the same pixels as painting it in place: on the desktop's clear base anything, on
//! a window's frosted base a subtree whose boxes lie over it (P7, `composes_apart`). A modal's or a toast's layer (`.overlay(modal|transient)`) is drawn in
//! front of the windows and the shell's chrome; a root that is one flat fill over the surface (a
//! modal's scrim) is the layer's backdrop — one patch texel windowd spreads and fades, the store
//! holds only the sheet. A node the compositor cannot draw stays the app's (Tier 2).
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the decisions in `crate::motion_math` and `crate::layout_diff`, the wire's
//!   bounds in `nexus-display-proto::surface_layers` (host); the lanes' `apphost: motion layer (`

use super::band::PATCH_ROWS;
use super::surface_mem::SurfaceMem;
use super::*;
use crate::band_math::PresentDamage;
use crate::layout_diff::TextRun;
use crate::motion_math::{
    anchor_point, anchor_shift, backdrop_of, body_extent, composes_apart, enter_from, paint_extent,
    subtree_span, Span,
};
use animation::MotionToken;
use nexus_display_proto::surface_layers as lw;
use nexus_dsl_runtime::overlay::OverlayKind;
use nexus_dsl_runtime::AnimKind;
use nexus_layout::LayoutBox;
use nexus_layout_types::Rgba8;

/// Motion layers a surface keeps (the wire's bound).
pub(super) const MAX_MOTION_LAYERS: usize = lw::MAX_MOTION_LAYERS;
/// A motion is over this long after its end: the next structure declares it at rest (a later
/// compositor never replays it).
const OVER_AFTER_NS: u64 = 250_000_000;

/// One motion layer.
#[derive(Clone, Copy)]
pub(super) struct MotionLayer {
    /// The node whose `.transition` it is, and one past its subtree's last node (pre-order).
    root: usize,
    end: usize,
    /// Where its subtree paints at rest `(x0, y0, x1, y1)`, surface pixels (its backdrop aside).
    pub(super) rect: Span,
    /// Where its pixels are in the store (`None`: not placed — the base paints the subtree).
    pub(super) store: Option<(u16, u16)>,
    token: u8,
    duration_ms: u16,
    /// Where it starts: an offset, an opacity and a scale in 1/10000.
    from: (i16, i16, u8, u16),
    /// The motion's identity on the wire: a new one starts the motion in windowd.
    id: u16,
    declared_ns: u64,
    /// Its store rows hold its subtree.
    pub(super) painted: bool,
    /// A modal's or a toast's layer: drawn in front of everything else of the surface.
    front: bool,
    /// Its root's flat fill over the whole surface (a scrim): the compositor draws it.
    backdrop: Option<Rgba8>,
}

impl MotionLayer {
    /// Its size in pixels.
    pub(super) fn size(&self) -> (u32, u32) {
        ((self.rect.2 - self.rect.0) as u32, (self.rect.3 - self.rect.1) as u32)
    }

    fn holds(&self, node_id: usize) -> bool {
        node_id >= self.root && node_id < self.end
    }

    /// The rows the base paints when it takes the subtree back: its rest's — every row when its
    /// root is a backdrop (the scrim covers the surface).
    fn base_rows(&self, h: i32) -> (i32, i32) {
        if self.backdrop.is_some() {
            (0, h)
        } else {
            (self.rect.1, self.rect.3)
        }
    }

    /// Whether `[first, end)` and its subtree share a node (one inside the other).
    fn nests(&self, (first, end): (usize, usize)) -> bool {
        first < self.end && self.root < end
    }
}

/// A motion layer claims only what lies inside its rest (windowd refuses a structure whole
/// otherwise): a claim whose region or viewport reaches outside is dropped — that part rests.
pub(super) fn keep_claims_inside(tree: &mut lw::SurfaceTree) {
    let inside = |r: (u16, u16, u16, u16), m: &lw::MotionLayerDesc| {
        r.0 >= m.x
            && r.1 >= m.y
            && u32::from(r.0) + u32::from(r.2) <= u32::from(m.x) + u32::from(m.w)
            && u32::from(r.1) + u32::from(r.3) <= u32::from(m.y) + u32::from(m.h)
    };
    let (regions, scrolls) = (tree.regions, tree.scrolls);
    let (nr, ns) = (tree.region_count, tree.scroll_count);
    for m in tree.motions.iter_mut().take(tree.motion_count) {
        for (k, r) in regions.iter().enumerate().take(nr) {
            if m.claims_region(k) && !inside((r.x, r.y, r.w, r.h), m) {
                m.regions &= !(1 << k);
            }
        }
        for (k, s) in scrolls.iter().enumerate().take(ns) {
            if m.claims_scroll(k) && !inside((s.vx, s.vy, s.vw, s.vh), m) {
                m.scrolls &= !(1 << k);
            }
        }
    }
}

fn union(a: Option<(i32, i32)>, b: (i32, i32)) -> Option<(i32, i32)> {
    Some(a.map_or(b, |a| (a.0.min(b.0), a.1.max(b.1))))
}

/// The patch a layer in slot `k` draws its backdrop from: the first and the last patch are the
/// interaction layers' (`crate::interaction_layer::WIRE_PATCHES`), the ones between one per slot
/// while they last (a layer past them paints its backdrop into its store rows).
fn backdrop_patch(k: usize) -> Option<usize> {
    (k + 2 < lw::MAX_PATCHES).then_some(k + 1)
}

impl super::DslApp {
    /// A `.transition` enter of `token` on `node_id`: a motion layer when the compositor can draw
    /// it — a token that moves and fades, a duration (a modal or a toast without one is still a
    /// layer: it rests in front), a subtree that composes apart on its base (anything on the
    /// desktop's clear base; on a window's frost what lies over it), that is not another layer's,
    /// room in the bound and the store. The same node entering again restarts its motion; a node
    /// the base painted until now (`in_base`) leaves its rows there to paint again. `false`: the
    /// app animates it (Tier 2).
    pub(super) fn start_motion_layer(
        &mut self,
        node_id: usize,
        token: MotionToken,
        in_base: bool,
    ) -> bool {
        let front = self.front_overlay(node_id);
        let ms = tokens_for(self.theme_mode).motion_ms(token.duration());
        let Some(from) = enter_from(token).filter(|_| ms > 0 || front) else { return false };
        let duration_ms = ms.min(u32::from(lw::MAX_MOTION_MS)) as u16;
        self.motion_seq = self.motion_seq.wrapping_add(1).max(1);
        let (id, now) = (self.motion_seq, nsec_now());
        if let Some(k) =
            self.motion_layers.iter().position(|m| m.is_some_and(|m| m.root == node_id))
        {
            let Some(m) = self.motion_layers[k] else { return false };
            let from = self.from_anchor(token, from, (m.root, m.end), m.rect, m.backdrop.is_some());
            let Some(m) = self.motion_layers[k].as_mut() else { return false };
            (m.id, m.declared_ns, m.token, m.duration_ms, m.from) =
                (id, now, token.id(), duration_ms, from);
            self.tree_dirty = true;
            return m.store.is_some();
        }
        let Some((first, end)) = subtree_span(self.view.scene(), node_id) else { return false };
        // On a base that is not clear (a window's frost) only what lies over it composes apart.
        if self.page_base_alpha() != 0 && !composes_apart(&self.layout.boxes, (first, end)) {
            return false;
        }
        let in_band = self.layout.boxes.iter().any(|b| b.node_id == node_id && self.in_band(b));
        let nested = self.motion_layers.iter().flatten().any(|m| m.nests((first, end)));
        let Some(slot) =
            self.motion_layers.iter().position(Option::is_none).filter(|_| !in_band && !nested)
        else {
            return false;
        };
        let Some((rect, backdrop)) = self.layer_shape((first, end), slot) else { return false };
        let from = self.from_anchor(token, from, (first, end), rect, backdrop.is_some());
        self.motion_layers[slot] = Some(MotionLayer {
            root: first,
            end,
            rect,
            store: None,
            token: token.id(),
            duration_ms,
            from,
            id,
            declared_ns: now,
            painted: false,
            front,
            backdrop,
        });
        // Placed now (arithmetic only): a store too small waits for the surface's re-create.
        let mut plan = self.plan_store();
        let placed = self.motion_layers[slot].is_some_and(|m| m.store.is_some());
        match self.motion_layers[slot] {
            // The base held the subtree until now: its rows paint again, without it.
            Some(m) if placed && in_base => {
                self.motion_repaint = union(self.motion_repaint, m.base_rows(self.h as i32));
            }
            Some(_) if placed => {}
            _ => {
                // No room for it: the app paints it; the layers that gave way with it come back.
                self.motion_layers[slot] = None;
                plan = self.plan_store();
            }
        }
        self.store_replan |= self.outgrows(plan);
        placed
    }

    /// Where the enter `from` of `token` starts for the layer of `[first, end)` resting at
    /// `rect`: a token that grows from the corner its surface hangs on (`MotionOrigin::Anchor`,
    /// a menu) starts shifted towards that corner — the body's (the subtree without its scrim and
    /// its shadows' reach), on the side nearer the edge of the transition node's box.
    fn from_anchor(
        &self,
        token: MotionToken,
        from: (i16, i16, u8, u16),
        (first, end): (usize, usize),
        rect: Span,
        scrim: bool,
    ) -> (i16, i16, u8, u16) {
        if token.origin() != animation::MotionOrigin::Anchor {
            return from;
        }
        let Some((body, within)) = self.anchor_spans((first, end), scrim) else { return from };
        let (dx, dy) = anchor_shift(from.3, rect, body, within);
        (from.0.saturating_add(dx), from.1.saturating_add(dy), from.2, from.3)
    }

    /// What a corner is judged by for the subtree `[first, end)`: its body (without a scrim
    /// root and without its shadows' reach) and the box of the node that carries the transition.
    fn anchor_spans(&self, (first, end): (usize, usize), scrim: bool) -> Option<(Span, Span)> {
        let size = (self.w as i32, self.h as i32);
        let span = if scrim { (first + 1, end) } else { (first, end) };
        let r = &self.layout.boxes.iter().find(|b| b.node_id == first)?.rect;
        let within = (r.x.0, r.y.0, r.x.0 + r.width.0, r.y.0 + r.height.0);
        let body = body_extent(&self.layout.boxes, span, size, |id| self.has_text(id))?;
        Some((body, within))
    }

    /// The point an enter of `node_id` the app paints itself scales about (a window's frosted
    /// base carries no motion layer): the corner a token that grows from its anchor hangs on —
    /// the same corner the compositor's layer keeps. `None`: the node's centre, as every other
    /// token.
    pub(super) fn enter_anchor(&self, node_id: usize) -> Option<(i32, i32)> {
        let (_, intent) = self
            .view
            .animations()
            .iter()
            .find(|(id, i)| *id == node_id && i.kind == AnimKind::Transition)?;
        let token = MotionToken::from_id(intent.token)?;
        if token.origin() != animation::MotionOrigin::Anchor {
            return None;
        }
        let span = subtree_span(self.view.scene(), node_id)?;
        let size = (self.w as i32, self.h as i32);
        let root = self.layout.boxes.iter().find(|b| b.node_id == span.0)?;
        let scrim = backdrop_of(root, size, self.has_text(span.0)).is_some();
        let (body, within) = self.anchor_spans(span, scrim)?;
        Some(anchor_point(body, within))
    }

    /// Whether `node_id` is a modal's or a toast's layer (`.overlay(modal|transient)`): in front.
    fn front_overlay(&self, node_id: usize) -> bool {
        self.view.overlays().entries().iter().any(|e| {
            e.box_id == node_id && matches!(e.kind, OverlayKind::Modal | OverlayKind::Transient)
        })
    }

    /// Where the subtree `[first, end)` paints at rest for a layer in slot `slot`, and its
    /// backdrop: a root that is one flat fill over the whole surface (a scrim) is drawn by the
    /// compositor — the rest is what its other nodes paint — while the slot has a patch for it.
    /// `None`: nothing to paint.
    fn layer_shape(
        &self,
        (first, end): (usize, usize),
        slot: usize,
    ) -> Option<(Span, Option<Rgba8>)> {
        let size = (self.w as i32, self.h as i32);
        let backdrop = backdrop_patch(slot)
            .and_then(|_| self.layout.boxes.iter().find(|b| b.node_id == first))
            .and_then(|b| backdrop_of(b, size, self.has_text(first)));
        let span = if backdrop.is_some() { (first + 1, end) } else { (first, end) };
        let rect = paint_extent(&self.layout.boxes, span, size, |id| self.has_text(id))?;
        Some((rect, backdrop))
    }

    /// After a re-emit: a motion layer whose root no longer enters (`entering`: the nodes that
    /// carry a `.transition` now) goes — its subtree, if still there, paints in the base again —
    /// and every other is re-resolved: a subtree that paints elsewhere paints its layer again, a
    /// new size is placed again.
    pub(super) fn sync_motion_layers(&mut self, entering: &[usize]) {
        let mut replan = false;
        for k in 0..MAX_MOTION_LAYERS {
            let Some(mut m) = self.motion_layers[k] else { continue };
            let now = subtree_span(self.view.scene(), m.root);
            let span = now.filter(|_| entering.contains(&m.root));
            let shape = span.and_then(|s| self.layer_shape(s, k));
            // A layer that came to hold another one's nodes goes too (a node is one layer's).
            let nested = span.is_some_and(|s| {
                (0..MAX_MOTION_LAYERS)
                    .any(|j| j != k && self.motion_layers[j].is_some_and(|o| o.nests(s)))
            });
            let (Some((_, end)), Some((rect, backdrop)), false) = (span, shape, nested) else {
                // Its subtree still mounted (the same nodes at the same place in the scene): the
                // base paints it again. One that left the scene left no pixel in the base.
                if now == Some((m.root, m.end)) {
                    self.motion_repaint = union(self.motion_repaint, m.base_rows(self.h as i32));
                }
                self.motion_layers[k] = None;
                replan = true;
                continue;
            };
            if (rect, end, backdrop) != (m.rect, m.end, m.backdrop) {
                replan |= (rect.2 - rect.0, rect.3 - rect.1)
                    != (m.rect.2 - m.rect.0, m.rect.3 - m.rect.1);
                (m.rect, m.end, m.backdrop, m.painted) = (rect, end, backdrop, false);
            }
            m.front = self.front_overlay(m.root);
            self.motion_layers[k] = Some(m);
        }
        if replan {
            let plan = self.plan_store();
            self.store_replan |= self.outgrows(plan);
        }
    }

    /// The store places a plan gave the motion layers: a new place paints the layer again; a
    /// layer that lost its place paints in the base again.
    pub(super) fn place_motions(&mut self, places: [Option<(u16, u16)>; MAX_MOTION_LAYERS]) {
        let h = self.h as i32;
        for (slot, place) in self.motion_layers.iter_mut().zip(places) {
            let Some(m) = slot.as_mut() else { continue };
            if m.store != place {
                if m.store.is_some() && place.is_none() {
                    self.motion_repaint = union(self.motion_repaint, m.base_rows(h));
                }
                (m.store, m.painted) = (place, false);
            }
        }
    }

    /// Whether node `node_id` carries a text run (it paints glyphs).
    pub(super) fn has_text(&self, node_id: usize) -> bool {
        self.texts.binary_search_by_key(&node_id, |t| t.node).is_ok()
    }

    /// The placed motion layer whose subtree holds node `node_id`.
    pub(super) fn motion_of(&self, node_id: usize) -> Option<usize> {
        self.motion_layers
            .iter()
            .position(|m| m.is_some_and(|m| m.store.is_some() && m.holds(node_id)))
    }

    /// Whether motion layer `k`'s own raster leaves node `node_id` out: the root its backdrop is.
    pub(super) fn motion_skips(&self, k: usize, node_id: usize) -> bool {
        self.motion_layers
            .get(k)
            .copied()
            .flatten()
            .is_some_and(|m| m.backdrop.is_some() && m.root == node_id)
    }

    /// The placed motion layer `k`: its rest and where its pixels are.
    pub(super) fn motion_place(&self, k: usize) -> Option<(Span, (u16, u16))> {
        let m = self.motion_layers.get(k).copied().flatten()?;
        Some((m.rect, m.store?))
    }

    /// The placed motion layers' slots in the app's paint order (pre-order: a modal after the
    /// panel it may cover) — the order on the wire, the order windowd draws them in.
    fn wire_order(&self) -> ([usize; MAX_MOTION_LAYERS], usize) {
        let mut out = [0; MAX_MOTION_LAYERS];
        let mut n = 0;
        for (k, m) in self.motion_layers.iter().enumerate() {
            if m.is_some_and(|m| m.store.is_some()) {
                out[n] = k;
                n += 1;
            }
        }
        let roots = |k: usize| self.motion_layers[k].map_or(usize::MAX, |m| m.root);
        out[..n].sort_unstable_by_key(|&k| roots(k));
        (out, n)
    }

    /// The structure's motion layers into `out`, in paint order: each placed one, what it claims
    /// — `regions`: the node of each region the structure declares; `scrolls`: each scroll layer's
    /// root in the structure's order — its motion (at rest once it is over), whether it is in
    /// front, its backdrop. Returns the count.
    pub(super) fn motion_descs(
        &self,
        regions: &[usize],
        scrolls: &[usize],
        out: &mut [lw::MotionLayerDesc; MAX_MOTION_LAYERS],
    ) -> usize {
        let now = nsec_now();
        let (order, n) = self.wire_order();
        let scrim_ms = tokens_for(self.theme_mode)
            .motion_ms(nexus_dsl_runtime::theme_tokens::MotionDurationToken::Quick)
            .min(u32::from(lw::MAX_MOTION_MS)) as u16;
        for (slot, &k) in out.iter_mut().zip(&order[..n]) {
            let Some(m) = self.motion_layers[k] else { continue };
            let Some((sx, sy)) = m.store else { continue };
            let mask = |ids: &[usize]| {
                ids.iter()
                    .enumerate()
                    .filter(|(_, &id)| m.holds(id))
                    .fold(0u32, |a, (k, _)| a | 1 << k)
            };
            let over = now > m.declared_ns + u64::from(m.duration_ms) * 1_000_000 + OVER_AFTER_NS;
            let (w, h) = m.size();
            let backdrop = m.backdrop.zip(backdrop_patch(k));
            *slot = lw::MotionLayerDesc {
                x: m.rect.0 as u16,
                y: m.rect.1 as u16,
                w: w as u16,
                h: h as u16,
                sx,
                sy,
                id: m.id,
                token: m.token,
                duration_ms: if over { 0 } else { m.duration_ms },
                from_dx: m.from.0,
                from_dy: m.from.1,
                from_opacity: m.from.2,
                from_scale: if m.from.3 == 10_000 { 0 } else { m.from.3 },
                regions: mask(regions) as u16,
                scrolls: mask(scrolls) as u8,
                front: m.front,
                backdrop: backdrop.map_or(0, |(_, p)| p as u8 + 1),
                backdrop_opacity: backdrop.map_or(0, |(c, _)| c.a),
                // A scrim arrives in the theme's quick step (the sheet springs in its own).
                backdrop_ms: if backdrop.is_some() && !over { scrim_ms } else { 0 },
            };
        }
        n
    }

    /// The wire place of motion layer `k` among the placed ones (a wash rides it by that number).
    pub(super) fn motion_wire_index(&self, k: usize) -> Option<u8> {
        let (order, n) = self.wire_order();
        order[..n].iter().position(|&j| j == k).map(|i| i as u8)
    }

    /// The placed motion layers' subtrees (pre-order spans) — taken before a relayout, the old
    /// layout's numbering (TASK-0354 P5).
    pub(super) fn held_spans(&self) -> [Option<(usize, usize)>; MAX_MOTION_LAYERS] {
        let mut out = [None; MAX_MOTION_LAYERS];
        for (o, m) in out.iter_mut().zip(self.motion_layers.iter()) {
            *o = m.filter(|m| m.store.is_some()).map(|m| (m.root, m.end));
        }
        out
    }

    /// What the relayout from `old_boxes`/`old_texts` changed, split by the motion layers
    /// (TASK-0354 P5; `held`: their subtrees before it): what they hold paints their store rows
    /// (queued in `motion_rows`), never the base's. Returns the base's rows (the
    /// `layout_diff::changed_row_span` convention).
    pub(super) fn layout_change(
        &mut self,
        (old_boxes, old_texts): (&[LayoutBox], &[TextRun]),
        held: &[Option<(usize, usize)>; MAX_MOTION_LAYERS],
    ) -> Option<(i32, i32)> {
        let held_old = |id: usize| held.iter().flatten().any(|&(a, b)| id >= a && id < b);
        let held_new = |id: usize| self.motion_of(id).is_some();
        let changed = crate::layout_diff::changed_rows(
            old_boxes,
            &self.layout.boxes,
            (old_texts, &self.texts),
            self.layout_bottom(),
            (&held_old, &held_new),
        );
        if let Some(rows) = changed.layers {
            self.motion_rows = union(self.motion_rows, rows);
        }
        changed.base
    }

    /// Whether a motion layer owes its store a paint: a change inside one, one not painted yet.
    pub(super) fn motion_paint_owed(&self) -> bool {
        self.motion_rows.is_some()
            || self.motion_layers.iter().flatten().any(|m| m.store.is_some() && !m.painted)
    }

    /// Paints each placed motion layer that does not hold its subtree yet — every one for `all` —
    /// into its store rows (and its backdrop's patch), named in `damage`; once per layer, its cost
    /// said. `false` when the surface refused a write.
    pub(super) fn paint_motion_layers(
        &mut self,
        mem: &mut SurfaceMem,
        damage: &mut PresentDamage,
        all: bool,
    ) -> bool {
        for k in 0..MAX_MOTION_LAYERS {
            let Some(m) =
                self.motion_layers[k].filter(|m| m.store.is_some() && (all || !m.painted))
            else {
                continue;
            };
            let first = !m.painted;
            let t0 = nsec_now();
            if !self.redraw_motion(mem, k, (m.rect.1, m.rect.3), damage)
                || !self.paint_backdrop(mem, k, damage)
            {
                return false;
            }
            // Its motion's time runs from here: the structure that carries it leaves with this
            // present (a first frame may take a second to paint — never counted as the motion).
            if let Some(l) = self.motion_layers[k].as_mut() {
                l.painted = true;
                if first {
                    l.declared_ns = nsec_now();
                }
            }
            if first {
                let (w, h) = m.size();
                let us = nsec_now().saturating_sub(t0) / 1000;
                raw_marker(&alloc::format!(
                    "apphost: motion layer (id={} token={} w={w} h={h} front={} backdrop={} \
                     paint_us={us})",
                    m.id,
                    m.token,
                    u8::from(m.front),
                    u8::from(m.backdrop.is_some()),
                ));
            }
        }
        true
    }

    /// Motion layer `k`'s backdrop colour into its patch (opaque: the backdrop's opacity carries
    /// the alpha), named in `damage`.
    fn paint_backdrop(
        &mut self,
        mem: &mut SurfaceMem,
        k: usize,
        damage: &mut PresentDamage,
    ) -> bool {
        let Some(c) = self.motion_layers[k].and_then(|m| m.backdrop) else { return true };
        let Some(p) = backdrop_patch(k) else { return true };
        let mut row = [0u8; PATCH_ROWS as usize * 4];
        for px in row.chunks_exact_mut(4) {
            px.copy_from_slice(&[c.b, c.g, c.r, 255]);
        }
        let (px, py) = (u32::from(self.patch.0) + PATCH_ROWS * p as u32, u32::from(self.patch.1));
        for r in 0..PATCH_ROWS {
            if !mem.write(py + r, px, &row) {
                return false;
            }
        }
        damage.add((px, py, PATCH_ROWS, PATCH_ROWS));
        true
    }

    /// Layout rows `span` of every placed motion layer that holds them, painted again into its
    /// store rows (a change inside a layer) and named in `damage`.
    pub(super) fn redraw_motion_rows(
        &mut self,
        mem: &mut SurfaceMem,
        span: (i32, i32),
        damage: &mut PresentDamage,
    ) -> bool {
        for k in 0..MAX_MOTION_LAYERS {
            if self.motion_layers[k].is_some_and(|m| m.store.is_some() && m.painted)
                && !self.redraw_motion(mem, k, span, damage)
            {
                return false;
            }
        }
        true
    }

    /// Rows `span` of motion layer `k` into its store rows, named in `damage`.
    fn redraw_motion(
        &mut self,
        mem: &mut SurfaceMem,
        k: usize,
        span: (i32, i32),
        damage: &mut PresentDamage,
    ) -> bool {
        let Some((rect, (sx, sy))) = self.motion_place(k) else { return true };
        let (y0, y1) = (span.0.max(rect.1), span.1.min(rect.3));
        if y1 <= y0 {
            return true;
        }
        if !self.render_motion_rows(mem, k, (y0, y1)) {
            return false;
        }
        let row = u32::from(sy) + (y0 - rect.1) as u32;
        damage.add((u32::from(sx), row, (rect.2 - rect.0) as u32, (y1 - y0) as u32));
        true
    }
}
