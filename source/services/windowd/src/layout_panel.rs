// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Layout tree builder for TASK-0058 proof panel.
//! OWNERS: @ui
//! STATUS: Done
//! ADR: docs/rfcs/RFC-0057-ui-v3a-layout-engine-pretext-contract.md
use alloc::vec;
use alloc::vec::Vec;
use input_live_protocol::VisibleState;
use nexus_layout::{LayoutEngine, LayoutResult};
use nexus_layout_types::{
    Align, Direction, EdgeBorder, EdgeInsets, FlexItem, FontWeight, FxPx, Justify, LayoutNode,
    LineHeight, LineLayout, LineMetrics, MeasureText, Overflow, PathPoint, PathShape,
    PreparedTextHandle, Rgba8, ShapeKind, Stack, TextAlign, TextContent, TextNode, TextStyle,
    VisualStyle, WhiteSpace,
};

use crate::assets;
use crate::proof_panel_spec::{
    BODY_TEXT, CARD_GAP, CARD_HEIGHT, CARD_ICON_SIZE, CARD_PADDING, CARD_WIDTH, CLICK_LABEL,
    HOVER_LABEL, ICON_TARGET_SIZE, KEY_LABEL, PANEL_GAP, PANEL_HEIGHT, PANEL_PADDING, PANEL_WIDTH,
    SCROLL_LABEL, SUBTITLE_TEXT, TITLE_TEXT,
};

pub struct ProofTextMeasure;

impl MeasureText for ProofTextMeasure {
    fn prepare(&self, content: &TextContent, style: &TextStyle) -> PreparedTextHandle {
        PreparedTextHandle(
            text_asset_id(content.as_str(), style).map(text_asset_index).unwrap_or(usize::MAX),
        )
    }

    fn measure_width(&self, handle: &PreparedTextHandle) -> FxPx {
        proof_text_asset_by_index(handle.0)
            .map(|asset| FxPx::new(asset.width as i32))
            .unwrap_or(FxPx::ZERO)
    }

    fn layout_lines(
        &self,
        handle: &PreparedTextHandle,
        width: FxPx,
        max_lines: Option<u32>,
    ) -> LineLayout {
        let natural_width = self.measure_width(handle);
        let line_height = proof_text_asset_by_index(handle.0)
            .map(|asset| FxPx::new(asset.height as i32))
            .unwrap_or(FxPx::new(20));
        let line = LineMetrics {
            text_range: 0..1,
            width: natural_width.min(width.max(FxPx::ONE)),
            baseline: line_height,
            height: line_height,
        };
        let lines = if matches!(max_lines, Some(0)) { Vec::new() } else { vec![line] };
        LineLayout { lines, natural_width }
    }
}

