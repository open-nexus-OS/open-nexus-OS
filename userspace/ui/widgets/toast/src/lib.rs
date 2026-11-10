// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

//! CONTEXT: `Toast` — the design-system transient notification (design artifacts `Toast`): an
//! opaque floating capsule on the overlay material (`surfaceOverlay`, the 1 px glass edge,
//! `shadow3`; never a blur), at most 440 px wide, padding 12×16, gap 12: the 8 px status dot,
//! the icon (accent or the status colour), the message (`body` 500) and the action (`body` 600
//! accent, a capsule tap target). This is the VIEW — showing, placement, motion and the
//! auto-dismiss belong to the host (`.overlay(transient)`, `.dismissAfter`). DSL-emittable.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: the tests below; tests/ui_v10_goldens (toast_*)
//! ADR: docs/rfcs/RFC-0116-one-token-set-the-design-artifacts-roles-text-styles-grid-shapes-and-materials.md

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use nexus_layout_types::{
    Align, CornerRadius, Direction, EdgeInsets, FlexItem, FontWeight, FxPx, Justify, LayoutNode,
    Material, Rgba8, ShapeKind, Stack, VisualStyle,
};
use nexus_style::Style;
use nexus_theme_tokens::{ColorToken, RadiusToken, TextStyleToken, Tokens};
use nexus_widget_text::Text;

/// Status (design `ToastProps.variant`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToastVariant {
    #[default]
    Default,
    Success,
    Warning,
    Destructive,
}

impl ToastVariant {
    /// The status colour (`None` for the default toast).
    pub fn role(self) -> Option<ColorToken> {
        match self {
            ToastVariant::Default => None,
            ToastVariant::Success => Some(ColorToken::Success),
            ToastVariant::Warning => Some(ColorToken::Warning),
            ToastVariant::Destructive => Some(ColorToken::Danger),
        }
    }
}

/// Diameter of the status dot.
const DOT: i32 = 8;
/// The widest a toast gets (the host also keeps it 16 px inside the viewport on each side).
pub const MAX_WIDTH: i32 = 440;

/// A transient notification capsule.
#[derive(Debug, Clone, Default)]
pub struct Toast {
    message: String,
    action: Option<String>,
    variant: ToastVariant,
    /// Leading icon (tinted accent, or the status colour).
    icon: Option<LayoutNode>,
    id: Option<&'static str>,
    action_id: Option<&'static str>,
}

