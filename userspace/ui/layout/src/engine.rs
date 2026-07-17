// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//!
//! CONTEXT: Layout engine algorithms for TASK-0058 / RFC-0057.
//! OWNERS: @ui
//! STATUS: Done
//! API_STABILITY: Unstable
//! TEST_COVERAGE: engine_tests (8 tests)
//! ADR: docs/rfcs/RFC-0057-ui-v3a-layout-engine-pretext-contract.md

use crate::error::LayoutError;
use alloc::vec::Vec;
use nexus_layout_types::{
    Align, FlexItem, FxPx, Justify, LayoutNode, MeasureText, Overflow, Rect, TextContent,
    VisualStyle,
};

const DEFAULT_MAX_NODES: usize = 4096;
const DEFAULT_MAX_DEPTH: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NodeSize {
    width: FxPx,
    height: FxPx,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LayoutConstraints {
    max_width: FxPx,
    max_height: Option<FxPx>,
    /// DEFINITE constraints (the viewport root, `layout_with_viewport`): the
    /// node FILLS the constraint box instead of hugging its content — the
    /// page root spans the surface, so `.align(center)` + `Spacer` really
    /// center. Never propagated to children (they keep content sizing).
    definite: bool,
}

impl LayoutConstraints {
    const fn new(max_width: FxPx, max_height: Option<FxPx>) -> Self {
        Self { max_width, max_height, definite: false }
    }

    const fn definite(max_width: FxPx, max_height: Option<FxPx>) -> Self {
        Self { max_width, max_height, definite: true }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutBox {
    pub node_id: usize,
    pub id: Option<&'static str>,
    pub rect: Rect,
    pub z_index: i16,
    pub visual: VisualStyle,
    /// Optional scissor rect inherited from nearest Overflow::Hidden ancestor.
    /// The renderer must clip paint output to this rect (in content coordinates).
    pub clip_rect: Option<Rect>,
    /// Scroll offset (dx, dy) for overflow containers. Children are shifted by this
    /// amount relative to the container origin. Non-scrollable boxes have (0, 0).
    pub scroll_offset: (FxPx, FxPx),
    /// The container's overflow mode. Used by renderer to decide whether to apply
    /// scissor clipping and scrollbar rendering.
    pub overflow: Overflow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutResult {
    pub boxes: Vec<LayoutBox>,
    pub content_height: FxPx,
}

/// Scroll damage — at most two dirty rects per scroll delta.
/// Allocation-free (stack-only), bounded size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollDamage {
    /// Up to two damage rects. A `None` entry means no rect at that slot.
    pub rects: [Option<Rect>; 2],
}

impl ScrollDamage {
    pub const EMPTY: Self = Self { rects: [None, None] };

    pub fn is_empty(&self) -> bool {
        self.rects[0].is_none() && self.rects[1].is_none()
    }
}

/// Compute the dirty area when a viewport scrolls from `old_offset` to `new_offset`.
/// Returns at most two rects: the newly-exposed strip and the newly-hidden strip
/// (which need invalidation and repaint respectively).
///
/// Integer-only, deterministic, order-agnostic.
pub fn compute_scroll_damage(
    old_offset: (FxPx, FxPx),
    new_offset: (FxPx, FxPx),
    viewport: Rect,
) -> ScrollDamage {
    let dx = new_offset.0 - old_offset.0;
    let dy = new_offset.1 - old_offset.1;
    if dx.0 == 0 && dy.0 == 0 {
        return ScrollDamage::EMPTY;
    }
    let mut damage = ScrollDamage::EMPTY;
    let abs_dx = FxPx::new(dx.0.abs());
    let abs_dy = FxPx::new(dy.0.abs());

    if dx.0 != 0 {
        // Newly-exposed strip at the leading edge
        let exposed = if dx.0 > 0 {
            // Scrolling right: left side shifts out, right side becomes visible
            Rect::new(
                viewport.x + viewport.width.saturating_sub(abs_dx),
                viewport.y,
                abs_dx.min(viewport.width),
                viewport.height,
            )
        } else {
            // Scrolling left: right side shifts out, left side becomes visible
            Rect::new(viewport.x, viewport.y, abs_dx.min(viewport.width), viewport.height)
        };
        if exposed.width > FxPx::ZERO && exposed.height > FxPx::ZERO {
            damage.rects[0] = Some(exposed);
        }
    }

    if dy.0 != 0 {
        let exposed = if dy.0 > 0 {
            Rect::new(
                viewport.x,
                viewport.y + viewport.height.saturating_sub(abs_dy),
                viewport.width,
                abs_dy.min(viewport.height),
            )
        } else {
            Rect::new(viewport.x, viewport.y, viewport.width, abs_dy.min(viewport.height))
        };
        if exposed.width > FxPx::ZERO && exposed.height > FxPx::ZERO {
            let slot = if damage.rects[0].is_some() { 1 } else { 0 };
            damage.rects[slot] = Some(exposed);
        }
    }

    damage
}

impl LayoutResult {
    /// Reposition all boxes inside the scroll container identified by `container_node_id`
    /// to reflect a new scroll offset. Returns the scroll damage rects.
    ///
    /// This is place-only: no remeasurement, no text reshaping.
    /// Allocation-free (mutates existing boxes).
    pub fn reposition_scroll(
        &mut self,
        container_node_id: usize,
        new_offset: (FxPx, FxPx),
    ) -> ScrollDamage {
        let mut old_offset = (FxPx::ZERO, FxPx::ZERO);
        let mut viewport = Rect::zero();
        let mut container_found = false;

        // Find the container
        for b in &self.boxes {
            if b.node_id == container_node_id {
                old_offset = b.scroll_offset;
                viewport = b.rect;
                container_found = true;
                break;
            }
        }
        if !container_found {
            return ScrollDamage::EMPTY;
        }

        let delta_x = new_offset.0 - old_offset.0;
        let delta_y = new_offset.1 - old_offset.1;

        if delta_x.0 == 0 && delta_y.0 == 0 {
            return ScrollDamage::EMPTY;
        }

        let damage = compute_scroll_damage(old_offset, new_offset, viewport);

        // Shift descendant boxes: only those with node_id > container_node_id
        // AND the same old scroll_offset. In DFS order, descendants always have
        // higher node_ids than their ancestor.
        for b in &mut self.boxes {
            if b.node_id > container_node_id && b.scroll_offset == old_offset {
                b.scroll_offset = new_offset;
                b.rect.x += delta_x;
                b.rect.y += delta_y;
            }
        }

        // Update the container's own scroll_offset
        for b in &mut self.boxes {
            if b.node_id == container_node_id {
                b.scroll_offset = new_offset;
                break;
            }
        }

        damage
    }
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
        self.layout_with_viewport(root, available_width, None, measure)
    }

    /// Like [`Self::layout`], with a bounded viewport HEIGHT: the root column
    /// then distributes free space to `flex_grow` children (`Spacer`), so
    /// vertical centering works. Width-only layout (scrollable content) keeps
    /// using [`Self::layout`] — content height stays unbounded there.
    pub fn layout_with_viewport(
        &self,
        root: &LayoutNode,
        available_width: FxPx,
        available_height: Option<FxPx>,
        measure: &dyn MeasureText,
    ) -> Result<LayoutResult, LayoutError> {
        let mut node_count = 0;
        let mut boxes = Vec::new();
        // The ROOT is the surface: definite constraints make it fill the
        // viewport (a hugging root collapsed every centered page top-left).
        let constraints = LayoutConstraints::definite(available_width, available_height);
        self.place_node(
            root,
            FxPx::ZERO,
            FxPx::ZERO,
            constraints,
            0,
            None,
            (FxPx::ZERO, FxPx::ZERO),
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
        constraints: LayoutConstraints,
        depth: usize,
        parent_clip: Option<Rect>,
        scroll_offset: (FxPx, FxPx),
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
            LayoutNode::Stack(stack, style, children) => self.place_stack(
                node_id,
                stack,
                style,
                children,
                x,
                y,
                constraints,
                depth,
                parent_clip,
                scroll_offset,
                measure,
                node_count,
                boxes,
            ),
            LayoutNode::Grid(grid, style, children) => self.place_grid(
                node_id,
                grid,
                style,
                children,
                x,
                y,
                constraints,
                depth,
                parent_clip,
                scroll_offset,
                measure,
                node_count,
                boxes,
            ),
            LayoutNode::Spacer(spacer) => {
                let main = spacer.min_size.unwrap_or(FxPx::ZERO);
                boxes.push(LayoutBox {
                    node_id,
                    id: spacer.id,
                    rect: Rect::new(x, y, main, FxPx::ZERO),
                    z_index: spacer.item.z_index,
                    visual: VisualStyle::default(),
                    clip_rect: parent_clip,
                    scroll_offset,
                    overflow: Overflow::Visible,
                });
                Ok(NodeSize { width: main, height: FxPx::ZERO })
            }
            LayoutNode::Text(text, style) => self.place_text(
                node_id,
                text,
                style,
                x,
                y,
                constraints,
                parent_clip,
                scroll_offset,
                measure,
                boxes,
            ),
            LayoutNode::TextInput(input, style) => self.place_text_input(
                node_id,
                input,
                style,
                x,
                y,
                constraints,
                parent_clip,
                scroll_offset,
                measure,
                boxes,
            ),
        }
    }

    fn place_text(
        &self,
        node_id: usize,
        text: &nexus_layout_types::TextNode,
        style: &VisualStyle,
        x: FxPx,
        y: FxPx,
        constraints: LayoutConstraints,
        parent_clip: Option<Rect>,
        scroll_offset: (FxPx, FxPx),
        measure: &dyn MeasureText,
        boxes: &mut Vec<LayoutBox>,
    ) -> Result<NodeSize, LayoutError> {
        let width_limit = clamp_width(
            constraints.max_width,
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
        .min(width_limit);
        let height = clamp_to_max_height(
            lines.lines.iter().fold(FxPx::ZERO, |acc, line| acc + line.height).max(FxPx::new(1)),
            constraints.max_height,
        );
        boxes.push(LayoutBox {
            node_id,
            id: text.id,
            rect: Rect::new(x, y, width, height),
            z_index: text.item.z_index,
            visual: style.clone(),
            clip_rect: parent_clip,
            scroll_offset,
            overflow: Overflow::Visible,
        });
        Ok(NodeSize { width, height })
    }

    fn place_text_input(
        &self,
        node_id: usize,
        input: &nexus_layout_types::TextInputNode,
        style: &VisualStyle,
        x: FxPx,
        y: FxPx,
        constraints: LayoutConstraints,
        parent_clip: Option<Rect>,
        scroll_offset: (FxPx, FxPx),
        measure: &dyn MeasureText,
        boxes: &mut Vec<LayoutBox>,
    ) -> Result<NodeSize, LayoutError> {
        // Treat TextInput like a Text node for layout: use its content for
        // measurement, defaulting to placeholder if content is empty.
        let display_content = if input.content.as_str().is_empty() {
            input.placeholder.as_ref().map(|p| p.as_str()).unwrap_or("")
        } else {
            input.content.as_str()
        };
        let text_content = TextContent::new(display_content);
        let text_node = nexus_layout_types::TextNode {
            id: input.id,
            content: text_content,
            style: input.style.clone(),
            item: input.item,
            max_lines: Some(1),
            min_width: input.min_width,
            max_width: input.max_width,
        };
        self.place_text(
            node_id,
            &text_node,
            style,
            x,
            y,
            constraints,
            parent_clip,
            scroll_offset,
            measure,
            boxes,
        )
    }

    fn place_stack(
        &self,
        node_id: usize,
        stack: &nexus_layout_types::Stack,
        style: &VisualStyle,
        children: &[LayoutNode],
        x: FxPx,
        y: FxPx,
        constraints: LayoutConstraints,
        depth: usize,
        parent_clip: Option<Rect>,
        scroll_offset: (FxPx, FxPx),
        measure: &dyn MeasureText,
        node_count: &mut usize,
        boxes: &mut Vec<LayoutBox>,
    ) -> Result<NodeSize, LayoutError> {
        let measured = self.measure_stack(stack, children, constraints, depth, measure)?;
        // Definite constraints (the viewport root): FILL the constraint box —
        // content sizing (hug) is for nested stacks only.
        let width = if constraints.definite { constraints.max_width } else { measured.width };
        let height = if constraints.definite {
            constraints.max_height.unwrap_or(measured.height)
        } else {
            measured.height
        };
        let container_index = boxes.len();
        let is_overflow_hidden = matches!(stack.overflow, Overflow::Hidden | Overflow::Scroll(_));
        let container_scroll =
            if is_overflow_hidden { scroll_offset } else { (FxPx::ZERO, FxPx::ZERO) };
        let content_width = width.saturating_sub(stack.padding.horizontal());
        let content_height = height.saturating_sub(stack.padding.vertical());
        let container_clip = if is_overflow_hidden {
            let own = Rect::new(
                x + stack.padding.left,
                y + stack.padding.top,
                content_width,
                content_height,
            );
            intersect_clip(Some(own), parent_clip)
        } else {
            parent_clip
        };
        boxes.push(LayoutBox {
            node_id,
            id: stack.id,
            rect: Rect::new(x, y, width, height),
            z_index: stack.item.z_index,
            visual: style.clone(),
            clip_rect: parent_clip,
            scroll_offset: container_scroll,
            overflow: stack.overflow,
        });
        let padding = stack.padding;
        let content_x = x + padding.left - container_scroll.0;
        let content_y = y + padding.top - container_scroll.1;
        // A scroll viewport lays its children out UNBOUNDED on the main axis:
        // content is allowed to overflow the clip (that overflow IS the
        // scrollable extent). A definite height here made the inner list
        // shrink its rows to fit — nothing left to scroll.
        let child_constraints = if matches!(stack.overflow, Overflow::Scroll(_)) {
            LayoutConstraints::new(content_width, None)
        } else {
            LayoutConstraints::new(content_width, Some(content_height))
        };
        if stack.direction.is_horizontal() {
            self.place_stack_row(
                stack,
                children,
                content_x,
                content_y,
                child_constraints,
                depth,
                container_clip,
                container_scroll,
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
                child_constraints,
                depth,
                container_clip,
                container_scroll,
                measure,
                node_count,
                boxes,
            )?
        };
        // Fix up the container clip rect height now that we know the actual height
        if is_overflow_hidden {
            let own = Rect::new(
                x + stack.padding.left,
                y + stack.padding.top,
                content_width,
                content_height,
            );
            boxes[container_index].clip_rect = intersect_clip(Some(own), parent_clip);
        }
        Ok(NodeSize { width, height })
    }

    fn place_stack_row(
        &self,
        stack: &nexus_layout_types::Stack,
        children: &[LayoutNode],
        content_x: FxPx,
        content_y: FxPx,
        constraints: LayoutConstraints,
        depth: usize,
        parent_clip: Option<Rect>,
        scroll_offset: (FxPx, FxPx),
        measure: &dyn MeasureText,
        node_count: &mut usize,
        boxes: &mut Vec<LayoutBox>,
    ) -> Result<NodeSize, LayoutError> {
        let mut in_flow: Vec<(usize, &LayoutNode, FlexItem, NodeSize, FxPx)> = Vec::new();
        let mut absolute: Vec<(&LayoutNode, FlexItem, NodeSize)> = Vec::new();
        let mut fixed_main = FxPx::ZERO;
        let mut total_grow = 0u32;
        let content_width = constraints.max_width;
        for (index, child) in children.iter().enumerate() {
            let item = effective_item(child);
            let measured = self.measure_node(
                child,
                child_constraints(constraints, item, content_width, constraints.max_height),
                depth + 1,
                measure,
            )?;
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
        let free_or_deficit = content_width.0 - fixed_with_gap.0;
        let mut allocations: Vec<FxPx> = Vec::with_capacity(in_flow.len());
        if free_or_deficit >= 0 {
            let free_space = FxPx::new(free_or_deficit);
            for (_, _, item, _, base_main) in &in_flow {
                let extra = if total_grow > 0 && item.flex_grow > 0 {
                    FxPx::new((free_space.0 * item.flex_grow as i32) / total_grow as i32)
                } else {
                    FxPx::ZERO
                };
                allocations.push(*base_main + extra);
            }
        } else {
            let deficit = (-free_or_deficit) as u32;
            let mut total_shrink = 0u32;
            for (_, _, item, _, _) in &in_flow {
                total_shrink += item.flex_shrink;
            }
            for (_, _, item, _, base_main) in &in_flow {
                let shrink = if total_shrink > 0 && item.flex_shrink > 0 {
                    (deficit as u64 * item.flex_shrink as u64 / total_shrink as u64) as i32
                } else {
                    0
                };
                allocations.push(FxPx::new((base_main.0 - shrink).max(0)));
            }
        }
        let mut row_height = FxPx::ZERO;
        let mut used_main = FxPx::ZERO;
        for ((_, child, item, _, _), allocation) in in_flow.iter().zip(allocations.iter()) {
            let child_width = allocation.saturating_sub(item.margin.horizontal());
            let measured = self.measure_node(
                child,
                child_constraints(constraints, *item, child_width, constraints.max_height),
                depth + 1,
                measure,
            )?;
            row_height = row_height.max(measured.height + item.margin.vertical());
            used_main += *allocation;
        }
        if !in_flow.is_empty() {
            used_main += stack.gap * gap_count;
        }
        let (mut cursor, extra_gap) = justify_offsets(
            stack.justify,
            content_width.saturating_sub(used_main),
            in_flow.len(),
            stack.gap,
        );
        cursor += content_x;
        let available_cross = constraints.max_height.unwrap_or(row_height);
        for ((_, child, item, _, _), allocation) in in_flow.iter().zip(allocations.iter()) {
            let child_width = allocation.saturating_sub(item.margin.horizontal());
            let measured = self.measure_node(
                child,
                child_constraints(constraints, *item, child_width, Some(available_cross)),
                depth + 1,
                measure,
            )?;
            let align = item.align_self.unwrap_or(stack.align);
            let child_x = cursor + item.margin.left;
            let cross_space =
                available_cross.saturating_sub(measured.height + item.margin.vertical());
            let child_y = content_y + item.margin.top + align_offset(align, cross_space);
            let child_node_id = *node_count + 1;
            // Stretched row children fill their allocation (definite) — see
            // the column loop above for the rationale.
            let child_c = if matches!(align, Align::Stretch) && constraints.max_height.is_some() {
                LayoutConstraints::definite(
                    child_width,
                    Some(available_cross.saturating_sub(item.margin.vertical())),
                )
            } else {
                child_constraints(constraints, *item, child_width, Some(available_cross))
            };
            self.place_node(
                child,
                child_x,
                child_y,
                child_c,
                depth + 1,
                parent_clip,
                scroll_offset,
                measure,
                node_count,
                boxes,
            )?;
            let final_height =
                if matches!(align, Align::Stretch) && constraints.max_height.is_some() {
                    available_cross.saturating_sub(item.margin.vertical())
                } else {
                    measured.height
                };
            update_box_geometry(
                boxes,
                child_node_id,
                child,
                child_x,
                child_y,
                child_width,
                final_height,
                parent_clip,
            );
            cursor += *allocation + extra_gap;
        }
        for (child, item, _) in absolute {
            let child_x = content_x + item.margin.left;
            let child_y = content_y + item.margin.top;
            let child_width = content_width.saturating_sub(item.margin.horizontal());
            // Overlay contract: an absolute child WITH `flex_grow` FILLS the
            // parent's content box (definite constraints — the viewport-root
            // fill semantic). Plain absolutes keep content sizing (pips).
            let child_c = if item.flex_grow > 0 {
                LayoutConstraints::definite(child_width, constraints.max_height)
            } else {
                child_constraints(constraints, item, child_width, constraints.max_height)
            };
            self.place_node(
                child,
                child_x,
                child_y,
                child_c,
                depth + 1,
                parent_clip,
                scroll_offset,
                measure,
                node_count,
                boxes,
            )?;
        }
        Ok(NodeSize { width: content_width, height: available_cross.max(row_height) })
    }

    fn place_stack_column(
        &self,
        stack: &nexus_layout_types::Stack,
        children: &[LayoutNode],
        content_x: FxPx,
        content_y: FxPx,
        constraints: LayoutConstraints,
        depth: usize,
        parent_clip: Option<Rect>,
        scroll_offset: (FxPx, FxPx),
        measure: &dyn MeasureText,
        node_count: &mut usize,
        boxes: &mut Vec<LayoutBox>,
    ) -> Result<NodeSize, LayoutError> {
        let mut in_flow: Vec<(&LayoutNode, FlexItem, NodeSize, FxPx)> = Vec::new();
        let mut absolute: Vec<(&LayoutNode, FlexItem)> = Vec::new();
        let mut fixed_main = FxPx::ZERO;
        let mut max_cross = FxPx::ZERO;
        let mut total_grow = 0u32;
        let content_width = constraints.max_width;
        for child in children {
            let item = effective_item(child);
            let child_width = content_width.saturating_sub(item.margin.horizontal());
            let measured = self.measure_node(
                child,
                child_constraints(constraints, item, child_width, None),
                depth + 1,
                measure,
            )?;
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
        let available_content = constraints.max_height.unwrap_or(fixed_with_gap.max(FxPx::ZERO));
        let free_or_deficit = available_content.0 - fixed_with_gap.0;
        let mut allocations: Vec<FxPx> = Vec::with_capacity(in_flow.len());
        let mut used_main = FxPx::ZERO;
        if free_or_deficit >= 0 {
            // Extra space: distribute via flex_grow
            let free_space = FxPx::new(free_or_deficit);
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
        } else if matches!(stack.overflow, Overflow::Scroll(_)) {
            // Scroll viewport: children KEEP their content size and overflow
            // the clip — shrinking them to fit would leave nothing to scroll.
            for (_, _, _, base_main) in &in_flow {
                allocations.push(*base_main);
                used_main += *base_main;
            }
        } else {
            // Deficit: distribute via flex_shrink (proportional shrink)
            let deficit = (-free_or_deficit) as u32;
            let mut total_shrink = 0u32;
            for (_, item, _, _) in &in_flow {
                total_shrink += item.flex_shrink;
            }
            for (_, item, _, base_main) in &in_flow {
                let shrink = if total_shrink > 0 && item.flex_shrink > 0 {
                    (deficit as u64 * item.flex_shrink as u64 / total_shrink as u64) as i32
                } else {
                    0
                };
                let allocation = FxPx::new((base_main.0 - shrink).max(0));
                allocations.push(allocation);
                used_main += allocation;
            }
        }
        if !in_flow.is_empty() {
            used_main += stack.gap * gap_count;
        }
        let justify_free = available_content.saturating_sub(used_main);
        let (mut cursor, extra_gap) =
            justify_offsets(stack.justify, FxPx::ZERO.max(justify_free), in_flow.len(), stack.gap);
        cursor += content_y;
        let mut column_height = FxPx::ZERO;
        for ((child, item, measured, _), allocation) in in_flow.iter().zip(allocations.iter()) {
            let align = item.align_self.unwrap_or(stack.align);
            let child_width = if matches!(align, Align::Stretch) {
                content_width.saturating_sub(item.margin.horizontal())
            } else {
                measured.width.min(content_width.saturating_sub(item.margin.horizontal()))
            };
            let cross_space = content_width.saturating_sub(child_width + item.margin.horizontal());
            let child_x = content_x + item.margin.left + align_offset(align, cross_space);
            let child_y = cursor + item.margin.top;
            let child_node_id = *node_count + 1; // predict the child's node_id
            let allocated_height = allocation.saturating_sub(item.margin.vertical());
            // A STRETCHED child's size is parent-determined (cross = stretched
            // width, main = the allocation) — DEFINITE constraints make the
            // child FILL it, so its own children lay out against the real
            // size (a hugging nested row collapsed its Spacer: the shell's
            // topbar was 161px wide inside a 1280px slot).
            let child_c = if matches!(align, Align::Stretch) {
                LayoutConstraints::definite(child_width, Some(allocated_height))
            } else {
                child_constraints(constraints, *item, child_width, Some(allocated_height))
            };
            self.place_node(
                child,
                child_x,
                child_y,
                child_c,
                depth + 1,
                parent_clip,
                scroll_offset,
                measure,
                node_count,
                boxes,
            )?;
            update_box_geometry(
                boxes,
                child_node_id,
                child,
                child_x,
                child_y,
                child_width,
                allocated_height,
                parent_clip,
            );
            cursor += *allocation + extra_gap;
            column_height = cursor - content_y - extra_gap;
            max_cross = max_cross.max(child_width + item.margin.horizontal());
        }
        for (child, item) in absolute {
            let child_x = content_x + item.margin.left;
            let child_y = content_y + item.margin.top;
            let child_width = content_width.saturating_sub(item.margin.horizontal());
            // Overlay contract: an absolute child WITH `flex_grow` FILLS the
            // parent's content box (definite constraints — the viewport-root
            // fill semantic). Plain absolutes keep content sizing (pips).
            let child_c = if item.flex_grow > 0 {
                LayoutConstraints::definite(child_width, constraints.max_height)
            } else {
                child_constraints(constraints, item, child_width, constraints.max_height)
            };
            self.place_node(
                child,
                child_x,
                child_y,
                child_c,
                depth + 1,
                parent_clip,
                scroll_offset,
                measure,
                node_count,
                boxes,
            )?;
        }
        Ok(NodeSize {
            width: max_cross,
            height: constraints.max_height.unwrap_or(column_height.max(FxPx::ZERO)),
        })
    }

    fn place_grid(
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
            visual: style.clone(),
            clip_rect: parent_clip,
            scroll_offset: container_scroll,
            overflow: grid.overflow,
        });
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
                self.place_node(
                    child,
                    col_x + item.margin.left,
                    row_y + item.margin.top,
                    child_constraints(
                        LayoutConstraints::new(content_width, Some(content_height)),
                        *item,
                        child_width,
                        None,
                    ),
                    depth + 1,
                    container_clip,
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

    fn measure_node(
        &self,
        node: &LayoutNode,
        constraints: LayoutConstraints,
        depth: usize,
        measure: &dyn MeasureText,
    ) -> Result<NodeSize, LayoutError> {
        if depth > self.max_depth {
            return Err(LayoutError::TooDeep { max: self.max_depth, actual: depth });
        }
        match node {
            LayoutNode::Stack(stack, _, children) => {
                self.measure_stack(stack, children, constraints, depth, measure)
            }
            LayoutNode::Grid(grid, _, children) => {
                self.measure_grid(grid, children, constraints, depth, measure)
            }
            LayoutNode::Spacer(spacer) => {
                let main = spacer.min_size.unwrap_or(FxPx::ZERO);
                Ok(NodeSize {
                    width: main,
                    height: clamp_to_max_height(FxPx::ZERO, constraints.max_height),
                })
            }
            LayoutNode::Text(text, _) => self.measure_text(text, constraints, measure),
            LayoutNode::TextInput(input, _) => {
                let display = if input.content.as_str().is_empty() {
                    input.placeholder.as_ref().map(|p| p.as_str()).unwrap_or("")
                } else {
                    input.content.as_str()
                };
                let text_node = nexus_layout_types::TextNode {
                    id: input.id,
                    content: TextContent::new(display),
                    style: input.style.clone(),
                    item: input.item,
                    max_lines: Some(1),
                    min_width: input.min_width,
                    max_width: input.max_width,
                };
                self.measure_text(&text_node, constraints, measure)
            }
        }
    }

    fn measure_text(
        &self,
        text: &nexus_layout_types::TextNode,
        constraints: LayoutConstraints,
        measure: &dyn MeasureText,
    ) -> Result<NodeSize, LayoutError> {
        let width_limit = clamp_width(
            constraints.max_width,
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
        .min(width_limit);
        let height = clamp_to_max_height(
            lines.lines.iter().fold(FxPx::ZERO, |acc, line| acc + line.height).max(FxPx::new(1)),
            constraints.max_height,
        );
        Ok(NodeSize { width, height })
    }

    fn measure_stack(
        &self,
        stack: &nexus_layout_types::Stack,
        children: &[LayoutNode],
        constraints: LayoutConstraints,
        depth: usize,
        measure: &dyn MeasureText,
    ) -> Result<NodeSize, LayoutError> {
        let width_limit = clamp_width(
            constraints.max_width,
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
            let measured = self.measure_node(
                child,
                child_constraints(constraints, *item, child_width, None),
                depth + 1,
                measure,
            )?;
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
        // A clipped container is a SCROLL VIEWPORT: its content scrolls, so
        // its preferred MAIN size must not be the content sum (that squeezed
        // every sibling to its minimum — the CSS `min-height: 0` flex rule).
        // The viewport takes only what flex gives it (`grow`); CROSS still
        // follows the content so a horizontal scroller keeps its row height.
        if matches!(stack.overflow, Overflow::Scroll(_)) {
            main = FxPx::ZERO;
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
            width: clamp_width(
                preferred_width,
                stack.min_width.or(stack.item.min_width),
                stack.max_width.or(stack.item.max_width),
            )
            .min(width_limit),
            height: clamp_to_max_height(
                clamp_height(preferred_height, stack.min_height, stack.max_height),
                constraints.max_height,
            ),
        })
    }

    fn measure_grid(
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

fn clamp_to_max_height(value: FxPx, max_height: Option<FxPx>) -> FxPx {
    match max_height {
        Some(max_height) => value.min(max_height),
        None => value,
    }
}

/// The child's flex data with `Spacer::flex_grow` honored: a `Spacer` grows
/// by its OWN declared factor (default 1) even when its generic `FlexItem`
/// says 0 — the spacer's whole purpose is absorbing free space; reading only
/// `FlexItem.flex_grow` made every default spacer inert (top-left greeter).
fn effective_item(child: &LayoutNode) -> FlexItem {
    let mut item = *child.item();
    if let LayoutNode::Spacer(spacer) = child {
        item.flex_grow = item.flex_grow.max(spacer.flex_grow);
    }
    item
}

fn child_constraints(
    parent: LayoutConstraints,
    item: FlexItem,
    max_width: FxPx,
    max_height: Option<FxPx>,
) -> LayoutConstraints {
    let height = max_height.or(parent.max_height);
    LayoutConstraints::new(
        max_width.max(FxPx::ZERO),
        height.map(|value| value.saturating_sub(item.margin.vertical())),
    )
}

fn update_box_geometry(
    boxes: &mut [LayoutBox],
    node_id: usize,
    node: &LayoutNode,
    x: FxPx,
    y: FxPx,
    width: FxPx,
    height: FxPx,
    parent_clip: Option<Rect>,
) {
    let Some(layout_box) = boxes.iter_mut().find(|layout_box| layout_box.node_id == node_id) else {
        return;
    };
    layout_box.rect = Rect::new(x, y, width, height);
    match node {
        LayoutNode::Stack(stack, _, _)
            if matches!(stack.overflow, Overflow::Hidden | Overflow::Scroll(_)) =>
        {
            let own = Rect::new(
                x + stack.padding.left,
                y + stack.padding.top,
                width.saturating_sub(stack.padding.horizontal()),
                height.saturating_sub(stack.padding.vertical()),
            );
            layout_box.clip_rect = intersect_clip(Some(own), parent_clip);
        }
        LayoutNode::Grid(grid, _, _)
            if matches!(grid.overflow, Overflow::Hidden | Overflow::Scroll(_)) =>
        {
            let own = Rect::new(
                x + grid.padding.left,
                y + grid.padding.top,
                width.saturating_sub(grid.padding.horizontal()),
                height.saturating_sub(grid.padding.vertical()),
            );
            layout_box.clip_rect = intersect_clip(Some(own), parent_clip);
        }
        _ => {}
    }
}

fn justify_offsets(
    justify: Justify,
    free_space: FxPx,
    count: usize,
    base_gap: FxPx,
) -> (FxPx, FxPx) {
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

/// Intersect two optional clip rects. Returns the intersection, or None if disjoint.
fn intersect_clip(a: Option<Rect>, b: Option<Rect>) -> Option<Rect> {
    match (a, b) {
        (Some(a), Some(b)) => {
            let x = a.x.max(b.x);
            let y = a.y.max(b.y);
            let x2 = (a.x + a.width).min(b.x + b.width);
            let y2 = (a.y + a.height).min(b.y + b.height);
            if x2 > x && y2 > y {
                Some(Rect::new(x, y, x2 - x, y2 - y))
            } else {
                None
            }
        }
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}
