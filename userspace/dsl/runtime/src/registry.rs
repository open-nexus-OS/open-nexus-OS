// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Token vocabulary mapping + widget builders (kind symbol → `LayoutNode`).
//!
//! One deliberate seam: everything the emitter knows about *rendering* a
//! widget kind lives here, so promoting a kind to a richer kit component (or
//! adding one) never touches the walker.

use crate::store::Value;
use alloc::string::String;
use nexus_layout_types::{
    Align, CornerRadius, Direction, EdgeInsets, FlexItem, FxPx, Justify, LayoutNode, Overflow,
    Spacer, Stack, TextContent, TextNode, TextStyle, VisualStyle,
};
use nexus_theme_tokens::{ColorToken, Tokens, TypographyToken};

/// Spacing scale: one step = 4px (matches the theme spacing scale).
#[must_use]
pub fn spacing(step: i64) -> FxPx {
    FxPx::new((step.clamp(0, 256) * 4) as i32)
}

#[must_use]
pub fn color_token(name: &str) -> Option<ColorToken> {
    Some(match name {
        "surface" => ColorToken::Surface,
        "surfaceVariant" => ColorToken::SurfaceVariant,
        "onSurface" => ColorToken::OnSurface,
        "onSurfaceVariant" => ColorToken::OnSurfaceVariant,
        "accent" => ColorToken::Accent,
        "onAccent" => ColorToken::OnAccent,
        "border" => ColorToken::Border,
        "background" => ColorToken::Background,
        "primary" => ColorToken::Primary,
        "onPrimary" => ColorToken::OnPrimary,
        "danger" => ColorToken::Danger,
        _ => return None,
    })
}

#[must_use]
pub fn type_size(name: &str) -> Option<TypographyToken> {
    Some(match name {
        "xs" => TypographyToken::Xs,
        "sm" => TypographyToken::Sm,
        "base" => TypographyToken::Base,
        "md" => TypographyToken::Md,
        "lg" => TypographyToken::Lg,
        "xl" => TypographyToken::Xl,
        _ => return None,
    })
}

#[must_use]
pub fn radius(name: &str) -> FxPx {
    FxPx::new(match name {
        "sm" => 4,
        "md" => 8,
        "lg" => 12,
        "xl" => 16,
        "full" => 9999,
        _ => 0,
    })
}

/// Layout/paint configuration accumulated from a node's modifiers.
pub struct Mods {
    pub padding: EdgeInsets,
    pub gap: FxPx,
    pub direction: Option<Direction>,
    pub align: Option<Align>,
    pub justify: Option<Justify>,
    pub bg: Option<ColorToken>,
    pub fg: Option<ColorToken>,
    pub rounded: Option<FxPx>,
    pub text_size: Option<TypographyToken>,
    pub opacity: Option<u8>,
    pub disabled: bool,
}

impl Default for Mods {
    fn default() -> Self {
        Self {
            padding: EdgeInsets::zero(),
            gap: FxPx::ZERO,
            direction: None,
            align: None,
            justify: None,
            bg: None,
            fg: None,
            rounded: None,
            text_size: None,
            opacity: None,
            disabled: false,
        }
    }
}

impl Mods {
    /// The paint part as a `VisualStyle`.
    pub fn visual(&self, tokens: &dyn Tokens) -> VisualStyle {
        let mut visual = VisualStyle::default();
        if let Some(bg) = self.bg {
            visual.background = Some(tokens.color(bg));
        }
        if let Some(rounded) = self.rounded {
            visual.corner_radius = CornerRadius::uniform(rounded);
        }
        if let Some(opacity) = self.opacity {
            visual.opacity = Some(nexus_layout_types::Fraction(u32::from(opacity)));
        }
        if self.disabled {
            // The InteractionState::Disabled wash (140/255).
            visual.opacity = Some(nexus_layout_types::Fraction(140));
        }
        visual
    }
}

fn plain_stack(mods: &Mods, tokens: &dyn Tokens, children: alloc::vec::Vec<LayoutNode>) -> LayoutNode {
    LayoutNode::Stack(
        Stack {
            id: None,
            direction: mods.direction.unwrap_or(Direction::Column),
            gap: mods.gap,
            padding: mods.padding,
            align: mods.align.unwrap_or(Align::Stretch),
            justify: mods.justify.unwrap_or(Justify::Start),
            overflow: Overflow::Visible,
            flex_wrap: false,
            min_width: None,
            max_width: None,
            min_height: None,
            max_height: None,
            item: FlexItem::default(),
        },
        mods.visual(tokens),
        children,
    )
}

fn text_node(value: String, mods: &Mods, tokens: &dyn Tokens) -> LayoutNode {
    let mut style = TextStyle {
        color: tokens.color(mods.fg.unwrap_or(ColorToken::OnSurface)),
        ..TextStyle::default()
    };
    if let Some(size) = mods.text_size {
        style.font_size = tokens.type_size(size);
    }
    LayoutNode::Text(
        TextNode {
            id: None,
            content: TextContent(value),
            style,
            item: FlexItem::default(),
            max_lines: None,
            min_width: None,
            max_width: None,
        },
        mods.visual(tokens),
    )
}

fn value_text(value: &Value) -> String {
    match value {
        Value::Str(s) => s.clone(),
        Value::Int(i) => alloc::format!("{i}"),
        Value::Bool(b) => String::from(if *b { "true" } else { "false" }),
        Value::Fx(raw) => alloc::format!("{}", raw >> 32),
        _ => String::new(),
    }
}