impl Toast {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into(), ..Self::default() }
    }

    pub fn variant(mut self, variant: ToastVariant) -> Self {
        self.variant = variant;
        self
    }
    /// Action label (design `action`); give it an id for hit-testing.
    pub fn action(mut self, label: impl Into<String>) -> Self {
        self.action = Some(label.into());
        self
    }
    /// Leading icon node — a symbol node is recoloured to [`icon_role`](Self::icon_role).
    pub fn icon(mut self, icon: LayoutNode) -> Self {
        self.icon = Some(icon);
        self
    }
    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }
    /// Interaction id of the action (hit-testing).
    pub fn action_id(mut self, id: &'static str) -> Self {
        self.action_id = Some(id);
        self
    }

    /// The icon's colour: the status colour, else `accent`.
    pub fn icon_role(&self) -> ColorToken {
        self.variant.role().unwrap_or(ColorToken::Accent)
    }

    fn dot(color: Rgba8) -> LayoutNode {
        let d = Some(FxPx::new(DOT));
        LayoutNode::Stack(
            Stack {
                min_width: d,
                max_width: d,
                min_height: d,
                max_height: d,
                item: FlexItem { flex_shrink: 0, ..FlexItem::default() },
                ..Stack::default()
            },
            VisualStyle {
                background: Some(color),
                corner_radius: CornerRadius::uniform(RadiusToken::Full.px()),
                ..VisualStyle::default()
            },
            Vec::new(),
        )
    }

    /// The action: `body` 600 in `accent`, padding 2×4, a capsule.
    fn action_node(&self, label: &str, tokens: &dyn Tokens) -> LayoutNode {
        let text = Text::new(label)
            .text_style(TextStyleToken::Body)
            .weight(FontWeight::Semibold)
            .color(ColorToken::Accent)
            .build(tokens);
        LayoutNode::Stack(
            Stack {
                id: self.action_id,
                direction: Direction::Row,
                padding: EdgeInsets::symmetric(FxPx::new(2), FxPx::new(4)),
                align: Align::Center,
                justify: Justify::Center,
                item: FlexItem { flex_shrink: 0, ..FlexItem::default() },
                ..Stack::default()
            },
            VisualStyle {
                corner_radius: CornerRadius::uniform(RadiusToken::Full.px()),
                ..VisualStyle::default()
            },
            alloc::vec![text],
        )
    }

    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        // RFC-0116: the overlay material — opaque, the glass edge, shadow3; never a blur.
        let style =
            Style::new().material(Material::Overlay, tokens).rounded(RadiusToken::Full.px());
        let mut row: Vec<LayoutNode> = Vec::new();
        if let Some(role) = self.variant.role() {
            row.push(Self::dot(tokens.color(role)));
        }
        if let Some(mut icon) = self.icon.clone() {
            if let LayoutNode::Stack(_, v, _) = &mut icon {
                if v.shape != ShapeKind::Rect {
                    v.background = Some(tokens.color(self.icon_role()));
                }
            }
            row.push(icon);
        }
        let mut message = Text::new(self.message.as_str())
            .text_style(TextStyleToken::Body)
            .weight(FontWeight::Medium)
            .build(tokens);
        if let LayoutNode::Text(t, _) = &mut message {
            t.item.flex_grow = 1;
        }
        row.push(message);
        if let Some(action) = &self.action {
            row.push(self.action_node(action, tokens));
        }
        LayoutNode::Stack(
            Stack {
                id: self.id,
                direction: Direction::Row,
                gap: FxPx::new(12),
                padding: EdgeInsets::symmetric(FxPx::new(12), FxPx::new(16)),
                align: Align::Center,
                max_width: Some(FxPx::new(MAX_WIDTH)),
                ..Stack::default()
            },
            style.visual(),
            row,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_theme_tokens::{BaseTokens, DarkTokens};

    fn parts(node: &LayoutNode) -> (&Stack, &VisualStyle, &Vec<LayoutNode>) {
        match node {
            LayoutNode::Stack(s, v, c) => (s, v, c),
            _ => panic!("toast root must be a stack"),
        }
    }

    #[test]
    fn the_capsule_is_the_frosted_overlay_with_shadow3() {
        let t = DarkTokens;
        let node = Toast::new("Gespeichert").build(&t);
        let (s, v, kids) = parts(&node);
        assert_eq!(v.material, Some(Material::Overlay));
        // Frosted as a root (RFC-0116, 2026-11-10): `glassOverlay` over the regular blur.
        assert_eq!(v.background, Some(t.color(ColorToken::GlassOverlay)));
        assert_eq!(v.border.top.map(|b| b.color), Some(t.color(ColorToken::GlassEdge)));
        let s3 = Style::new().shadow_token(&t, nexus_theme_tokens::ShadowToken::Shadow3);
        assert_eq!(v.shadow, s3.visual().shadow);
        assert_eq!(v.corner_radius.top_left, RadiusToken::Full.px(), "a capsule");
        assert_eq!(s.padding, EdgeInsets::symmetric(FxPx::new(12), FxPx::new(16)));
        assert_eq!(s.gap, FxPx::new(12));
        assert_eq!(s.max_width, Some(FxPx::new(440)));
        assert_eq!(kids.len(), 1, "message only");
        match &kids[0] {
            LayoutNode::Text(m, _) => {
                assert_eq!(m.style.font_weight, FontWeight::Medium);
                assert_eq!(m.style.font_size, FxPx::new(14));
                assert_eq!(m.item.flex_grow, 1);
            }
            _ => panic!("message"),
        }
    }

    #[test]
    fn status_dot_then_icon_then_message_then_action() {
        let t = BaseTokens;
        let icon = LayoutNode::Stack(
            Stack::default(),
            VisualStyle { shape: ShapeKind::Circle, ..VisualStyle::default() },
            Vec::new(),
        );
        let node = Toast::new("Offline")
            .variant(ToastVariant::Destructive)
            .icon(icon)
            .action("Erneut")
            .action_id("toast_retry")
            .build(&t);
        let (_, _, kids) = parts(&node);
        assert_eq!(kids.len(), 4);
        let (dot, dv, _) = parts(&kids[0]);
        assert_eq!(dot.min_width, Some(FxPx::new(8)));
        assert_eq!(dv.background, Some(t.color(ColorToken::Danger)));
        assert_eq!(
            parts(&kids[1]).1.background,
            Some(t.color(ColorToken::Danger)),
            "icon in the status colour"
        );
        let (action, av, label) = parts(&kids[3]);
        assert_eq!(action.id, Some("toast_retry"));
        assert_eq!(action.padding, EdgeInsets::symmetric(FxPx::new(2), FxPx::new(4)));
        assert_eq!(av.corner_radius.top_left, RadiusToken::Full.px());
        assert!(
            matches!(&label[0], LayoutNode::Text(a, _) if a.style.font_weight == FontWeight::Semibold && a.style.color == t.color(ColorToken::Accent))
        );
    }

    #[test]
    fn the_default_icon_is_accent() {
        assert_eq!(Toast::new("x").icon_role(), ColorToken::Accent);
        assert_eq!(Toast::new("x").variant(ToastVariant::Success).icon_role(), ColorToken::Success);
    }
}
