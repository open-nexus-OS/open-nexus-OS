// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: windowd compositor runtime — generic wheel-scroll routing + active proof-layout hot-path accessors.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests (behavior covered via windowd QEMU smoke + host integration)
//!
//! Split out of `runtime/mod.rs` (TASK-0063 modularization). A child module of
//! `runtime`, so these `impl DisplayServerRuntime` methods read the runtime's
//! private fields directly; previously-private methods are widened to
//! `pub(super)` so the parent and sibling submodules can still call them.

use super::*;

impl DisplayServerRuntime {
    pub(super) fn handle_scroll_input(&mut self) {
        if !self.scroll_marker_emitted {
            let _ = debug_println(crate::markers::SCROLL_ON_MARKER);
            self.scroll_marker_emitted = true;
        }

        let wheel_down_visible = self.state.wheel_down_visible;
        // Compute content height before mutable borrow of proof_layouts
        let content_h = filter_list_content_height(&self.filtered_words);

        let mut scroll_damage = None;
        if let Some(layout) = self.active_proof_layout_mut() {
            // Find the filter_list container
            let container_id =
                layout.boxes.iter().find(|b| b.id == Some("filter_list")).map(|b| b.node_id);

            if let Some(id) = container_id {
                let viewport_h = layout
                    .boxes
                    .iter()
                    .find(|b| b.node_id == id)
                    .map(|b| {
                        FxPx::new(
                            filter_list_viewport_height(b.rect.height.as_u32().unwrap_or(0)) as i32
                        )
                    })
                    .unwrap_or(FxPx::ZERO);
                let current_offset = layout
                    .boxes
                    .iter()
                    .find(|b| b.node_id == id)
                    .map(|b| b.scroll_offset)
                    .unwrap_or((FxPx::ZERO, FxPx::ZERO));

                let dy = if wheel_down_visible { FxPx::new(20) } else { FxPx::new(-20) };
                let max_scroll = FxPx::new((content_h as i32).saturating_sub(viewport_h.0).max(0));
                let new_offset_y = (current_offset.1 + dy).clamp(FxPx::ZERO, max_scroll);
                let new_offset = (current_offset.0, new_offset_y);
                scroll_damage = Some(layout.reposition_scroll(id, new_offset));
            }
        }
        if let Some(damage) = scroll_damage {
            self.refresh_active_proof_hot_path();
            // G3: the proof panel is a GPU layer — re-render its content surface so
            // the scrolled list is reflected (G3c will make this a GPU offset).
            self.proof_surface_dirty = true;
            for rect in damage.rects.into_iter().flatten() {
                let x = SCENE_ORIGIN_X.saturating_add(rect.x.as_u32().unwrap_or(0));
                let y = SCENE_ORIGIN_Y.saturating_add(rect.y.as_u32().unwrap_or(0));
                let w = rect.width.as_u32().unwrap_or(0);
                let h = rect.height.as_u32().unwrap_or(0);
                if w > 0 && h > 0 {
                    self.queue_dirty_rect(DamageRect { x, y, width: w, height: h });
                }
            }
            if !damage.is_empty() {
                let _ = debug_println(crate::markers::LIVE_SCROLL_OK_MARKER);
                self.live_scroll_marker_emitted = true;
            }
        }
    }

    pub(super) fn current_filter_text(&self) -> &'static str {
        LIVE_FILTER_VARIANTS[self.active_filter_idx]
    }

    pub(super) fn active_proof_layout(&self) -> Option<&LayoutResult> {
        self.proof_layouts.as_ref()?.get(self.active_filter_idx)
    }

    pub(super) fn active_proof_layout_mut(&mut self) -> Option<&mut LayoutResult> {
        self.proof_layouts.as_mut()?.get_mut(self.active_filter_idx)
    }

    pub(super) fn active_proof_layout_index(&self) -> Option<&LayoutHotPathIndex> {
        self.proof_layout_index.as_ref()
    }

    pub(super) fn refresh_active_proof_hot_path(&mut self) {
        let Some(new_index) = self.active_proof_layout().map(|layout| {
            LayoutHotPathIndex::build(
                layout,
                SCENE_ORIGIN_X,
                SCENE_ORIGIN_Y,
                self.mode.width,
                self.mode.height,
            )
        }) else {
            return;
        };
        self.proof_layout_index = Some(new_index);
    }
}
