// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! `Banner` — the design-system inline status strip (handoff `Banner`,
//! ArkUI `ExceptionPrompt` counterpart): a glass bar with a status accent
//! stripe, optional title + message, an optional action label and a dismiss
//! affordance. INLINE (it takes layout space) — the floating sibling is
//! `Toast`. A pure `LayoutNode` builder from theme tokens. DSL-emittable.

extern crate alloc;

use alloc::string::String;
use nexus_layout_types::{
    Align, CornerRadius, Direction, EdgeInsets, FlexItem, FxPx, Justify, LayoutNode, Overflow,
    Rgba8, Stack, VisualStyle,
};
use nexus_style::Style;
use nexus_theme_tokens::{ColorToken, LengthToken, MaterialToken, Tokens, TypographyToken};
use nexus_widget_panel::Panel;
use nexus_widget_text::Text;

/// Status accent (handoff `BannerProps.variant`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BannerVariant {
    #[default]
    Info,
    Success,
    Warning,
    Destructive,
}

impl BannerVariant {
    fn accent(self) -> ColorToken {
        match self {
            Self::Info => ColorToken::Info,
            Self::Success => ColorToken::Success,
            Self::Warning => ColorToken::Warning,
            Self::Destructive => ColorToken::Danger,
        }
    }
}

/// Width of the leading status stripe.
const STRIPE: i32 = 3;

/// An inline status/notification strip.
#[derive(Debug, Clone, Default)]
pub struct Banner {
    title: Option<String>,
    message: Option<String>,
    variant: BannerVariant,
    /// Leading icon slot (overrides the default status stripe emphasis).
    icon: Option<LayoutNode>,
    action: Option<String>,
    dismissible: bool,
    id: Option<&'static str>,
    action_id: Option<&'static str>,
    dismiss_id: Option<&'static str>,
}

impl Banner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }

    pub fn variant(mut self, variant: BannerVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn icon(mut self, icon: LayoutNode) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Action label (handoff `action`); give it an id for hit-testing.
    pub fn action(mut self, label: impl Into<String>) -> Self {
        self.action = Some(label.into());
        self
    }

    /// Shows the dismiss affordance (handoff `onDismiss` presence).
    pub fn dismissible(mut self, on: bool) -> Self {
        self.dismissible = on;
        self
    }

    pub fn id(mut self, id: &'static str) -> Self {
        self.id = Some(id);
        self
    }

    pub fn action_id(mut self, id: &'static str) -> Self {
        self.action_id = Some(id);
        self
    }

    pub fn dismiss_id(mut self, id: &'static str) -> Self {
        self.dismiss_id = Some(id);
        self
    }

    pub fn build(self, tokens: &dyn Tokens) -> LayoutNode {
        let g = tokens.glass(MaterialToken::Card);
        let accent = tokens.color(self.variant.accent());
        let mut style = Style::new()
            .background(g.tint)
            .rounded(tokens.length(LengthToken::RadiusMedium))
            .blur(g.blur_radius, g.saturation);
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
        // Leading status stripe (full-height accent bar).
        panel = panel.child(stripe_node(accent));
        if let Some(icon) = self.icon {
            panel = panel.child(icon);
        }
        // Title + message column.
        let mut text_col = Panel::column().gap(FxPx::new(2)).align(Align::Start);
        if let Some(title) = self.title {
            text_col = text_col.child(
                Text::new(title)
                    .size(TypographyToken::Base)
                    .weight(nexus_layout_types::FontWeight::Semibold)
                    .color(ColorToken::OnSurface)
                    .build(tokens),
            );
        }
        if let Some(message) = self.message {
            text_col = text_col.child(
                Text::new(message)
                    .size(TypographyToken::Sm)
                    .color(ColorToken::OnSurfaceVariant)
                    .build(tokens),
            );
        }
        panel = panel.child(grow(text_col.build()));
        if let Some(action) = self.action {
            panel = panel.child(tap_region(
                self.action_id,
                Text::new(action)
                    .size(TypographyToken::Base)
                    .color(ColorToken::Accent)
                    .build(tokens),
                tokens,
            ));
        }
        if self.dismissible {
            panel = panel.child(tap_region(
                self.dismiss_id,
                Text::new("\u{2715}")
                    .size(TypographyToken::Base)
                    .color(ColorToken::OnSurfaceVariant)
                    .build(tokens),
                tokens,
            ));
        }
        panel.build()
    }
}

/// The leading full-height accent stripe.
fn stripe_node(color: Rgba8) -> LayoutNode {
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
            min_width: Some(FxPx::new(STRIPE)),
            max_width: Some(FxPx::new(STRIPE)),
            min_height: Some(FxPx::new(28)),
            max_height: None,
            item: FlexItem { align_self: Some(Align::Stretch), ..FlexItem::default() },
        },
        VisualStyle {
            background: Some(color),
            corner_radius: CornerRadius::uniform(FxPx::new(STRIPE / 2)),
            ..VisualStyle::default()
        },
        alloc::vec![],
    )
}

/// A padded, id-addressable tap region around `content` (action / dismiss).
fn tap_region(id: Option<&'static str>, content: LayoutNode, tokens: &dyn Tokens) -> LayoutNode {
    let pad = tokens.length(LengthToken::SpacingSmall);
    LayoutNode::Stack(
        Stack {
            id,
            direction: Direction::Row,
            gap: FxPx::ZERO,
            padding: EdgeInsets { left: pad, right: pad, top: FxPx::new(2), bottom: FxPx::new(2) },
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
        alloc::vec![content],
    )
}

/// Wrap a node so it absorbs the row's free space (text column grows).
fn grow(node: LayoutNode) -> LayoutNode {
    match node {
        LayoutNode::Stack(mut s, v, c) => {
            s.item.flex_grow = 1;
            LayoutNode::Stack(s, v, c)
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_theme_tokens::BaseTokens;

    fn children_of(node: LayoutNode) -> alloc::vec::Vec<LayoutNode> {
        match node {
            LayoutNode::Stack(_, _, c) => c,
            _ => panic!("banner root must be a stack"),
        }
    }

    #[test]
    fn full_banner_has_stripe_text_action_dismiss() {
        let c = children_of(
            Banner::new()
                .variant(BannerVariant::Warning)
                .title("Speicher fast voll")
                .message("Noch 1,2 GB frei.")
                .action("Verwalten")
                .action_id("banner_action")
                .dismissible(true)
                .dismiss_id("banner_dismiss")
                .build(&BaseTokens),
        );
        assert_eq!(c.len(), 4, "stripe + text column + action + dismiss");
    }

    #[test]
    fn message_only_banner_is_minimal() {
        let c = children_of(
            Banner::new()
                .variant(BannerVariant::Success)
                .message("Synchronisierung abgeschlossen")
                .build(&BaseTokens),
        );
        assert_eq!(c.len(), 2, "stripe + text column");
    }
}
