// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! GlassButton geometry, looks and states, laid out by the real engine.

use super::*;
use alloc::vec;
use nexus_layout::{LayoutBox, LayoutEngine};
use nexus_layout_types::{
    FontWeight, LineLayout, LineMetrics, MeasureText, PreparedTextHandle, ShapeKind, Spacer,
    TextStyle,
};
use nexus_style::FOCUS_RING;
use nexus_theme_tokens::{BaseTokens, DarkTokens, ShadowToken};

/// 7 px per character, one line of the style's line height.
struct Fixed;

impl MeasureText for Fixed {
    fn prepare(&self, content: &TextContent, style: &TextStyle) -> PreparedTextHandle {
        let line = style.line_height.effective(style.font_size).0 as usize;
        PreparedTextHandle(content.as_str().chars().count() * 7 * 1000 + line)
    }
    fn measure_width(&self, handle: &PreparedTextHandle) -> FxPx {
        FxPx::new((handle.0 / 1000) as i32)
    }
    fn layout_lines(&self, handle: &PreparedTextHandle, _w: FxPx, _max: Option<u32>) -> LineLayout {
        let (width, line) =
            (FxPx::new((handle.0 / 1000) as i32), FxPx::new((handle.0 % 1000) as i32));
        LineLayout {
            lines: vec![LineMetrics { text_range: 0..0, width, baseline: line, height: line }],
            natural_width: width,
        }
    }
}

fn boxes(node: &LayoutNode) -> Vec<LayoutBox> {
    LayoutEngine::new().layout(node, FxPx::new(800), &Fixed).expect("lays out").boxes
}

/// The node's own size: laid out inside a hugging row (the engine gives the scene root the
/// whole viewport width).
fn root(node: &LayoutNode) -> (i32, i32) {
    let b = &boxes(&hug(node.clone()))[1];
    (b.rect.width.0, b.rect.height.0)
}

fn hug(node: LayoutNode) -> LayoutNode {
    LayoutNode::Stack(
        Stack { direction: Direction::Row, align: Align::Start, ..Stack::default() },
        VisualStyle::default(),
        vec![node],
    )
}

fn visual(node: &LayoutNode) -> &VisualStyle {
    match node {
        LayoutNode::Stack(_, v, _) => v,
        _ => panic!("a button is a Stack"),
    }
}

fn kids(node: &LayoutNode) -> &[LayoutNode] {
    match node {
        LayoutNode::Stack(_, _, c) => c,
        _ => panic!("expected a Stack"),
    }
}

fn icon() -> LayoutNode {
    LayoutNode::Spacer(Spacer {
        id: Some("icon"),
        flex_grow: 0,
        min_size: Some(FxPx::new(16)),
        ..Spacer::default()
    })
}

#[test]
fn one_outer_height_per_size_and_padding_0_by_pad_x() {
    let t = BaseTokens;
    for (size, h, pad) in [
        (ControlSize::Xs, 24, 8),
        (ControlSize::Sm, 30, 12),
        (ControlSize::Md, 36, 16),
        (ControlSize::Lg, 44, 20),
        (ControlSize::Xl, 52, 24),
    ] {
        let node = GlassButton::new().size(size).label("Öffnen").build(&t);
        let (w, height) = root(&node);
        assert_eq!(height, h, "{size:?}");
        assert_eq!(w, 6 * 7 + 2 * pad, "{size:?}: label + 0 × padX");
    }
}

#[test]
fn a_labelled_button_is_a_capsule_an_icon_only_one_a_circle() {
    let t = BaseTokens;
    let capsule = GlassButton::new().label("Senden").build(&t);
    assert_eq!(visual(&capsule).corner_radius.top_left, RadiusToken::Full.px());
    for size in [ControlSize::Sm, ControlSize::Lg] {
        let circle = GlassButton::new().size(size).icon(icon()).build(&t);
        let h = size.height().0;
        assert_eq!(root(&circle), (h, h), "{size:?}: width = height");
        assert_eq!(visual(&circle).corner_radius.top_left, RadiusToken::Full.px());
    }
    assert_eq!(
        root(&GlassButton::new().size(ButtonSize::Icon).content(icon()).build(&t)),
        (36, 36)
    );
    // The rounded exception takes the size's field radius.
    let tile =
        GlassButton::new().size(ControlSize::Xl).shape(ButtonShape::Rounded).label("A").build(&t);
    assert_eq!(visual(&tile).corner_radius.top_left, FxPx::new(16));
}

#[test]
fn the_widget_owns_its_label() {
    let t = DarkTokens;
    let node = GlassButton::new().label("Öffnen").build(&t);
    let content = &kids(&node)[0];
    match &kids(content)[0] {
        LayoutNode::Text(text, _) => {
            assert_eq!(text.content.as_str(), "Öffnen");
            assert_eq!(text.style.font_size, FxPx::new(14), "md = body");
            assert_eq!(text.style.font_weight, FontWeight::Medium);
            assert_eq!(text.style.color, t.color(ColorToken::Label), "glass label");
        }
        other => panic!("label expected, got {other:?}"),
    }
    let prominent = GlassButton::new().prominent(true).label("Senden").build(&t);
    match &kids(&kids(&prominent)[0])[0] {
        LayoutNode::Text(text, _) => assert_eq!(text.style.color, t.color(ColorToken::OnAccent)),
        _ => panic!(),
    }
}

