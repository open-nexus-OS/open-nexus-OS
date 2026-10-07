// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: grid placement and measurement of the layout engine (TASK-0058 / RFC-0057),
//! moved out of `engine.rs` (structure ratchet) with one rule added (TASK-0067B): a cell
//! that declares `.grow(1)` fills its track — the grid twin of the flex rule for
//! flex-grown row children. Cells without it keep their content size, so every existing
//! grid (launcher, workspace, Control Center) lays out unchanged.
//! OWNERS: @ui
//! STATUS: Done
//! API_STABILITY: Unstable
//! TEST_COVERAGE: engine_tests (grid cases incl. the grow rule)
//! ADR: docs/rfcs/RFC-0057-ui-v3a-layout-engine-pretext-contract.md

use crate::boxes::LayoutBox;
use crate::constraints::child_constraints;
use crate::engine::{LayoutConstraints, LayoutEngine, NodeSize};
use crate::error::LayoutError;
use crate::geometry::{clamp_height, clamp_to_max_height, clamp_width, intersect_clip};
use alloc::vec::Vec;
use nexus_layout_types::{FxPx, LayoutNode, MeasureText, Overflow, Rect, VisualStyle};

impl LayoutEngine {
    pub(crate) fn place_grid(
        &self,
        node_id: usize,
        grid: &nexus_layout_types::Grid,
        style: &VisualStyle,
        children: &[LayoutNode],
        x: FxPx,
        y: FxPx,
        constraints: LayoutConstraints,
        depth: usize,
        parent_clip: Option<Rect>,
        in_glass: bool,
        scroll_offset: (FxPx, FxPx),
        measure: &dyn MeasureText,
        node_count: &mut usize,
        boxes: &mut Vec<LayoutBox>,
    ) -> Result<NodeSize, LayoutError> {
        let measured = self.measure_grid(grid, children, constraints, depth, measure)?;
        let width = measured.width;
        let height = measured.height;
        let container_index = boxes.len();
        let is_overflow_hidden = matches!(grid.overflow, Overflow::Hidden | Overflow::Scroll(_));
        let container_scroll =
            if is_overflow_hidden { scroll_offset } else { (FxPx::ZERO, FxPx::ZERO) };
        let content_width = width.saturating_sub(grid.padding.horizontal());
        let content_height = height.saturating_sub(grid.padding.vertical());
        let container_clip = if is_overflow_hidden {
            let own = Rect::new(
                x + grid.padding.left,
                y + grid.padding.top,
                content_width,
                content_height,
            );
            intersect_clip(Some(own), parent_clip)
        } else {
            parent_clip
        };
        boxes.push(LayoutBox {
            node_id,
            id: grid.id,
            rect: Rect::new(x, y, width, height),
            z_index: grid.item.z_index,
            hit_slop: grid.item.hit_slop,
            visual: style.clone(),
            clip_rect: parent_clip,
            scroll_offset: container_scroll,
            overflow: grid.overflow,
            glass_nested: in_glass,
            text_px: None,
        });
        // Descendants nest under this container's glass, if any.
        let in_glass =
            in_glass || matches!(style.material, nexus_layout_types::SurfaceMaterial::Glass(_));
        let padding = grid.padding;
        let n_cols = grid.columns.len().max(1);
        let total_fr: u32 = grid.columns.iter().map(|f| f.0).sum();
        if total_fr == 0 {
            return Err(LayoutError::DivByZero);
        }
        let gap_total = grid.gap * (n_cols as i32 - 1).max(0);
        let usable = content_width.saturating_sub(gap_total);
        let mut col_widths: Vec<FxPx> = grid
            .columns
            .iter()
            .map(|f| FxPx::new(usable.0 * f.0 as i32 / total_fr as i32))
            .collect();
        let sum_w: i32 = col_widths.iter().map(|w| w.0).sum();
        let mut rem = usable.0 - sum_w;
        for width in &mut col_widths {
            if rem <= 0 {
                break;
            }
            width.0 += 1;
            rem -= 1;
        }
        let row_gap = grid.row_gap.unwrap_or(grid.gap);
        let mut row_y = y + padding.top - container_scroll.1;
        let mut total_height = FxPx::ZERO;
        let mut child_idx = 0usize;
        while child_idx < children.len() {
            let row_start = child_idx;
            let mut row_height = FxPx::ZERO;
            for col_width in col_widths.iter().take(n_cols) {
                if child_idx >= children.len() {
                    break;
                }
                let child = &children[child_idx];
                let item = child.item();
                let child_width = col_width.saturating_sub(item.margin.horizontal());
                let measured = self.measure_node(
                    child,
                    child_constraints(
                        LayoutConstraints::new(content_width, Some(content_height)),
                        *item,
                        child_width,
                        None,
                    ),
                    depth + 1,
                    measure,
                )?;
                row_height = row_height.max(measured.height + item.margin.vertical());
                child_idx += 1;
            }
            let mut col_x = x + padding.left - scroll_offset.0;
            for (col, col_width) in col_widths.iter().enumerate().take(n_cols) {
                let index = row_start + col;
                if index >= child_idx {
                    break;
                }
                let child = &children[index];
                let item = child.item();
                let child_width = col_width.saturating_sub(item.margin.horizontal());
                // A cell that declares `.grow(1)` FILLS its track (the row rule for
                // flex-grown children, `row_child_constraints`); every other cell keeps
                // its content size, so existing grids lay out exactly as before.
                let cell = if item.flex_grow > 0 {
                    LayoutConstraints::definite(child_width, None).with_text_px(constraints.text_px)
                } else {
                    child_constraints(
                        LayoutConstraints::new(content_width, Some(content_height)),
                        *item,
                        child_width,
                        None,
                    )
                };
                self.place_node(
                    child,
                    col_x + item.margin.left,
                    row_y + item.margin.top,
                    cell,
                    depth + 1,
                    container_clip,
                    in_glass,
                    container_scroll,
                    measure,
                    node_count,
                    boxes,
                )?;
                col_x += *col_width + grid.gap;
            }
            total_height += row_height;
            row_y += row_height + row_gap;
        }
        // Fix up the container clip rect height for overflow:hidden grids
        if is_overflow_hidden {
            let own = Rect::new(
                x + grid.padding.left,
                y + grid.padding.top,
                content_width,
                content_height,
            );
            boxes[container_index].clip_rect = intersect_clip(Some(own), parent_clip);
        }
        Ok(NodeSize { width, height })
    }