/// Builds one widget kind. `primary` = the `value`-like prop (resolved),
/// `props` = (name symbol text, value), `children` already emitted.
pub fn build_widget(
    kind: &str,
    props: &[(String, Value)],
    mods: &Mods,
    tokens: &dyn Tokens,
    children: alloc::vec::Vec<LayoutNode>,
) -> LayoutNode {
    let prop = |name: &str| props.iter().find(|(n, _)| n == name).map(|(_, v)| v);
    match kind {
        "Stack" | "List" => plain_stack(mods, tokens, children),
        "Card" => {
            // Kit promotion: GlassCard (Panel + material tokens) is the SSOT.
            let mut card = nexus_widget_card::GlassCard::new()
                .padding(if mods.padding == EdgeInsets::zero() {
                    spacing(3)
                } else {
                    mods.padding.top
                });
            if mods.direction == Some(Direction::Row) {
                card = card.row();
            }
            for child in children {
                card = card.child(child);
            }
            card.build(tokens)
        }
        "Spacer" => LayoutNode::Spacer(Spacer::default()),
        "Text" => {
            let value = prop("value").map(value_text).unwrap_or_default();
            text_node(value, mods, tokens)
        }
        "Button" => {
            // Kit promotion: the design-system GlassButton is the SSOT for
            // button visuals; DSL modifiers select variant/state, the kit
            // decides the look. Structure: root → content stack (index 0) →
            // label text (0) + declared children (1+) — `child_path_prefix`
            // mirrors this for handler/child paths.
            let label = prop("label").map(value_text).unwrap_or_default();
            let variant = if mods.bg == Some(ColorToken::Danger) {
                nexus_widget_button::ButtonVariant::Destructive
            } else if mods.bg == Some(ColorToken::SurfaceVariant) {
                nexus_widget_button::ButtonVariant::Secondary
            } else {
                nexus_widget_button::ButtonVariant::Default
            };
            let state = if mods.disabled {
                nexus_style::InteractionState::Disabled
            } else {
                nexus_style::InteractionState::Default
            };
            let label_mods = Mods {
                fg: Some(mods.fg.unwrap_or(ColorToken::OnAccent)),
                text_size: mods.text_size,
                ..Mods::default()
            };
            let mut content = alloc::vec![text_node(label, &label_mods, tokens)];
            content.extend(children);
            let content_mods = Mods {
                direction: Some(Direction::Row),
                align: Some(Align::Center),
                gap: mods.gap,
                ..Mods::default()
            };
            nexus_widget_button::GlassButton::new()
                .variant(variant)
                .state(state)
                .content(plain_stack(&content_mods, tokens, content))
                .build(tokens)
        }
        "TextField" => {
            // Kit promotion: GlassTextField (label + field + focus tokens).
            let mut field = nexus_widget_text_field::GlassTextField::new();
            if let Some(label) = prop("label").map(value_text) {
                field = field.label(label);
            }
            if let Some(value) = prop("value").map(value_text).filter(|v| !v.is_empty()) {
                field = field.value(value);
            }
            if let Some(placeholder) = prop("placeholder").map(value_text) {
                field = field.placeholder(placeholder);
            }
            field.build(tokens)
        }
        "Toggle" => {
            let checked = matches!(prop("checked"), Some(Value::Bool(true)));
            let mut toggle = nexus_widget_toggle::GlassToggle::new().checked(checked);
            if mods.disabled {
                toggle = toggle.state(nexus_style::InteractionState::Disabled);
            }
            toggle.build(tokens)
        }
        "Icon" => {
            // Kit promotion: the vector Icon primitive (symbol name → Symbol).
            let symbol = match prop("symbol").map(value_text).as_deref() {
                Some("plus") => Some(nexus_widget_icon::Symbol::Plus),
                Some("minus") => Some(nexus_widget_icon::Symbol::Minus),
                Some("close") => Some(nexus_widget_icon::Symbol::Close),
                Some("star") => Some(nexus_widget_icon::Symbol::Star),
                Some("chevronRight") => Some(nexus_widget_icon::Symbol::ChevronRight),
                Some("chevronLeft") => Some(nexus_widget_icon::Symbol::ChevronLeft),
                Some("chevronDown") => Some(nexus_widget_icon::Symbol::ChevronDown),
                Some("chevronUp") => Some(nexus_widget_icon::Symbol::ChevronUp),
                _ => None,
            };
            match symbol {
                Some(symbol) => nexus_widget_icon::Icon::new(symbol)
                    .size(16)
                    .color(mods.fg.unwrap_or(ColorToken::OnSurfaceVariant))
                    .build(tokens),
                None => {
                    // Unknown symbol: honest tinted placeholder box.
                    let box_mods = Mods {
                        bg: Some(mods.fg.unwrap_or(ColorToken::OnSurfaceVariant)),
                        rounded: Some(FxPx::new(3)),
                        padding: EdgeInsets::all(FxPx::new(8)),
                        ..Mods::default()
                    };
                    plain_stack(&box_mods, tokens, alloc::vec![])
                }
            }
        }
        _ => plain_stack(mods, tokens, children),
    }
}

/// Where a kind's *declared* children live inside the produced tree:
/// (path prefix from the widget root, index of the first declared child).
/// Mirrors the kit builders' structure — update together with `build_widget`.
#[must_use]
pub fn child_path(kind: &str) -> (&'static [u32], u32) {
    match kind {
        // GlassButton: root → content stack (0) → label (0), children (1+).
        "Button" => (&[0], 1),
        _ => (&[], 0),
    }
}