#[test]
fn declared_children_follow_the_label_and_an_empty_label_keeps_its_place() {
    let t = BaseTokens;
    let node = GlassButton::new().label("").children(vec![icon()]).build(&t);
    let content = kids(&node)[0].clone();
    let row = kids(&content);
    assert_eq!(row.len(), 2);
    match &row[0] {
        LayoutNode::Text(text, _) => assert_eq!(text.item.position, Position::Absolute),
        _ => panic!("the label is child 0"),
    }
    assert_eq!(row[1].id(), Some("icon"));
    // The legacy caller content is child 0 itself.
    let legacy = GlassButton::new().content(icon()).build(&t);
    assert_eq!(kids(&legacy)[0].id(), Some("icon"));
}

#[test]
fn hover_and_press_never_repaint() {
    for t in [&BaseTokens as &dyn Tokens, &DarkTokens] {
        for variant in [ButtonVariant::Glass, ButtonVariant::Filled, ButtonVariant::Ghost] {
            let rest = GlassButton::new().variant(variant).style(t);
            for state in [InteractionState::Hover, InteractionState::Pressed] {
                assert_eq!(
                    GlassButton::new().variant(variant).state(state).style(t),
                    rest,
                    "{variant:?} {state:?}"
                );
            }
        }
    }
}

#[test]
fn disabled_dims_to_half_and_focus_draws_the_ring() {
    let t = BaseTokens;
    let disabled = GlassButton::new().state(InteractionState::Disabled).style(&t).visual();
    assert_eq!(disabled.opacity, Some(nexus_layout_types::Fraction::new(128)));
    let focused = GlassButton::new().state(InteractionState::Focused).style(&t).visual();
    let ring = focused.shadow.expect("the ring");
    assert_eq!(ring.spread, FOCUS_RING);
    assert_eq!(ring.color, t.color(ColorToken::Accent));
    assert!(focused.border.top.is_some(), "the glass edge stays");
}

#[test]
fn glass_carries_its_edge_shadow_and_lit_top_and_never_blurs() {
    let t = BaseTokens;
    let s = GlassButton::new().style(&t);
    let v = s.visual();
    assert_eq!(v.background, Some(t.color(ColorToken::GlassCard)));
    assert_eq!(v.border.top.map(|b| b.color), Some(t.color(ColorToken::GlassEdge)));
    assert_eq!(v.shadow, Some(nexus_style::box_shadow(t.shadow(ShadowToken::Shadow1))));
    assert_eq!(v.inset_highlight, Some(t.shadow(ShadowToken::EdgeHighlight).color));
    assert!(s.backdrop_blur().is_none(), "one blur per stack: a control never blurs");
    assert_eq!(v.material, None);
}

#[test]
fn the_tinted_look_paints_the_16_percent_wash() {
    let t = BaseTokens;
    let node = GlassButton::new()
        .variant(ButtonVariant::Tinted)
        .tint(ColorToken::Danger)
        .label("x")
        .build(&t);
    let danger = t.color(ColorToken::Danger);
    let bg = visual(&node).background.expect("the wash");
    assert_eq!((bg.r, bg.g, bg.b, bg.a), (danger.r, danger.g, danger.b, 41));
}

#[test]
fn close_and_confirm_without_content_draw_their_glyph_in_a_circle() {
    let t = BaseTokens;
    let close = GlassButton::new().role(Role::Close).build(&t);
    assert_eq!(root(&close), (36, 36));
    let glyph = &kids(&kids(&close)[0])[0];
    match glyph {
        LayoutNode::Stack(stack, v, _) => {
            assert!(matches!(v.shape, ShapeKind::Stroke { .. }));
            assert_eq!(stack.min_width, Some(FxPx::new(14)), "✕ = round(16 × 0.85)");
            assert_eq!(v.background, Some(t.color(ColorToken::Label)));
        }
        _ => panic!("the ✕ glyph"),
    }
    let confirm = GlassButton::new().role(Role::Confirm).build(&t);
    assert_eq!(visual(&confirm).background, Some(t.color(ColorToken::Accent)), "confirm fills");
}

#[test]
fn a_tile_stacks_icon_over_a_smaller_label() {
    let t = BaseTokens;
    let node =
        GlassButton::new().icon(icon()).icon_position(IconPosition::Top).label("Fotos").build(&t);
    let (_, h) = root(&node);
    assert_eq!(h, 58, "min-h round(36 × 1.6)");
    assert_eq!(visual(&node).corner_radius.top_left, RadiusToken::Lg.px());
    match &kids(&node)[0] {
        LayoutNode::Stack(stack, _, row) => {
            assert_eq!(stack.direction, Direction::Column);
            match &row[1] {
                LayoutNode::Text(text, _) => {
                    assert_eq!(text.style.font_size, FxPx::new(12), "footnote")
                }
                _ => panic!("label after the icon"),
            }
        }
        _ => panic!(),
    }
}

#[test]
fn full_width_stretches_but_keeps_the_height() {
    let t = BaseTokens;
    let column = LayoutNode::Stack(
        Stack {
            min_width: Some(FxPx::new(300)),
            max_width: Some(FxPx::new(300)),
            min_height: Some(FxPx::new(200)),
            max_height: Some(FxPx::new(200)),
            ..Stack::default()
        },
        VisualStyle::default(),
        vec![GlassButton::new().full_width(true).label("Weiter").build(&t)],
    );
    let b = boxes(&hug(column));
    assert_eq!((b[2].rect.width.0, b[2].rect.height.0), (300, 36));
}

#[test]
fn build_keeps_the_interaction_id() {
    let t = BaseTokens;
    match GlassButton::new().id("open").label("Öffnen").build(&t) {
        LayoutNode::Stack(stack, visual, _) => {
            assert_eq!(stack.id, Some("open"));
            assert!(visual.background.is_some());
        }
        _ => panic!("GlassButton must build a Stack"),
    }
}
