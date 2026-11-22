// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the base's ink as the probe keeps it (RFC-0121) — computed by `crate::ink` from the
//! retained layout after each relayout, or when a base box starts or stops moving, or with the
//! structure; carried by the next `OP_SURFACE_TREE` when it changed. Only a clear base declares
//! it (the desktop, the greeter: `page_base_alpha() == 0`); every other surface is the whole
//! base, as before the field.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the ink itself in `crate::ink` (host oracle); this glue on the QEMU lanes
//!   (`windowd: body ink (`).

use super::*;
use crate::ink::{ink_of, Ink, Placed};

impl DslApp {
    /// Whether a box of the base pass is animated off identity: it paints outside its layout
    /// place (an animated node moves its whole subtree, which lies in the pass its node does —
    /// a band's content and a motion layer's subtree are their own passes). A node with no box
    /// counts as moving unless a motion layer holds it (an exiting panel's ghost is its layer's,
    /// never the base's): never a hole.
    fn base_moving(&self) -> bool {
        self.anim.moving().any(|node| match self.layout.boxes.iter().find(|b| b.node_id == node) {
            Some(b) => !self.in_band(b) && self.motion_of(b.node_id).is_none(),
            None => self.motion_of(node).is_none(),
        })
    }

    /// The base's ink now: computed for a clear base, the whole base otherwise.
    fn base_ink(&mut self) -> Ink {
        if self.page_base_alpha() != 0 {
            return Ink::WHOLE;
        }
        let mut views = core::mem::take(&mut self.view_scratch);
        self.base_views(&mut views);
        let placed = |b: &nexus_layout::LayoutBox| {
            if self.in_band(b) || self.motion_of(b.node_id).is_some() {
                return Placed::Elsewhere;
            }
            match b.scroll_root.and_then(|r| views.iter().find(|v| v.root == r)) {
                Some(v) => Placed::InView(v.clip),
                None => Placed::Here,
            }
        };
        let size = (self.w, self.h);
        let ink = ink_of(&self.layout.boxes, &self.texts, size, self.base_moving(), placed);
        self.view_scratch = views;
        ink
    }

    /// Before a present: the ink again after a relayout, a change of the structure, or a base
    /// box that started or stopped moving — and the structure owed when it differs from the one
    /// sent (RFC-0121: sent with the structure, atomic with the pixels it names).
    pub(super) fn sync_ink(&mut self) {
        if self.ink_stale || self.tree_dirty || self.base_moving() != self.ink_moving {
            self.ink = self.base_ink();
            self.ink_moving = self.base_moving();
            self.ink_stale = false;
        }
        if !self.ink.sent_in(&self.sent_tree) {
            self.tree_dirty = true;
        }
    }
}
