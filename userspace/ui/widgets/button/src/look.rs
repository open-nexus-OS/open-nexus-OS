// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The looks of the [`GlassButton`](crate::GlassButton) (the design artifacts' `GlassButton`
//! look resolution, rule for rule): which fill, edge, label colour and shadow a variant, a tint,
//! a role and the selected state come to. Hover and press are not looks — the host's layer.

use nexus_layout_types::{BoxShadow, Rgba8};
use nexus_style::box_shadow;
use nexus_style::control::{glow_shadow, on_tint, over, wash, Glow, Role, Wash};
use nexus_theme_tokens::{ColorToken, ShadowToken, Tokens};

/// The button's look (the design's `variant`). No look ever blurs: the blur belongs to the
/// surface the button sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonVariant {
    /// Translucent `glassCard` with the glass edge, `shadow1` and the lit top edge — the
    /// design's default. The label takes the tint when one is set.
    #[default]
    Glass,
    /// No background and no edge (borderless). The label takes the tint when one is set.
    Ghost,
    /// The neutral `fill` with a `separator` edge. The label takes the tint when one is set.
    Secondary,
    /// The tint's 16 % wash with a tint label.
    Tinted,
    /// The tint fill with its on-colour label and the medium glow (the prominent look).
    Filled,
    /// Legacy name: filled `primary` (a set tint or role overrides it), no glow.
    Default,
    /// Legacy name: filled in the tint (accent) with the large glow — an "on" button.
    Active,
    /// Legacy name: filled `danger` (a set tint overrides it), no glow.
    Destructive,
}

/// The quiet and filled families the variants resolve to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Glass,
    Ghost,
    Secondary,
    Tinted,
    Filled,
}

/// A resolved look: everything the button paints apart from its geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Look {
    pub background: Option<Rgba8>,
    /// The 1 px edge (`None` = the design's transparent border, which paints nothing).
    pub edge: Option<ColorToken>,
    /// The label and icon colour.
    pub label: ColorToken,
    pub shadow: Option<BoxShadow>,
    /// The lit top edge (`edgeHighlight`).
    pub edge_highlight: bool,
    /// The fill is a tint (the host's hover layer lightens instead of darkening).
    pub on_fill: bool,
}

/// What a look is resolved from.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct LookInput {
    pub variant: Option<ButtonVariant>,
    pub prominent: bool,
    pub tint: Option<ColorToken>,
    pub role: Option<Role>,
    pub selected: bool,
}