/// Build the proof panel layout tree — single source of truth for visible proof geometry.
pub fn build_proof_panel_tree(state: VisibleState) -> LayoutNode {
    let panel_style = VisualStyle {
        background: Some(assets::PROOF_PANEL_BG),
        border: EdgeBorder::all(FxPx::new(1), assets::PROOF_PANEL_BORDER),
        ..Default::default()
    };
    let text_column = LayoutNode::Stack(
        Stack {
            id: Some("proof_text_column"),
            direction: Direction::Column,
            gap: FxPx::new(8),
            padding: EdgeInsets::zero(),
            align: Align::Start,
            justify: Justify::Start,
            overflow: Overflow::Visible,
            flex_wrap: false,
            min_width: None,
            max_width: None,
            min_height: None,
            max_height: None,
            item: FlexItem { flex_grow: 1, ..FlexItem::default() },
        },
        VisualStyle::default(),
        vec![
            text_node(TITLE_TEXT, assets::PROOF_PANEL_TITLE),
            text_node(SUBTITLE_TEXT, assets::PROOF_PANEL_SUBTITLE),
            text_node(BODY_TEXT, assets::PROOF_PANEL_MUTED),
        ],
    );
    let icon_glyph = path_node(
        "icon_target_glyph",
        24,
        assets::PROOF_ICON_FG,
        PathShape::line(&[
            PathPoint::new(120, 760),
            PathPoint::new(420, 460),
            PathPoint::new(700, 720),
            PathPoint::new(880, 280),
        ]),
    );
    let top_row = LayoutNode::Stack(
        Stack {
            id: Some("proof_top_row"),
            direction: Direction::Row,
            gap: FxPx::new(16),
            padding: EdgeInsets::zero(),
            align: Align::Start,
            justify: Justify::Start,
            overflow: Overflow::Visible,
            flex_wrap: false,
            min_width: None,
            max_width: None,
            min_height: None,
            max_height: None,
            item: FlexItem::default(),
        },
        VisualStyle::default(),
        vec![
            text_column,
            LayoutNode::Spacer(nexus_layout_types::Spacer {
                id: Some("proof_top_spacer"),
                flex_grow: 1,
                min_size: None,
                item: FlexItem::default(),
            }),
            LayoutNode::Stack(
                Stack {
                    id: Some("icon_target"),
                    direction: Direction::Column,
                    gap: FxPx::ZERO,
                    padding: EdgeInsets::all(FxPx::new(12)),
                    align: Align::Center,
                    justify: Justify::Center,
                    overflow: Overflow::Visible,
                    flex_wrap: false,
                    min_width: Some(FxPx::new(ICON_TARGET_SIZE)),
                    max_width: Some(FxPx::new(ICON_TARGET_SIZE)),
                    min_height: Some(FxPx::new(ICON_TARGET_SIZE)),
                    max_height: Some(FxPx::new(ICON_TARGET_SIZE)),
                    item: FlexItem::default(),
                },
                VisualStyle {
                    background: Some(assets::PROOF_ICON_BG),
                    border: EdgeBorder::all(FxPx::new(1), assets::PROOF_PANEL_BORDER),
                    ..Default::default()
                },
                vec![icon_glyph],
            ),
        ],
    );
    let cards_row = LayoutNode::Stack(
        Stack {
            id: Some("proof_cards_row"),
            direction: Direction::Row,
            gap: FxPx::new(CARD_GAP),
            padding: EdgeInsets::zero(),
            align: Align::Start,
            justify: Justify::Start,
            overflow: Overflow::Visible,
            flex_wrap: false,
            min_width: None,
            max_width: None,
            min_height: None,
            max_height: None,
            item: FlexItem::default(),
        },
        VisualStyle::default(),
        vec![
            card_node("card_hover", state.hover_visible, assets::PROOF_HOVER, HOVER_LABEL, false),
            card_node(
                "card_click",
                state.launcher_click_visible,
                assets::PROOF_CLICK,
                CLICK_LABEL,
                false,
            ),
            card_node(
                "card_scroll",
                state.wheel_up_visible || state.wheel_down_visible,
                assets::PROOF_SCROLL,
                SCROLL_LABEL,
                true,
            ),
            card_node("card_key", state.keyboard_visible, assets::PROOF_KEYBOARD, KEY_LABEL, false),
        ],
    );
    LayoutNode::Stack(
        Stack {
            id: Some("proof_panel"),
            direction: Direction::Column,
            gap: FxPx::new(PANEL_GAP),
            padding: EdgeInsets::all(FxPx::new(PANEL_PADDING)),
            align: Align::Start,
            justify: Justify::Start,
            overflow: Overflow::Visible,
            flex_wrap: false,
            min_width: Some(FxPx::new(PANEL_WIDTH)),
            max_width: Some(FxPx::new(PANEL_WIDTH)),
            min_height: Some(FxPx::new(PANEL_HEIGHT)),
            max_height: Some(FxPx::new(PANEL_HEIGHT)),
            item: FlexItem::default(),
        },
        panel_style,
        vec![
            top_row,
            LayoutNode::Spacer(nexus_layout_types::Spacer {
                id: Some("proof_panel_spacer"),
                flex_grow: 1,
                min_size: None,
                item: FlexItem::default(),
            }),
            cards_row,
        ],
    )
}

pub fn compute_proof_layout(state: VisibleState) -> Result<LayoutResult, &'static str> {
    LayoutEngine::new()
        .layout(&build_proof_panel_tree(state), FxPx::new(PANEL_WIDTH), &ProofTextMeasure)
        .map_err(|_| "layout failed")
}

