// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The damage an animated node leaves (TASK-0351 P15, TASK-0354 P8): its rectangle.
//!
//! A `.transition(slideDown)` moves a 330-wide panel down the desktop. What CHANGED is the
//! panel's rectangle where it was and where it is, and that is what is painted and what the
//! present names (`band_math::Pending`): the compositor then redraws, and re-blurs the glass
//! over, 330 columns a frame instead of 1920. The iOS rule, in our terms: a transition changes
//! the difference between A and B, not everything the difference's rows hold. Split out of
//! `anim.rs` (structure ratchet).

use crate::motion_math::moved_extent;
use crate::pass_plan::GLYPH_OVERHANG;

impl super::super::DslApp {
    /// `node_id`'s CURRENT painted rectangle, pending — what moves with it and paints
    /// (`motion_math::moved_extent`; its layout box when that cannot be told) transformed by
    /// the node's active `NodeAnim` (scale can overpaint the box; translate moves it), a glyph's
    /// overhang around it, cut to the layout (a scroll layer's content runs below the surface).
    /// A node without a layout box (mid-relayout) is every row — correctness over thrift.
    pub(super) fn damage_anim(&mut self, node_id: usize) {
        let Some(b) = self.layout.boxes.iter().find(|b| b.node_id == node_id) else {
            self.pending.rows((0, self.layout_bottom()), self.w);
            return;
        };
        // What moves with it and paints; its own rectangle when that cannot be told.
        let has_text = |id| self.has_text(id);
        let (x, y, w, h) = match moved_extent(&self.layout.boxes, node_id, has_text) {
            Some((l, t, r, bo)) => (l, t, r - l, bo - t),
            None => (b.rect.x.0, b.rect.y.0, b.rect.width.0, b.rect.height.0),
        };
        // About the point the painter scales it (`expand_node_anims`): its centre, or the
        // corner a menu grows from.
        let (nx, ny, nw, nh) = match self.anim.anims.iter().find(|a| a.node_id == node_id) {
            Some(a) if !a.is_identity() => match self.enter_anchor(node_id) {
                Some((cx, cy)) => a.anchored_at(cx, cy).transform_rect(x, y, w, h),
                None => a.transform_rect(x, y, w, h),
            },
            _ => (x, y, w, h),
        };
        let o = GLYPH_OVERHANG;
        self.damage_rect((nx - o, ny - o, nx + nw + o, ny + nh + o));
    }

    /// Every node that paints at a transform, pending where it paints now: taken before a
    /// relayout and after it — the layout diff compares layout boxes, and such a node's pixels
    /// are not at its box.
    pub(in crate::probe) fn damage_animated(&mut self) {
        for k in 0..self.anim.anims.len() {
            let (node_id, moved) = (self.anim.anims[k].node_id, !self.anim.anims[k].is_identity());
            if moved {
                self.damage_anim(node_id);
            }
        }
    }
}
