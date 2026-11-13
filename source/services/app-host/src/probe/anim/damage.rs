// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The damage an animated node leaves (TASK-0351 P15): its rows AND its columns.
//!
//! A `.transition(slideDown)` moves a 330-wide panel down the desktop. The row painter repaints
//! the rows it crosses whole — a row is the unit of the retained raster — but what CHANGED in
//! them is the panel's rectangle, and that is what the present names (`band_math::Cols`): the
//! compositor then redraws, and re-blurs the glass over, 330 columns a frame instead of 1920.
//! The iOS rule, in our terms: a transition changes the difference between A and B, not
//! everything the difference's rows hold. Split out of `anim.rs` (structure ratchet).

use crate::motion_math::moved_extent;

impl super::super::DslApp {
    /// Grow `span` by `node_id`'s CURRENT painted row extent — what moves with it and paints
    /// (`motion_math::moved_extent`; its layout box when that cannot be told) transformed by
    /// the node's active `NodeAnim` (scale can overpaint the box; translate moves it), ±1 row
    /// rounding pad, clamped to the layout (a scroll layer's content runs below the surface) —
    /// and claim its columns the same way. A node without a layout box (mid-relayout) falls
    /// back to all of it, rows and columns — correctness over thrift.
    pub(super) fn grow_anim_span(&mut self, node_id: usize, span: &mut Option<(i32, i32)>) {
        let bottom = self.layout_bottom();
        let (x0, x1, y0, y1) = match self.layout.boxes.iter().find(|b| b.node_id == node_id) {
            Some(b) => {
                // What moves with it and paints; its own rectangle when that cannot be told.
                let has_text = |id| self.has_text(id);
                let (x, y, w, h) = match moved_extent(&self.layout.boxes, node_id, has_text) {
                    Some((l, t, r, bo)) => (l, t, r - l, bo - t),
                    None => (b.rect.x.0, b.rect.y.0, b.rect.width.0, b.rect.height.0),
                };
                // About the point the painter scales it (`expand_node_anims`): its centre, or
                // the corner a menu grows from.
                let (nx, ny, nw, nh) = match self.anim.anims.iter().find(|a| a.node_id == node_id) {
                    Some(a) if !a.is_identity() => match self.enter_anchor(node_id) {
                        Some((cx, cy)) => a.anchored_at(cx, cy).transform_rect(x, y, w, h),
                        None => a.transform_rect(x, y, w, h),
                    },
                    _ => (x, y, w, h),
                };
                (nx - 1, nx + nw + 1, ny - 1, ny + nh + 1)
            }
            None => {
                self.damage_cols.full();
                (0, self.w as i32, 0, bottom)
            }
        };
        let (y0, y1) = (y0.max(0), y1.min(bottom));
        if y0 >= y1 {
            return;
        }
        self.damage_cols.within(x0, x1);
        *span = Some(match *span {
            Some((s0, s1)) => (s0.min(y0), s1.max(y1)),
            None => (y0, y1),
        });
    }
}
