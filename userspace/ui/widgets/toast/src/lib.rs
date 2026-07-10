// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! `Toast` — the design-system transient notification (handoff `Toast`): a
//! floating frosted pill with a status accent dot, message, and an optional
//! action label. This is the VIEW (a pure `LayoutNode` builder on the dense
//! overlay glass material); showing, auto-dismiss timing and placement belong
//! to the notification manager (TASK-0074 five-surface routing).
//! DSL-emittable.

extern crate alloc;

use alloc::string::String;
use nexus_layout_types::{
    Align, BoxShadow, CornerRadius, Direction, EdgeInsets, FlexItem, FxPx, Justify, LayoutNode,
    Overflow, Rgba8, Stack, VisualStyle,
};
use nexus_style::Style;
use nexus_theme_tokens::{ColorToken, LengthToken, MaterialToken, Tokens, TypographyToken};
use nexus_widget_panel::Panel;
use nexus_widget_text::Text;

/// Status accent of the leading dot (handoff `ToastProps.variant`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToastVariant {
    #[default]
    Default,
    Success,
    Warning,
    Destructive,
}

/// Diameter of the status dot.
const DOT: i32 = 8;
/// Pill corner radius (a full pill; the radius token scale tops out lower).
const PILL_RADIUS: i32 = 22;

/// A transient notification pill.
#[derive(Debug, Clone, Default)]
pub struct Toast {
    message: String,
    action: Option<String>,
    variant: ToastVariant,
    /// Leading icon slot (e.g. `Icon::lucide(...)`); drawn before the dot.
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

    /// Action label (handoff `action`); give it an id for hit-testing.
    pub fn action(mut self, label: impl Into<String>) -> Self {
        self.action = Some(label.into());
        self
    }

    /// Leading icon node.
    pub fn icon(mut self, icon: LayoutNode) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    /// Interaction id of the action label (hit-testing).
    pub fn action_id(mut self, id: &'static str) -> Self {
        self.action_id = Some(id);
        self
    }

    fn dot_color(&self, tokens: &dyn Tokens) -> Option<Rgba8> {
        Some(tokens.color(match self.variant {
            ToastVariant::Default => return None,
            ToastVariant::Success => ColorToken::Success,
            ToastVariant::Warning => ColorToken::Warning,
            ToastVariant::Destructive => ColorToken::Danger,
        }))
    }

    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let dot = self.dot_color(tokens);
        let g = tokens.glass(MaterialToken::Overlay);
        let mut style = Style::new()
            .background(g.tint)
            .rounded(FxPx::new(PILL_RADIUS))
            .blur(g.blur_radius, g.saturation)
            .shadow(BoxShadow {
                offset_x: FxPx::ZERO,
                offset_y: FxPx::new(6),
                blur_radius: FxPx::new(18),
                spread: FxPx::ZERO,
                color: tokens.color(ColorToken::Shadow),
            });
        if let Some(border) = g.border {
            style = style.border(tokens.length(LengthToken::BorderThin), border);
        }
        let mut panel = Panel::row()
            .style(style)
            .padding(tokens.length(LengthToken::SpacingMedium))
            .gap(tokens.length(LengthToken::SpacingSmall))
            .align(Align::Center);
        if let Some(id) = self.id {
            panel = panel.id(id);
        }
        if let Some(icon) = self.icon {
            panel = panel.child(icon);
        }
        if let Some(dot) = dot {
            panel = panel.child(dot_node(dot));
        }
        panel = panel.child(
            Text::new(self.message)
                .size(TypographyToken::Base)
                .color(ColorToken::OnSurface)
                .build(tokens),
        );
        if let Some(action) = self.action {
            let label = Text::new(action)
                .size(TypographyToken::Base)
                .color(ColorToken::Accent)
                .build(tokens);
            let pad = tokens.length(LengthToken::SpacingSmall);
            panel = panel.child(LayoutNode::Stack(
                Stack {
                    id: self.action_id,
                    direction: Direction::Row,
                    gap: FxPx::ZERO,
                    padding: EdgeInsets {
                        left: pad,
                        right: pad,
                        top: FxPx::new(2),
                        bottom: FxPx::new(2),
                    },
                    align: Align::Center,
                    justify: Justify::Center,
                    overflow: Overflow::Visible,
                    flex_wrap: false,
                    min_width: None,
                    max_width: None,
                    min_height: None,
                    max_height: None,
                    item: FlexItem::default(),
                },
                VisualStyle::default(),
                alloc::vec![label],
            ));
        }
        panel.build()
    }
}

/// The status dot node.
fn dot_node(color: Rgba8) -> LayoutNode {
    let d = Some(FxPx::new(DOT));
    LayoutNode::Stack(
        Stack {
            id: None,
            direction: Direction::Row,
            gap: FxPx::ZERO,
            padding: EdgeInsets::zero(),
            align: Align::Center,
            justify: Justify::Center,
            overflow: Overflow::Visible,
            flex_wrap: false,
            min_width: d,
            max_width: d,
            min_height: d,
            max_height: d,
            item: FlexItem::default(),
        },
        VisualStyle {
            background: Some(color),
            corner_radius: CornerRadius::uniform(FxPx::new(DOT / 2)),
            ..VisualStyle::default()
        },
        alloc::vec![],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_theme_tokens::BaseTokens;

    fn children_of(node: LayoutNode) -> alloc::vec::Vec<LayoutNode> {
        match node {
            LayoutNode::Stack(_, _, c) => c,
            _ => panic!("toast root must be a stack"),
        }
    }

    #[test]
    fn default_variant_has_no_dot() {
        let c = children_of(Toast::new("Saved").build(&BaseTokens));
        assert_eq!(c.len(), 1, "message only");
    }

    #[test]
    fn status_variant_prepends_a_dot_and_action_appends() {
        let c = children_of(
            Toast::new("Offline")
                .variant(ToastVariant::Destructive)
                .action("Retry")
                .action_id("toast_retry")
                .build(&BaseTokens),
        );
        assert_eq!(c.len(), 3, "dot + message + action");
    }
}
