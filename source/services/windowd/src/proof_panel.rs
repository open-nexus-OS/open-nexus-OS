// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use alloc::vec::Vec;
use nexus_layout::{LayoutEngine, LayoutResult};
use nexus_layout_types::{Align, Direction, EdgeBorder, EdgeInsets, FxPx, Justify, LayoutNode, LineLayout, MeasureText, Overflow, PreparedTextHandle, Rgba8, Stack, TextContent, TextNode, VisualStyle};

pub fn build_proof_panel() -> LayoutNode {
    let panel_style = VisualStyle {
        background: Some(Rgba8 { r: 0x18, g: 0x18, b: 0x16, a: 0xd8 }),
        border: EdgeBorder::all(FxPx::new(1), Rgba8 { r: 0xff, g: 0xff, b: 0xff, a: 0x70 }),
        ..Default::default()
    };
    fn card() -> LayoutNode {
        LayoutNode::Stack(Stack { direction: Direction::Column, gap: FxPx::ZERO, padding: EdgeInsets::all(FxPx::new(14)), align: Align::Start, justify: Justify::Start, overflow: Overflow::Visible, flex_wrap: false, min_width: Some(FxPx::new(126)), max_width: Some(FxPx::new(126)), min_height: Some(FxPx::new(82)), max_height: Some(FxPx::new(82)) },
        VisualStyle { background: Some(Rgba8 { r: 0x28, g: 0x24, b: 0x20, a: 0xd8 }), border: EdgeBorder::all(FxPx::new(1), Rgba8 { r: 0x88, g: 0x88, b: 0x88, a: 0x88 }), ..Default::default() },
        vec![LayoutNode::Text(TextNode { content: TextContent::new(""), max_lines: None, min_width: None, max_width: None }, VisualStyle::default())])
    }
    LayoutNode::Stack(Stack { direction: Direction::Column, gap: FxPx::new(16), padding: EdgeInsets::all(FxPx::new(24)), align: Align::Start, justify: Justify::Start, overflow: Overflow::Visible, flex_wrap: false, min_width: Some(FxPx::new(610)), max_width: Some(FxPx::new(610)), min_height: None, max_height: None },
    panel_style,
    vec![
        LayoutNode::Text(TextNode { content: TextContent::new("Open Nexus OS"), max_lines: None, min_width: None, max_width: None }, VisualStyle::default()),
        LayoutNode::Stack(Stack { direction: Direction::Row, gap: FxPx::new(16), padding: EdgeInsets::zero(), align: Align::Center, justify: Justify::Start, overflow: Overflow::Visible, flex_wrap: false, min_width: None, max_width: None, min_height: None, max_height: None },
        VisualStyle::default(), vec![card(), card(), card(), card()]),
    ])
}

/// Compute layout for the proof panel. Returns the LayoutResult for rendering.
pub fn compute_panel_layout() -> Result<LayoutResult, &'static str> {
    let panel = build_proof_panel();
    struct NoopMeasure;
    impl MeasureText for NoopMeasure {
        fn prepare(&self, _: &str) -> PreparedTextHandle { PreparedTextHandle(0) }
        fn measure_width(&self, _: PreparedTextHandle) -> FxPx { FxPx::ZERO }
        fn layout_lines(&self, _: PreparedTextHandle, _: FxPx, _: Option<u32>) -> LineLayout { LineLayout { lines: Vec::new(), natural_width: FxPx::ZERO } }
    }
    let engine = LayoutEngine::new();
    engine.layout(&panel, FxPx::new(610), &NoopMeasure).map_err(|_| "layout: proof panel failed")
}