fn text_node(spec: crate::proof_panel_spec::ProofTextSpec, color: Rgba8) -> LayoutNode {
    LayoutNode::Text(
        TextNode {
            id: Some(spec.id),
            content: TextContent::new(spec.content),
            style: TextStyle {
                font_size: FxPx::new(spec.font_size as i32),
                font_weight: match spec.font_weight {
                    700 => FontWeight::Bold,
                    600 => FontWeight::Semibold,
                    500 => FontWeight::Medium,
                    _ => FontWeight::Regular,
                },
                line_height: LineHeight::Absolute(FxPx::new(match spec.font_size {
                    30 => 34,
                    18 => 22,
                    _ => 20,
                })),
                text_align: TextAlign::Left,
                color,
                white_space: WhiteSpace::NoWrap,
            },
            item: FlexItem::default(),
            max_lines: Some(1),
            min_width: None,
            max_width: None,
        },
        VisualStyle::default(),
    )
}

fn card_node(
    id: &'static str,
    active: bool,
    accent: Rgba8,
    label: crate::proof_panel_spec::ProofTextSpec,
    show_scroll: bool,
) -> LayoutNode {
    let background = if active { assets::PROOF_CARD_ACTIVE_BG } else { assets::PROOF_CARD_BG };
    let border = if active { accent } else { assets::PROOF_CARD_BORDER };
    let mut top_row_children = vec![
        shape_node(card_part_id(id, "icon"), CARD_ICON_SIZE, accent, Some(border), ShapeKind::Rect),
        LayoutNode::Spacer(nexus_layout_types::Spacer {
            id: Some(card_part_id(id, "icon_spacer")),
            flex_grow: 1,
            min_size: None,
            item: FlexItem::default(),
        }),
    ];
    top_row_children.insert(
        1,
        shape_node(
            card_part_id(id, "dot"),
            12,
            if active { assets::PROOF_ICON_FG } else { background },
            None,
            ShapeKind::Circle,
        ),
    );
    if show_scroll {
        top_row_children.push(LayoutNode::Stack(
            Stack {
                id: Some(card_part_id(id, "scroll_markers")),
                direction: Direction::Column,
                gap: FxPx::new(6),
                padding: EdgeInsets::zero(),
                align: Align::End,
                justify: Justify::Start,
                overflow: Overflow::Visible,
                flex_wrap: false,
                min_width: None,
                max_width: None,
                min_height: None,
                max_height: None,
                item: FlexItem::default(),
            },
            VisualStyle::default(),
            vec![
                shape_node(card_part_id(id, "scroll_up"), 12, accent, None, ShapeKind::TriangleUp),
                shape_node(
                    card_part_id(id, "scroll_down"),
                    12,
                    if active { assets::PROOF_ICON_FG } else { assets::PROOF_CARD_BORDER },
                    None,
                    ShapeKind::TriangleDown,
                ),
            ],
        ));
    }
    if id != "card_scroll" {
        top_row_children.push(path_node(
            card_part_id(id, "glyph"),
            16,
            if active { assets::PROOF_ICON_FG } else { border },
            PathShape::line(&[
                PathPoint::new(160, 820),
                PathPoint::new(420, 540),
                PathPoint::new(760, 180),
            ]),
        ));
    }
    LayoutNode::Stack(
        Stack {
            id: Some(id),
            direction: Direction::Column,
            gap: FxPx::new(8),
            padding: EdgeInsets::all(FxPx::new(CARD_PADDING)),
            align: Align::Start,
            justify: Justify::SpaceBetween,
            overflow: Overflow::Visible,
            flex_wrap: false,
            min_width: Some(FxPx::new(CARD_WIDTH)),
            max_width: Some(FxPx::new(CARD_WIDTH)),
            min_height: Some(FxPx::new(CARD_HEIGHT)),
            max_height: Some(FxPx::new(CARD_HEIGHT)),
            item: FlexItem::default(),
        },
        VisualStyle {
            background: Some(background),
            border: EdgeBorder::all(FxPx::new(1), border),
            ..Default::default()
        },
        vec![
            LayoutNode::Stack(
                Stack {
                    id: Some(card_part_id(id, "top_row")),
                    direction: Direction::Row,
                    gap: FxPx::new(8),
                    padding: EdgeInsets::zero(),
                    align: Align::Center,
                    justify: Justify::Start,
                    overflow: Overflow::Visible,
                    flex_wrap: false,
                    min_width: None,
                    max_width: None,
                    min_height: None,
                    max_height: None,
                    item: FlexItem::default(),
                },
                VisualStyle::default(),
                top_row_children,
            ),
            text_node(label, assets::PROOF_CARD_LABEL),
        ],
    )
}

