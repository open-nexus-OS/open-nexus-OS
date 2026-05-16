// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use alloc::vec::Vec;
use nexus_layout_types::{Direction, EdgeInsets, FxPx, LayoutNode, MeasureText, Rect, VisualStyle};
use crate::error::LayoutError;

const DEFAULT_MAX_NODES: usize = 4096;
const DEFAULT_MAX_DEPTH: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutBox { pub node_id: usize, pub rect: Rect, pub z_index: i16, pub visual: VisualStyle }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutResult { pub boxes: Vec<LayoutBox>, pub content_height: FxPx }
pub struct LayoutEngine { max_nodes: usize, max_depth: usize }

impl LayoutEngine {
    pub fn new() -> Self { LayoutEngine { max_nodes: DEFAULT_MAX_NODES, max_depth: DEFAULT_MAX_DEPTH } }
    pub fn with_limits(max_nodes: usize, max_depth: usize) -> Self { LayoutEngine { max_nodes, max_depth } }

    pub fn layout(&self, root: &LayoutNode, available_width: FxPx, measure: &dyn MeasureText,
    ) -> Result<LayoutResult, LayoutError> {
        let mut node_count = 0;
        let mut boxes = Vec::new();
        self.layout_node(root, FxPx::ZERO, FxPx::ZERO, available_width, 0, measure, &mut node_count, &mut boxes)?;
        let content_height = boxes.iter().map(|b| b.rect.y + b.rect.height).max().unwrap_or(FxPx::ZERO);
        Ok(LayoutResult { boxes, content_height })
    }

    fn layout_node(&self, node: &LayoutNode, x: FxPx, y: FxPx, available_width: FxPx, depth: usize,
        measure: &dyn MeasureText, node_count: &mut usize, boxes: &mut Vec<LayoutBox>,
    ) -> Result<FxPx, LayoutError> {
        if depth > self.max_depth { return Err(LayoutError::TooDeep { max: self.max_depth, actual: depth }); }
        *node_count += 1;
        if *node_count > self.max_nodes { return Err(LayoutError::TooManyNodes { max: self.max_nodes, actual: *node_count }); }
        let node_id = *node_count;
        match node {
            LayoutNode::Stack(stack, style, children) =>
                self.layout_stack(stack, style, children, x, y, available_width, depth, measure, node_count, boxes),
            LayoutNode::Grid(grid, style, children) =>
                self.layout_grid(grid, style, children, x, y, available_width, depth, measure, node_count, boxes),
            LayoutNode::Spacer(_) => Ok(FxPx::ZERO),
            LayoutNode::Text(text, style) => {
                let handle = measure.prepare(text.content.as_str());
                let w = measure.measure_width(handle).min(available_width);
                boxes.push(LayoutBox { node_id, rect: Rect::new(x, y, w, FxPx::new(20)), z_index: 0, visual: style.clone() });
                Ok(FxPx::new(20))
            }
        }
    }

    fn layout_stack(&self, stack: &nexus_layout_types::Stack, style: &VisualStyle,
        children: &[LayoutNode], x: FxPx, y: FxPx, available_width: FxPx, depth: usize,
        measure: &dyn MeasureText, node_count: &mut usize, boxes: &mut Vec<LayoutBox>,
    ) -> Result<FxPx, LayoutError> {
        let padding = stack.padding;
        let content_x = x + padding.left;
        let content_width = available_width.saturating_sub(padding.left + padding.right);
        let node_id = *node_count;
        boxes.push(LayoutBox { node_id, rect: Rect::new(x, y, available_width, FxPx::ZERO), z_index: 0, visual: style.clone() });
        let is_vertical = stack.direction.is_vertical();
        let mut cursor = if is_vertical { y + padding.top } else { content_x };
        let mut max_cross = FxPx::ZERO;
        for child in children {
            let child_x = if is_vertical { content_x } else { cursor };
            let child_y = if is_vertical { cursor } else { y + padding.top };
            let ch = self.layout_node(child, child_x, child_y, content_width, depth + 1, measure, node_count, boxes)?;
            if is_vertical { cursor = cursor + ch + stack.gap; }
            else {
                let cw = boxes.last().map(|b| b.rect.width).unwrap_or(FxPx::ZERO);
                cursor = cursor + cw + stack.gap;
                max_cross = max_cross.max(ch);
            }
        }
        let container_height = if is_vertical {
            let end = if cursor > y + padding.top { cursor - stack.gap } else { cursor };
            end.saturating_sub(y + padding.top) + padding.top + padding.bottom
        } else { max_cross + padding.top + padding.bottom };
        if let Some(cb) = boxes.iter_mut().find(|b| b.node_id == node_id) { cb.rect.height = container_height; }
        Ok(container_height)
    }

    fn layout_grid(&self, grid: &nexus_layout_types::Grid, style: &VisualStyle,
        children: &[LayoutNode], x: FxPx, y: FxPx, available_width: FxPx, depth: usize,
        measure: &dyn MeasureText, node_count: &mut usize, boxes: &mut Vec<LayoutBox>,
    ) -> Result<FxPx, LayoutError> {
        let padding = grid.padding;
        let content_width = available_width.saturating_sub(padding.left + padding.right);
        let n_cols = grid.columns.len().max(1);
        let total_fr: u32 = grid.columns.iter().map(|f| f.0).sum();
        if total_fr == 0 { return Err(LayoutError::DivByZero); }
        let gap_total = grid.gap * (n_cols as i32 - 1).max(0) as i32;
        let usable = content_width.saturating_sub(gap_total);
        let mut col_widths: Vec<FxPx> = grid.columns.iter().map(|f| {
            FxPx::new(usable.0 * f.0 as i32 / total_fr as i32)
        }).collect();
        let sum_w: i32 = col_widths.iter().map(|w| w.0).sum();
        let mut rem = usable.0 - sum_w;
        for w in col_widths.iter_mut() { if rem <= 0 { break; } w.0 += 1; rem -= 1; }
        let container_id = *node_count;
        boxes.push(LayoutBox { node_id: container_id, rect: Rect::new(x, y, available_width, FxPx::ZERO), z_index: 0, visual: style.clone() });
        let row_gap = grid.row_gap.unwrap_or(grid.gap);
        let content_x = x + padding.left;
        let content_y_base = y + padding.top;
        let mut row_y = content_y_base;
        let mut total_height = FxPx::ZERO;
        let mut child_idx = 0;
        while child_idx < children.len() {
            let mut row_height = FxPx::ZERO;
            let mut col_x = content_x;
            for col in 0..n_cols {
                if child_idx >= children.len() { break; }
                let child = &children[child_idx];
                let ch = self.layout_node(child, col_x, row_y, col_widths[col], depth + 1, measure, node_count, boxes)?;
                row_height = row_height.max(ch);
                col_x = col_x + col_widths[col] + grid.gap;
                child_idx += 1;
            }
            total_height = total_height + row_height;
            row_y = row_y + row_height + row_gap;
        }
        let container_height = total_height + padding.top + padding.bottom;
        if let Some(cb) = boxes.iter_mut().find(|b| b.node_id == container_id) { cb.rect.height = container_height; }
        Ok(container_height)
    }
}
impl Default for LayoutEngine { fn default() -> Self { Self::new() } }
