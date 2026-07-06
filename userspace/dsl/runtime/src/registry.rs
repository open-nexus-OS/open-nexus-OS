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
        "Stack" | "Card" | "List" => {
            // Card gets a surface + radius default when unstyled.
            let mut mods_out = Mods {
                padding: mods.padding,
                gap: mods.gap,
                direction: mods.direction,
                align: mods.align,
                justify: mods.justify,
                bg: mods.bg.or(if kind == "Card" { Some(ColorToken::Surface) } else { None }),
                fg: mods.fg,
                rounded: mods
                    .rounded
                    .or(if kind == "Card" { Some(FxPx::new(8)) } else { None }),
                text_size: mods.text_size,
                opacity: mods.opacity,
                disabled: mods.disabled,
            };
            if kind == "Card" && mods.padding == EdgeInsets::zero() {
                mods_out.padding = EdgeInsets::all(spacing(3));
            }
            plain_stack(&mods_out, tokens, children)
        }
        "Spacer" => LayoutNode::Spacer(Spacer::default()),
        "Text" => {
            let value = prop("value").map(value_text).unwrap_or_default();
            text_node(value, mods, tokens)
        }
        "Button" => {
            let label = prop("label").map(value_text).unwrap_or_default();
            let mut inner = Mods::default();
            inner.fg = Some(mods.fg.unwrap_or(ColorToken::OnAccent));
            inner.text_size = mods.text_size;
            let mut wrapper = Mods {
                padding: if mods.padding == EdgeInsets::zero() {
                    EdgeInsets::symmetric(spacing(2), spacing(4))
                } else {
                    mods.padding
                },
                gap: mods.gap,
                direction: Some(Direction::Row),
                align: Some(Align::Center),
                justify: Some(Justify::Center),
                bg: Some(mods.bg.unwrap_or(ColorToken::Accent)),
                fg: mods.fg,
                rounded: Some(mods.rounded.unwrap_or(FxPx::new(8))),
                text_size: None,
                opacity: mods.opacity,
                disabled: mods.disabled,
            };
            if wrapper.disabled {
                wrapper.opacity = Some(140);
            }
            let mut content = alloc::vec![text_node(label, &inner, tokens)];
            content.extend(children);
            plain_stack(&wrapper, tokens, content)
        }
        "TextField" => {
            let label = prop("label").map(value_text).unwrap_or_default();
            let value = prop("value")
                .map(value_text)
                .filter(|v| !v.is_empty())
                .or_else(|| prop("placeholder").map(value_text))
                .unwrap_or_default();
            let label_mods = Mods {
                fg: Some(ColorToken::OnSurfaceVariant),
                text_size: Some(TypographyToken::Sm),
                ..Mods::default()
            };
            let field_mods = Mods {
                padding: EdgeInsets::all(spacing(2)),
                bg: Some(ColorToken::SurfaceVariant),
                rounded: Some(FxPx::new(6)),
                ..Mods::default()
            };
            let field = plain_stack(
                &field_mods,
                tokens,
                alloc::vec![text_node(value, &Mods::default(), tokens)],
            );
            let column = Mods { gap: spacing(1), ..copy_layout(mods) };
            plain_stack(&column, tokens, alloc::vec![
                text_node(label, &label_mods, tokens),
                field,
            ])
        }
        "Toggle" => {
            let checked = matches!(prop("checked"), Some(Value::Bool(true)));
            let track = Mods {
                padding: EdgeInsets::all(FxPx::new(2)),
                direction: Some(Direction::Row),
                justify: Some(if checked { Justify::End } else { Justify::Start }),
                bg: Some(if checked { ColorToken::Accent } else { ColorToken::SurfaceVariant }),
                rounded: Some(FxPx::new(10)),
                ..copy_layout(mods)
            };
            let knob = Mods {
                bg: Some(ColorToken::Surface),
                rounded: Some(FxPx::new(8)),
                padding: EdgeInsets::all(FxPx::new(8)),
                ..Mods::default()
            };
            plain_stack(&track, tokens, alloc::vec![plain_stack(&knob, tokens, alloc::vec![])])
        }
        "Icon" => {
            // A tinted square placeholder box; the vector symbol path is the
            // in-compositor mount's job (ShapeKind rendering, TASK-0076B+).
            let box_mods = Mods {
                bg: Some(mods.fg.unwrap_or(ColorToken::OnSurfaceVariant)),
                rounded: Some(FxPx::new(3)),
                padding: EdgeInsets::all(FxPx::new(8)),
                ..Mods::default()
            };
            plain_stack(&box_mods, tokens, alloc::vec![])
        }
        _ => plain_stack(mods, tokens, children),
    }
}

fn copy_layout(mods: &Mods) -> Mods {
    Mods {
        padding: mods.padding,
        gap: mods.gap,
        direction: mods.direction,
        align: mods.align,
        justify: mods.justify,
        opacity: mods.opacity,
        disabled: mods.disabled,
        ..Mods::default()
    }
}