fn shape_node(
    id: &'static str,
    size: i32,
    background: Rgba8,
    border: Option<Rgba8>,
    shape: ShapeKind,
) -> LayoutNode {
    LayoutNode::Stack(
        Stack {
            id: Some(id),
            direction: Direction::Column,
            gap: FxPx::ZERO,
            padding: EdgeInsets::zero(),
            align: Align::Start,
            justify: Justify::Start,
            overflow: Overflow::Visible,
            flex_wrap: false,
            min_width: Some(FxPx::new(size)),
            max_width: Some(FxPx::new(size)),
            min_height: Some(FxPx::new(size)),
            max_height: Some(FxPx::new(size)),
            item: FlexItem::default(),
        },
        VisualStyle {
            background: Some(background),
            border: border.map(|color| EdgeBorder::all(FxPx::new(1), color)).unwrap_or_default(),
            shape,
            ..Default::default()
        },
        vec![],
    )
}

fn path_node(id: &'static str, size: i32, color: Rgba8, path: PathShape) -> LayoutNode {
    LayoutNode::Stack(
        Stack {
            id: Some(id),
            direction: Direction::Column,
            gap: FxPx::ZERO,
            padding: EdgeInsets::zero(),
            align: Align::Start,
            justify: Justify::Start,
            overflow: Overflow::Visible,
            flex_wrap: false,
            min_width: Some(FxPx::new(size)),
            max_width: Some(FxPx::new(size)),
            min_height: Some(FxPx::new(size)),
            max_height: Some(FxPx::new(size)),
            item: FlexItem::default(),
        },
        VisualStyle { background: Some(color), shape: ShapeKind::Path(path), ..Default::default() },
        vec![],
    )
}

fn card_part_id(card_id: &'static str, suffix: &'static str) -> &'static str {
    match (card_id, suffix) {
        ("card_hover", "icon") => "card_hover_icon",
        ("card_hover", "icon_spacer") => "card_hover_icon_spacer",
        ("card_hover", "dot") => "card_hover_dot",
        ("card_hover", "top_row") => "card_hover_top_row",
        ("card_click", "icon") => "card_click_icon",
        ("card_click", "icon_spacer") => "card_click_icon_spacer",
        ("card_click", "dot") => "card_click_dot",
        ("card_click", "top_row") => "card_click_top_row",
        ("card_scroll", "icon") => "card_scroll_icon",
        ("card_scroll", "icon_spacer") => "card_scroll_icon_spacer",
        ("card_scroll", "dot") => "card_scroll_dot",
        ("card_scroll", "top_row") => "card_scroll_top_row",
        ("card_scroll", "scroll_markers") => "card_scroll_markers",
        ("card_scroll", "scroll_up") => "card_scroll_up",
        ("card_scroll", "scroll_down") => "card_scroll_down",
        ("card_hover", "glyph") => "card_hover_glyph",
        ("card_click", "glyph") => "card_click_glyph",
        ("card_key", "glyph") => "card_key_glyph",
        ("card_key", "icon") => "card_key_icon",
        ("card_key", "icon_spacer") => "card_key_icon_spacer",
        ("card_key", "dot") => "card_key_dot",
        ("card_key", "top_row") => "card_key_top_row",
        _ => "proof_unknown",
    }
}

fn text_asset_id(content: &str, style: &TextStyle) -> Option<&'static str> {
    [TITLE_TEXT, SUBTITLE_TEXT, BODY_TEXT, HOVER_LABEL, CLICK_LABEL, SCROLL_LABEL, KEY_LABEL]
        .into_iter()
        .find(|spec| spec.content == content && spec.font_size as i32 == style.font_size.0)
        .map(|spec| spec.id)
}

fn text_asset_index(id: &str) -> usize {
    match id {
        "proof_title" => 0,
        "proof_subtitle" => 1,
        "proof_body" => 2,
        "card_hover_label" => 3,
        "card_click_label" => 4,
        "card_scroll_label" => 5,
        "card_key_label" => 6,
        _ => usize::MAX,
    }
}

fn proof_text_asset_by_index(index: usize) -> Option<crate::assets::ProofTextAsset> {
    let id = match index {
        0 => "proof_title",
        1 => "proof_subtitle",
        2 => "proof_body",
        3 => "card_hover_label",
        4 => "card_click_label",
        5 => "card_scroll_label",
        6 => "card_key_label",
        _ => return None,
    };
    assets::proof_text_asset(id)
}