/// Resolve a look against the theme (the artifact's rules, in their order).
pub(crate) fn resolve(input: LookInput, tokens: &dyn Tokens) -> Look {
    // An explicit tint wins, then the role's; quiet looks colour their label only then.
    let explicit = input.tint.or(input.role.and_then(Role::tint));
    let tint_set = explicit.is_some();
    let mut tint = explicit.unwrap_or(ColorToken::Accent);
    let mut glow = None;
    let mut kind = match input.variant {
        None if input.role == Some(Role::Confirm) => Kind::Filled,
        None | Some(ButtonVariant::Glass) => Kind::Glass,
        Some(ButtonVariant::Ghost) => Kind::Ghost,
        Some(ButtonVariant::Secondary) => Kind::Secondary,
        Some(ButtonVariant::Tinted) => Kind::Tinted,
        Some(ButtonVariant::Filled) => Kind::Filled,
        Some(ButtonVariant::Default) => {
            if input.tint.is_none() && input.role.is_none() {
                tint = ColorToken::Primary;
            }
            Kind::Filled
        }
        Some(ButtonVariant::Destructive) => {
            if input.tint.is_none() {
                tint = ColorToken::Danger;
            }
            Kind::Filled
        }
        Some(ButtonVariant::Active) => {
            glow = Some(Glow::Lg);
            Kind::Filled
        }
    };
    if input.prominent {
        kind = Kind::Filled;
    }
    let legacy_flat =
        matches!(input.variant, Some(ButtonVariant::Default | ButtonVariant::Destructive));
    let glowless_tint = matches!(tint, ColorToken::Primary | ColorToken::Label);
    if kind == Kind::Filled && glow.is_none() && !legacy_flat && !glowless_tint {
        glow = Some(Glow::Md);
    }
    let quiet = if tint_set || input.selected { tint } else { ColorToken::Label };
    let mut look = match kind {
        Kind::Filled => Look {
            background: Some(tokens.color(tint)),
            edge: None,
            label: on_tint(tint),
            shadow: None,
            edge_highlight: false,
            on_fill: true,
        },
        Kind::Tinted => Look {
            background: Some(wash(tokens, tint, Wash::Fill)),
            edge: None,
            label: tint,
            shadow: None,
            edge_highlight: false,
            on_fill: false,
        },
        Kind::Glass => Look {
            background: Some(tokens.color(ColorToken::GlassCard)),
            edge: Some(ColorToken::GlassEdge),
            label: quiet,
            shadow: Some(box_shadow(tokens.shadow(ShadowToken::Shadow1))),
            edge_highlight: true,
            on_fill: false,
        },
        Kind::Ghost => Look {
            background: None,
            edge: None,
            label: quiet,
            shadow: None,
            edge_highlight: false,
            on_fill: false,
        },
        Kind::Secondary => Look {
            background: Some(tokens.color(ColorToken::Fill)),
            edge: Some(ColorToken::Separator),
            label: quiet,
            shadow: None,
            edge_highlight: false,
            on_fill: false,
        },
    };
    if let Some(level) = glow {
        look.shadow = Some(glow_shadow(tokens, tint, level));
    }
    if input.selected {
        // The persistent selected layer (`fill`, opacity 1) between the fill and the label.
        let fill = tokens.color(ColorToken::Fill);
        look.background = Some(match look.background {
            Some(bg) => over(fill, bg),
            None => fill,
        });
    }
    look
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_theme_tokens::{BaseTokens, DarkTokens};

    fn look(variant: Option<ButtonVariant>) -> Look {
        resolve(LookInput { variant, ..LookInput::default() }, &BaseTokens)
    }

    #[test]
    fn glass_is_the_default_look() {
        let t = BaseTokens;
        let l = look(None);
        assert_eq!(l, look(Some(ButtonVariant::Glass)));
        assert_eq!(l.background, Some(t.color(ColorToken::GlassCard)));
        assert_eq!(l.edge, Some(ColorToken::GlassEdge));
        assert_eq!(l.label, ColorToken::Label);
        assert_eq!(l.shadow, Some(box_shadow(t.shadow(ShadowToken::Shadow1))));
        assert!(l.edge_highlight);
    }

    #[test]
    fn legacy_names_resolve_to_their_fills() {
        let t = BaseTokens;
        let primary = look(Some(ButtonVariant::Default));
        assert_eq!(primary.background, Some(t.color(ColorToken::Primary)));
        assert_eq!(primary.label, ColorToken::OnPrimary);
        assert_eq!(primary.shadow, None, "no glow on primary");
        let danger = look(Some(ButtonVariant::Destructive));
        assert_eq!(danger.background, Some(t.color(ColorToken::Danger)));
        assert_eq!(danger.label, ColorToken::OnAccent);
        assert_eq!(danger.shadow, None);
        let active = look(Some(ButtonVariant::Active));
        assert_eq!(active.background, Some(t.color(ColorToken::Accent)));
        assert_eq!(active.shadow, Some(glow_shadow(&t, ColorToken::Accent, Glow::Lg)));
    }

    #[test]
    fn a_tinted_look_is_the_16_percent_wash_with_a_tint_label() {
        let t = DarkTokens;
        let l = resolve(
            LookInput {
                variant: Some(ButtonVariant::Tinted),
                tint: Some(ColorToken::Success),
                ..LookInput::default()
            },
            &t,
        );
        let success = t.color(ColorToken::Success);
        let bg = l.background.expect("the wash");
        assert_eq!((bg.r, bg.g, bg.b), (success.r, success.g, success.b));
        assert_eq!(u32::from(bg.a), (u32::from(success.a) * 16 + 50) / 100);
        assert_eq!(l.label, ColorToken::Success);
        assert_eq!(l.edge, None);
    }

    #[test]
    fn prominent_fills_any_look_with_the_medium_glow() {
        let t = BaseTokens;
        let l = resolve(
            LookInput {
                variant: Some(ButtonVariant::Ghost),
                prominent: true,
                ..LookInput::default()
            },
            &t,
        );
        assert_eq!(l.background, Some(t.color(ColorToken::Accent)));
        assert_eq!(l.label, ColorToken::OnAccent);
        assert_eq!(l.shadow, Some(glow_shadow(&t, ColorToken::Accent, Glow::Md)));
        assert!(l.on_fill);
    }

    #[test]
    fn roles_pick_the_tint_and_the_confirm_fill() {
        let t = BaseTokens;
        let destructive =
            resolve(LookInput { role: Some(Role::Destructive), ..LookInput::default() }, &t);
        assert_eq!(destructive.background, Some(t.color(ColorToken::GlassCard)), "still glass");
        assert_eq!(destructive.label, ColorToken::Danger, "a quiet look takes the role's tint");
        let confirm = resolve(LookInput { role: Some(Role::Confirm), ..LookInput::default() }, &t);
        assert_eq!(confirm.background, Some(t.color(ColorToken::Accent)), "confirm is filled");
    }

    #[test]
    fn selected_adds_the_fill_layer_and_tints_a_quiet_label() {
        let t = BaseTokens;
        let rest = look(Some(ButtonVariant::Ghost));
        let sel = resolve(
            LookInput {
                variant: Some(ButtonVariant::Ghost),
                selected: true,
                ..LookInput::default()
            },
            &t,
        );
        assert_eq!(rest.background, None);
        assert_eq!(sel.background, Some(t.color(ColorToken::Fill)));
        assert_eq!(sel.label, ColorToken::Accent);
    }
}