    pub(crate) fn measure_grid(
        &self,
        grid: &nexus_layout_types::Grid,
        children: &[LayoutNode],
        constraints: LayoutConstraints,
        depth: usize,
        measure: &dyn MeasureText,
    ) -> Result<NodeSize, LayoutError> {
        let width = clamp_width(
            constraints.max_width,
            grid.min_width.or(grid.item.min_width),
            grid.max_width.or(grid.item.max_width),
        );
        let content_width = width.saturating_sub(grid.padding.horizontal());
        let n_cols = grid.columns.len().max(1);
        let total_fr: u32 = grid.columns.iter().map(|f| f.0).sum();
        if total_fr == 0 {
            return Err(LayoutError::DivByZero);
        }
        let gap_total = grid.gap * (n_cols as i32 - 1).max(0);
        let usable = content_width.saturating_sub(gap_total);
        let col_widths: Vec<FxPx> = grid
            .columns
            .iter()
            .map(|f| FxPx::new(usable.0 * f.0 as i32 / total_fr as i32))
            .collect();
        let row_gap = grid.row_gap.unwrap_or(grid.gap);
        let mut total_height = FxPx::ZERO;
        let mut child_idx = 0usize;
        while child_idx < children.len() {
            let mut row_height = FxPx::ZERO;
            for col_width in col_widths.iter().take(n_cols) {
                if child_idx >= children.len() {
                    break;
                }
                let child = &children[child_idx];
                let item = child.item();
                let width = col_width.saturating_sub(item.margin.horizontal());
                let measured = self.measure_node(
                    child,
                    child_constraints(constraints, *item, width, None),
                    depth + 1,
                    measure,
                )?;
                row_height = row_height.max(measured.height + item.margin.vertical());
                child_idx += 1;
            }
            total_height += row_height;
            if child_idx < children.len() {
                total_height += row_gap;
            }
        }
        Ok(NodeSize {
            width,
            height: clamp_to_max_height(
                clamp_height(
                    total_height + grid.padding.vertical(),
                    grid.min_height,
                    grid.max_height,
                ),
                constraints.max_height,
            ),
        })
    }
}
