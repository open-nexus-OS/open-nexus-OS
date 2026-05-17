// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use alloc::vec::Vec;
use crate::error::LayoutError;
use nexus_layout_types::{
    Align, Direction, EdgeInsets, FlexItem, FxPx, Justify, LayoutNode, MeasureText, Rect, VisualStyle,
};

const DEFAULT_MAX_NODES: usize = 4096;
const DEFAULT_MAX_DEPTH: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NodeSize {
    width: FxPx,
    height: FxPx,
}

impl NodeSize {
    const ZERO: Self = Self { width: FxPx::ZERO, height: FxPx::ZERO };
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutBox {
    pub node_id: usize,
    pub id: Option<&'static str>,
    pub rect: Rect,
    pub z_index: i16,
    pub visual: VisualStyle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutResult {
    pub boxes: Vec<LayoutBox>,
    pub content_height: FxPx,
}

pub struct LayoutEngine {
    max_nodes: usize,
    max_depth: usize,
}

impl LayoutEngine {
    pub fn new() -> Self {
        Self { max_nodes: DEFAULT_MAX_NODES, max_depth: DEFAULT_MAX_DEPTH }
    }

    pub fn with_limits(max_nodes: usize, max_depth: usize) -> Self {
        Self { max_nodes, max_depth }
    }

    pub fn layout(
        &self,
        root: &LayoutNode,
        available_width: FxPx,
        measure: &dyn MeasureText,
    ) -> Result<LayoutResult, LayoutError> {
        let mut node_count = 0;
        let mut boxes = Vec::new();
        self.place_node(
            root,
            FxPx::ZERO,
            FxPx::ZERO,
            available_width,
            0,
            measure,
            &mut node_count,
            &mut boxes,
        )?;
        let content_height =
            boxes.iter().map(|b| b.rect.y + b.rect.height).max().unwrap_or(FxPx::ZERO);
        Ok(LayoutResult { boxes, content_height })
    }

    fn place_node(
        &self,
        node: &LayoutNode,
        x: FxPx,
        y: FxPx,
        available_width: FxPx,
        depth: usize,
        measure: &dyn MeasureText,
        node_count: &mut usize,
        boxes: &mut Vec<LayoutBox>,
    ) -> Result<NodeSize, LayoutError> {
        if depth > self.max_depth {
            return Err(LayoutError::TooDeep { max: self.max_depth, actual: depth });
        }
        *node_count += 1;
        if *node_count > self.max_nodes {
            return Err(LayoutError::TooManyNodes { max: self.max_nodes, actual: *node_count });
        }
        let node_id = *node_count;
        match node {
            LayoutNode::Stack(stack, style, children) => {
                self.place_stack(node_id, stack, style, children, x, y, available_width, depth, measure, node_count, boxes)
            }
            LayoutNode::Grid(grid, style, children) => {
                self.place_grid(node_id, grid, style, children, x, y, available_width, depth, measure, node_count, boxes)
            }
            LayoutNode::Spacer(spacer) => {
                let main = spacer.min_size.unwrap_or(FxPx::ZERO);
                boxes.push(LayoutBox {
                    node_id,
                    id: spacer.id,
                    rect: Rect::new(x, y, main, FxPx::ZERO),
                    z_index: spacer.item.z_index,
                    visual: VisualStyle::default(),
                });
                Ok(NodeSize { width: main, height: FxPx::ZERO })
            }
            LayoutNode::Text(text, style) => self.place_text(node_id, text, style, x, y, available_width, measure, boxes),
        }
    }

    fn place_text(
        &self,
        node_id: usize,
        text: &nexus_layout_types::TextNode,
        style: &VisualStyle,
        x: FxPx,
        y: FxPx,
        available_width: FxPx,
        measure: &dyn MeasureText,
        boxes: &mut Vec<LayoutBox>,
    ) -> Result<NodeSize, LayoutError> {
        let width_limit = clamp_width(
            available_width,
            text.min_width.or(text.item.min_width),
            text.max_width.or(text.item.max_width),
        );
        let handle = measure.prepare(&text.content, &text.style);
        let natural_width = measure.measure_width(&handle);
        let target_width = natural_width.min(width_limit);
        let lines = measure.layout_lines(&handle, target_width, text.max_lines);
        let line_width = lines
            .lines
            .iter()
            .map(|line| line.width)
            .max()
            .unwrap_or_else(|| natural_width.min(width_limit));
        let width = clamp_width(
            line_width.max(target_width.min(width_limit)),
            text.min_width.or(text.item.min_width),
            text.max_width.or(text.item.max_width),
        )
        .min(width_limit.max(line_width));
        let height =
            lines.lines.iter().fold(FxPx::ZERO, |acc, line| acc + line.height).max(FxPx::new(1));
        boxes.push(LayoutBox {
            node_id,
            id: text.id,
            rect: Rect::new(x, y, width, height),
            z_index: text.item.z_index,
            visual: style.clone(),
        });
        Ok(NodeSize { width, height })
    }

    fn place_stack(
        &self,
        node_id: usize,
        stack: &nexus_layout_types::Stack,
        style: &VisualStyle,
        children: &[LayoutNode],
        x: FxPx,
        y: FxPx,
        available_width: FxPx,
        depth: usize,
        measure: &dyn MeasureText,
        node_count: &mut usize,
        boxes: &mut Vec<LayoutBox>,
    ) -> Result<NodeSize, LayoutError> {
        let measured = self.measure_stack(stack, children, available_width, depth, measure)?;
        let width = clamp_width(measured.width, stack.min_width.or(stack.item.min_width), stack.max_width.or(stack.item.max_width)).min(available_width.max(measured.width));
        let container_index = boxes.len();
        boxes.push(LayoutBox {
            node_id,
            id: stack.id,
            rect: Rect::new(x, y, width, FxPx::ZERO),
            z_index: stack.item.z_index,
            visual: style.clone(),
        });
        let padding = stack.padding;
        let content_x = x + padding.left;
        let content_y = y + padding.top;
        let content_width = width.saturating_sub(padding.horizontal());
        let size = if stack.direction.is_horizontal() {
            self.place_stack_row(
                stack,
                children,
                content_x,
                content_y,
                content_width,
                depth,
                measure,
                node_count,
                boxes,
            )?
        } else {
            self.place_stack_column(
                stack,
                children,
                content_x,
                content_y,
                content_width,
                depth,
                measure,
                node_count,
                boxes,
            )?
        };
        let height = clamp_height(
            size.height + padding.vertical(),
            stack.min_height,
            stack.max_height,
        );
        boxes[container_index].rect.height = height;
        Ok(NodeSize { width, height })
    }

    fn place_stack_row(
        &self,
        stack: &nexus_layout_types::Stack,
        children: &[LayoutNode],
        content_x: FxPx,
        content_y: FxPx,
        content_width: FxPx,
        depth: usize,
        measure: &dyn MeasureText,
        node_count: &mut usize,
        boxes: &mut Vec<LayoutBox>,
    ) -> Result<NodeSize, LayoutError> {
        let mut in_flow: Vec<(usize, &LayoutNode, FlexItem, NodeSize, FxPx)> = Vec::new();
        let mut absolute: Vec<(&LayoutNode, FlexItem, NodeSize)> = Vec::new();
        let mut fixed_main = FxPx::ZERO;
        let mut total_grow = 0u32;
        for (index, child) in children.iter().enumerate() {
            let item = *child.item();
            let measured = self.measure_node(child, content_width, depth + 1, measure)?;
            if item.position == nexus_layout_types::Position::Absolute {
                absolute.push((child, item, measured));
                continue;
            }
            let base_main = measured.width + item.margin.horizontal();
            fixed_main += base_main;
            total_grow += item.flex_grow;
            in_flow.push((index, child, item, measured, base_main));
        }
        let gap_count = in_flow.len().saturating_sub(1) as i32;
        let fixed_with_gap = fixed_main + stack.gap * gap_count;
        let free_space = content_width.saturating_sub(fixed_with_gap);
        let mut allocations: Vec<FxPx> = Vec::with_capacity(in_flow.len());
        for (_, _, item, _, base_main) in &in_flow {
            let extra = if total_grow > 0 && item.flex_grow > 0 {
                FxPx::new((free_space.0 * item.flex_grow as i32) / total_grow as i32)
            } else {
                FxPx::ZERO
            };
            allocations.push(*base_main + extra);
        }
        let mut row_height = FxPx::ZERO;
        let mut used_main = FxPx::ZERO;
        for ((_, child, item, _, _), allocation) in in_flow.iter().zip(allocations.iter()) {
            let child_width = allocation.saturating_sub(item.margin.horizontal());
            let measured = self.measure_node(child, child_width, depth + 1, measure)?;
            row_height = row_height.max(measured.height + item.margin.vertical());
            used_main += *allocation;
        }
        if !in_flow.is_empty() {
            used_main += stack.gap * gap_count;
        }
        let (mut cursor, extra_gap) = justify_offsets(stack.justify, content_width.saturating_sub(used_main), in_flow.len(), stack.gap);
        cursor += content_x;
        for ((_, child, item, _, _), allocation) in in_flow.iter().zip(allocations.iter()) {
            let child_width = allocation.saturating_sub(item.margin.horizontal());
            let measured = self.measure_node(child, child_width, depth + 1, measure)?;
            let align = item.align_self.unwrap_or(stack.align);
            let child_x = cursor + item.margin.left;
            let cross_space = row_height.saturating_sub(measured.height + item.margin.vertical());
            let child_y = content_y
                + item.margin.top
                + align_offset(align, cross_space);
            self.place_node(
                child,
                child_x,
                child_y,
                child_width,
                depth + 1,
                measure,
                node_count,
                boxes,
            )?;
            cursor += *allocation + extra_gap;
        }
        for (child, item, _) in absolute {
            let child_x = content_x + item.margin.left;
            let child_y = content_y + item.margin.top;
            let child_width = content_width.saturating_sub(item.margin.horizontal());
            self.place_node(
                child,
                child_x,
                child_y,
                child_width,
                depth + 1,
                measure,
                node_count,
                boxes,
            )?;
        }
        Ok(NodeSize { width: content_width, height: row_height })
    }

    fn place_stack_column(
        &self,
        stack: &nexus_layout_types::Stack,
        children: &[LayoutNode],
        content_x: FxPx,
        content_y: FxPx,
        content_width: FxPx,
        depth: usize,
        measure: &dyn MeasureText,
        node_count: &mut usize,
        boxes: &mut Vec<LayoutBox>,
    ) -> Result<NodeSize, LayoutError> {
        let mut in_flow: Vec<(&LayoutNode, FlexItem, NodeSize, FxPx)> = Vec::new();
        let mut absolute: Vec<(&LayoutNode, FlexItem)> = Vec::new();
        let mut fixed_main = FxPx::ZERO;
        let mut max_cross = FxPx::ZERO;
        let mut total_grow = 0u32;
        for child in children {
            let item = *child.item();
            let child_width = content_width.saturating_sub(item.margin.horizontal());
            let measured = self.measure_node(child, child_width, depth + 1, measure)?;
            if item.position == nexus_layout_types::Position::Absolute {
                absolute.push((child, item));
                continue;
            }
            let base_main = measured.height + item.margin.vertical();
            max_cross = max_cross.max(measured.width + item.margin.horizontal());
            fixed_main += base_main;
            total_grow += item.flex_grow;
            in_flow.push((child, item, measured, base_main));
        }
        let gap_count = in_flow.len().saturating_sub(1) as i32;
        let fixed_with_gap = fixed_main + stack.gap * gap_count;
        let free_space = FxPx::ZERO.max(self.measure_stack(stack, children, content_width, depth + 1, measure)?.height.saturating_sub(stack.padding.vertical()).saturating_sub(fixed_with_gap));
        let mut allocations: Vec<FxPx> = Vec::with_capacity(in_flow.len());
        let mut used_main = FxPx::ZERO;
        for (_, item, _, base_main) in &in_flow {
            let extra = if total_grow > 0 && item.flex_grow > 0 {
                FxPx::new((free_space.0 * item.flex_grow as i32) / total_grow as i32)
            } else {
                FxPx::ZERO
            };
            let allocation = *base_main + extra;
            allocations.push(allocation);
            used_main += allocation;
        }
        if !in_flow.is_empty() {
            used_main += stack.gap * gap_count;
        }
        let (mut cursor, extra_gap) =
            justify_offsets(stack.justify, FxPx::ZERO.max(free_space.saturating_sub(used_main.saturating_sub(fixed_main))), in_flow.len(), stack.gap);
        cursor += content_y;
        let mut column_height = FxPx::ZERO;
        for ((child, item, measured, _), allocation) in in_flow.iter().zip(allocations.iter()) {
            let align = item.align_self.unwrap_or(stack.align);
            let child_width = if matches!(align, Align::Stretch) {
                content_width.saturating_sub(item.margin.horizontal())
            } else {
                measured.width
            };
            let cross_space = content_width.saturating_sub(child_width + item.margin.horizontal());
            let child_x = content_x
                + item.margin.left
                + align_offset(align, cross_space);
            let child_y = cursor + item.margin.top;
            self.place_node(
                child,
                child_x,
                child_y,
                child_width,
                depth + 1,
                measure,
                node_count,
                boxes,
            )?;
            cursor += *allocation + extra_gap;
            column_height = cursor - content_y - extra_gap;
            max_cross = max_cross.max(child_width + item.margin.horizontal());
        }
        for (child, item) in absolute {
            let child_x = content_x + item.margin.left;
            let child_y = content_y + item.margin.top;
            let child_width = content_width.saturating_sub(item.margin.horizontal());
            self.place_node(
                child,
                child_x,
                child_y,
                child_width,
                depth + 1,
                measure,
                node_count,
                boxes,
            )?;
        }
        Ok(NodeSize { width: max_cross, height: column_height.max(FxPx::ZERO) })
    }

    fn place_grid(
        &self,
        node_id: usize,
        grid: &nexus_layout_types::Grid,
        style: &VisualStyle,
        children: &[LayoutNode],
        x: FxPx,
        y: FxPx,
        available_width: FxPx,
        depth: usize,
        measure: &dyn MeasureText,
        node_count: &mut usize,
        boxes: &mut Vec<LayoutBox>,
    ) -> Result<NodeSize, LayoutError> {
        let width = clamp_width(
            available_width,
            grid.min_width.or(grid.item.min_width),
            grid.max_width.or(grid.item.max_width),
        );
        let container_index = boxes.len();
        boxes.push(LayoutBox {
            node_id,
            id: grid.id,
            rect: Rect::new(x, y, width, FxPx::ZERO),
            z_index: grid.item.z_index,
            visual: style.clone(),
        });
        let padding = grid.padding;
        let content_width = width.saturating_sub(padding.horizontal());
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
        let mut row_y = y + padding.top;
        let mut total_height = FxPx::ZERO;
        let mut child_idx = 0usize;
        while child_idx < children.len() {
            let row_start = child_idx;
            let mut row_height = FxPx::ZERO;
            for col in 0..n_cols {
                if child_idx >= children.len() {
                    break;
                }
                let child = &children[child_idx];
                let item = child.item();
                let child_width = col_widths[col].saturating_sub(item.margin.horizontal());
                let measured = self.measure_node(child, child_width, depth + 1, measure)?;
                row_height = row_height.max(measured.height + item.margin.vertical());
                child_idx += 1;
            }
            let mut col_x = x + padding.left;
            for col in 0..n_cols {
                let index = row_start + col;
                if index >= child_idx {
                    break;
                }
                let child = &children[index];
                let item = child.item();
                let child_width = col_widths[col].saturating_sub(item.margin.horizontal());
                self.place_node(
                    child,
                    col_x + item.margin.left,
                    row_y + item.margin.top,
                    child_width,
                    depth + 1,
                    measure,
                    node_count,
                    boxes,
                )?;
                col_x += col_widths[col] + grid.gap;
            }
            total_height += row_height;
            row_y += row_height + row_gap;
        }
        let height = clamp_height(total_height + padding.vertical(), grid.min_height, grid.max_height);
        boxes[container_index].rect.height = height;
        Ok(NodeSize { width, height })
    }

    fn measure_node(
        &self,
        node: &LayoutNode,
        available_width: FxPx,
        depth: usize,
        measure: &dyn MeasureText,
    ) -> Result<NodeSize, LayoutError> {
        if depth > self.max_depth {
            return Err(LayoutError::TooDeep { max: self.max_depth, actual: depth });
        }
        match node {
            LayoutNode::Stack(stack, _, children) => self.measure_stack(stack, children, available_width, depth, measure),
            LayoutNode::Grid(grid, _, children) => self.measure_grid(grid, children, available_width, depth, measure),
            LayoutNode::Spacer(spacer) => {
                let main = spacer.min_size.unwrap_or(FxPx::ZERO);
                Ok(NodeSize { width: main, height: FxPx::ZERO })
            }
            LayoutNode::Text(text, _) => self.measure_text(text, available_width, measure),
        }
    }

    fn measure_text(
        &self,
        text: &nexus_layout_types::TextNode,
        available_width: FxPx,
        measure: &dyn MeasureText,
    ) -> Result<NodeSize, LayoutError> {
        let width_limit = clamp_width(
            available_width,
            text.min_width.or(text.item.min_width),
            text.max_width.or(text.item.max_width),
        );
        let handle = measure.prepare(&text.content, &text.style);
        let natural_width = measure.measure_width(&handle);
        let target_width = natural_width.min(width_limit);
        let lines = measure.layout_lines(&handle, target_width, text.max_lines);
        let width = clamp_width(
            lines.lines.iter().map(|line| line.width).max().unwrap_or(target_width),
            text.min_width.or(text.item.min_width),
            text.max_width.or(text.item.max_width),
        )
        .min(width_limit.max(target_width));
        let height = lines.lines.iter().fold(FxPx::ZERO, |acc, line| acc + line.height).max(FxPx::new(1));
        Ok(NodeSize { width, height })
    }

    fn measure_stack(
        &self,
        stack: &nexus_layout_types::Stack,
        children: &[LayoutNode],
        available_width: FxPx,
        depth: usize,
        measure: &dyn MeasureText,
    ) -> Result<NodeSize, LayoutError> {
        let width_limit = clamp_width(
            available_width,
            stack.min_width.or(stack.item.min_width),
            stack.max_width.or(stack.item.max_width),
        );
        let content_width = width_limit.saturating_sub(stack.padding.horizontal());
        let mut main = FxPx::ZERO;
        let mut cross = FxPx::ZERO;
        let mut visible_children = 0usize;
        for child in children {
            if child.item().position == nexus_layout_types::Position::Absolute {
                continue;
            }
            let item = child.item();
            let child_width = content_width.saturating_sub(item.margin.horizontal());
            let measured = self.measure_node(child, child_width, depth + 1, measure)?;
            visible_children += 1;
            if stack.direction.is_horizontal() {
                main += measured.width + item.margin.horizontal();
                cross = cross.max(measured.height + item.margin.vertical());
            } else {
                main += measured.height + item.margin.vertical();
                cross = cross.max(measured.width + item.margin.horizontal());
            }
        }
        if visible_children > 1 {
            main += stack.gap * (visible_children as i32 - 1);
        }
        let preferred_width = if stack.direction.is_horizontal() {
            main + stack.padding.horizontal()
        } else {
            cross + stack.padding.horizontal()
        };
        let preferred_height = if stack.direction.is_horizontal() {
            cross + stack.padding.vertical()
        } else {
            main + stack.padding.vertical()
        };
        Ok(NodeSize {
            width: clamp_width(preferred_width, stack.min_width.or(stack.item.min_width), stack.max_width.or(stack.item.max_width)).min(width_limit.max(preferred_width)),
            height: clamp_height(preferred_height, stack.min_height, stack.max_height),
        })
    }

    fn measure_grid(
        &self,
        grid: &nexus_layout_types::Grid,
        children: &[LayoutNode],
        available_width: FxPx,
        depth: usize,
        measure: &dyn MeasureText,
    ) -> Result<NodeSize, LayoutError> {
        let width = clamp_width(
            available_width,
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
            for col in 0..n_cols {
                if child_idx >= children.len() {
                    break;
                }
                let child = &children[child_idx];
                let item = child.item();
                let width = col_widths[col].saturating_sub(item.margin.horizontal());
                let measured = self.measure_node(child, width, depth + 1, measure)?;
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
            height: clamp_height(total_height + grid.padding.vertical(), grid.min_height, grid.max_height),
        })
    }
}

impl Default for LayoutEngine {
    fn default() -> Self {
        Self::new()
    }
}

fn clamp_width(value: FxPx, min: Option<FxPx>, max: Option<FxPx>) -> FxPx {
    let mut out = value.max(FxPx::ZERO);
    if let Some(max) = max {
        out = out.min(max);
    }
    if let Some(min) = min {
        out = out.max(min);
    }
    out
}

fn clamp_height(value: FxPx, min: Option<FxPx>, max: Option<FxPx>) -> FxPx {
    let mut out = value.max(FxPx::ZERO);
    if let Some(max) = max {
        out = out.min(max);
    }
    if let Some(min) = min {
        out = out.max(min);
    }
    out
}

fn justify_offsets(justify: Justify, free_space: FxPx, count: usize, base_gap: FxPx) -> (FxPx, FxPx) {
    if count <= 1 {
        return match justify {
            Justify::Center => (free_space / 2, FxPx::ZERO),
            Justify::End => (free_space, FxPx::ZERO),
            _ => (FxPx::ZERO, base_gap),
        };
    }
    match justify {
        Justify::Start => (FxPx::ZERO, base_gap),
        Justify::Center => (free_space / 2, base_gap),
        Justify::End => (free_space, base_gap),
        Justify::SpaceBetween => (FxPx::ZERO, base_gap + free_space / (count as i32 - 1)),
        Justify::SpaceAround => {
            let slot = free_space / count as i32;
            (slot / 2, base_gap + slot)
        }
        Justify::SpaceEvenly => {
            let slot = free_space / (count as i32 + 1);
            (slot, base_gap + slot)
        }
    }
}

fn align_offset(align: Align, free_space: FxPx) -> FxPx {
    match align {
        Align::Start | Align::Stretch => FxPx::ZERO,
        Align::Center => free_space / 2,
        Align::End => free_space,
    }
}
