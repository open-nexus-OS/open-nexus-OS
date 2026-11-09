// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The control system of the design artifacts (their `control.js`): ONE size, tint and label
//! vocabulary every control primitive reads, so a row of controls of one size lines up and a
//! tint means the same thing on a button, a switch and a checkbox.
//!
//! - [`metrics`]: a size's horizontal padding, icon size and icon↔label gap next to the outer
//!   height `ControlSize` already carries (from the themes).
//! - [`label_style`]: the control label — the size's text style at weight 500.
//! - The tint helpers derive what a tint role paints from the role's own colour — the readable
//!   label on a tint fill ([`on_tint`]), the glow under it ([`glow`], [`glow_shadow`]) and the
//!   translucent washes ([`wash`]) — so no tint needs a role of its own.
//! - [`Role`]: what a control means (destructive, confirm, cancel, close).
//!
//! Pure data, no rendering: the widgets turn these values into `VisualStyle`s.

use nexus_layout_types::{BoxShadow, FontWeight, FxPx, Rgba8, TextStyle, WhiteSpace};
use nexus_theme_tokens::{ColorToken, ControlSize, Tokens};

/// A control size's metrics (px). Every control of one size is `height` tall (border box).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlMetrics {
    /// The outer height (the theme's control height of the size).
    pub height: FxPx,
    /// The horizontal padding of a labelled control.
    pub pad_x: FxPx,
    /// The icon size.
    pub icon: FxPx,
    /// The gap between an icon and its label.
    pub gap: FxPx,
}

/// The metrics of a control size: xs 24/8/12/4 · sm 30/12/14/4 · md 36/16/16/6 · lg 44/20/18/6 ·
/// xl 52/24/20/8 (height / padding / icon / gap).
#[must_use]
pub fn metrics(size: ControlSize) -> ControlMetrics {
    let (pad_x, icon, gap) = match size {
        ControlSize::Xs => (8, 12, 4),
        ControlSize::Sm => (12, 14, 4),
        ControlSize::Md => (16, 16, 6),
        ControlSize::Lg => (20, 18, 6),
        ControlSize::Xl => (24, 20, 8),
    };
    ControlMetrics {
        height: size.height(),
        pad_x: FxPx::new(pad_x),
        icon: FxPx::new(icon),
        gap: FxPx::new(gap),
    }
}

/// The control label of a size in `color`: caption · footnote · body · headline · title3, every
/// one at weight 500 (the artifacts request 500 at every size; the atlas sets 15/17 px on its
/// 600 face), on one line.
#[must_use]
pub fn label_style(size: ControlSize, color: Rgba8) -> TextStyle {
    let (style, _) = size.label();
    TextStyle {
        white_space: WhiteSpace::NoWrap,
        ..style.text_style(color, Some(FontWeight::Medium))
    }
}

/// The label a tint FILL carries: `onPrimary` on `primary` and `label`, `onAccent` on every
/// other tint.
#[must_use]
pub const fn on_tint(tint: ColorToken) -> ColorToken {
    match tint {
        ColorToken::Primary | ColorToken::Label => ColorToken::OnPrimary,
        _ => ColorToken::OnAccent,
    }
}

/// The colour of the glow under a tint fill: `accentGlow` for the accent (a user accent recolours
/// it), the tint at 38 % for any other.
#[must_use]
pub fn glow(tokens: &dyn Tokens, tint: ColorToken) -> Rgba8 {
    if tint == ColorToken::Accent {
        tokens.color(ColorToken::AccentGlow)
    } else {
        with_alpha_percent(tokens.color(tint), 38)
    }
}

/// The three glows under a tint fill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glow {
    /// `0 2px 8px` — switch tracks, checkboxes, radios.
    Sm,
    /// `0 3px 12px` — filled buttons, toggle buttons.
    Md,
    /// `0 4px 16px` — the legacy `active` button.
    Lg,
}

/// The glow under a tint fill as the painter's box shadow.
#[must_use]
pub fn glow_shadow(tokens: &dyn Tokens, tint: ColorToken, level: Glow) -> BoxShadow {
    let (y, blur) = match level {
        Glow::Sm => (2, 8),
        Glow::Md => (3, 12),
        Glow::Lg => (4, 16),
    };
    BoxShadow {
        offset_x: FxPx::ZERO,
        offset_y: FxPx::new(y),
        blur_radius: FxPx::new(blur),
        spread: FxPx::ZERO,
        color: glow(tokens, tint),
    }
}

/// The translucent washes of a tint — the only strengths the controls use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wash {
    /// 16 %: the background of a tinted look.
    Fill,
    /// 30 %: a hairline on a tinted or filled look.
    Border,
    /// 55 %: a strong outline.
    Line,
}

impl Wash {
    /// The strength in percent of the tint's own alpha.
    #[must_use]
    pub const fn percent(self) -> u32 {
        match self {
            Wash::Fill => 16,
            Wash::Border => 30,
            Wash::Line => 55,
        }
    }
}

/// A tint's wash: the role's colour at `strength` of its own alpha (no role of its own — every
/// tint washes the same way).
#[must_use]
pub fn wash(tokens: &dyn Tokens, tint: ColorToken, strength: Wash) -> Rgba8 {
    with_alpha_percent(tokens.color(tint), strength.percent())
}

/// `color` with its alpha scaled to `percent` (rounded).
#[must_use]
pub fn with_alpha_percent(color: Rgba8, percent: u32) -> Rgba8 {
    let a = (u32::from(color.a) * percent.min(100) + 50) / 100;
    Rgba8::new(color.r, color.g, color.b, a as u8)
}

/// `top` composited over `bottom` (straight alpha, source-over) — a persistent layer folded into
/// the colour under it (the selected / open layer of a control).
#[must_use]
pub fn over(top: Rgba8, bottom: Rgba8) -> Rgba8 {
    let ta = u32::from(top.a);
    let below = u32::from(bottom.a) * (255 - ta) / 255;
    let alpha = ta + below;
    if alpha == 0 {
        return Rgba8::new(0, 0, 0, 0);
    }
    let mix = |t: u8, b: u8| ((u32::from(t) * ta + u32::from(b) * below + alpha / 2) / alpha) as u8;
    Rgba8::new(mix(top.r, bottom.r), mix(top.g, bottom.g), mix(top.b, bottom.b), alpha as u8)
}

/// What a control means (the artifacts' `role`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// An irreversible action: tints the control `danger`.
    Destructive,
    /// The confirming action of a dialog: the filled look (a ✓ circle when it has no content).
    Confirm,
    /// The dismissing action of a dialog (quiet).
    Cancel,
    /// Closes a surface: a ✕ circle when it has no content.
    Close,
}

impl Role {
    /// The tint a role implies: `danger` for destructive, `accent` for confirm.
    #[must_use]
    pub const fn tint(self) -> Option<ColorToken> {
        match self {
            Role::Destructive => Some(ColorToken::Danger),
            Role::Confirm => Some(ColorToken::Accent),
            Role::Cancel | Role::Close => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_theme_tokens::{BaseTokens, DarkTokens, TextStyleToken};

    #[test]
    fn one_outer_height_per_size() {
        let heights: [i32; 5] =
            [ControlSize::Xs, ControlSize::Sm, ControlSize::Md, ControlSize::Lg, ControlSize::Xl]
                .map(|s| metrics(s).height.0);
        assert_eq!(heights, [24, 30, 36, 44, 52]);
        let md = metrics(ControlSize::Md);
        assert_eq!((md.pad_x.0, md.icon.0, md.gap.0), (16, 16, 6));
        let xl = metrics(ControlSize::Xl);
        assert_eq!((xl.pad_x.0, xl.icon.0, xl.gap.0), (24, 20, 8));
    }

    #[test]
    fn every_control_label_requests_weight_500() {
        let ink = Rgba8::new(0, 0, 0, 255);
        for (size, style) in [
            (ControlSize::Xs, TextStyleToken::Caption),
            (ControlSize::Sm, TextStyleToken::Footnote),
            (ControlSize::Md, TextStyleToken::Body),
            (ControlSize::Lg, TextStyleToken::Headline),
            (ControlSize::Xl, TextStyleToken::Title3),
        ] {
            let label = label_style(size, ink);
            assert_eq!(label.font_size.0, i32::from(style.spec().size), "{size:?}");
            assert_eq!(label.font_weight, FontWeight::Medium, "{size:?}");
            assert_eq!(label.white_space, WhiteSpace::NoWrap);
        }
    }

    #[test]
    fn the_tinted_wash_is_the_role_at_16_percent() {
        let t = BaseTokens;
        let accent = t.color(ColorToken::Accent);
        let w = wash(&t, ColorToken::Accent, Wash::Fill);
        assert_eq!((w.r, w.g, w.b), (accent.r, accent.g, accent.b));
        assert_eq!(w.a, 41, "255 × 16 % rounds to 41");
        // A translucent role washes from its own alpha (modeAirplane is 85 %).
        let airplane = t.color(ColorToken::ModeAirplane);
        let a = wash(&t, ColorToken::ModeAirplane, Wash::Border).a;
        assert_eq!(u32::from(a), (u32::from(airplane.a) * 30 + 50) / 100);
    }

    #[test]
    fn the_accent_glows_in_its_own_role_others_at_38_percent() {
        let t = DarkTokens;
        assert_eq!(glow(&t, ColorToken::Accent), t.color(ColorToken::AccentGlow));
        let danger = t.color(ColorToken::Danger);
        assert_eq!(glow(&t, ColorToken::Danger).a, ((u32::from(danger.a) * 38 + 50) / 100) as u8);
        let md = glow_shadow(&t, ColorToken::Accent, Glow::Md);
        assert_eq!((md.offset_y.0, md.blur_radius.0, md.spread.0), (3, 12, 0));
    }

    #[test]
    fn a_tint_fill_reads_in_its_on_colour() {
        assert_eq!(on_tint(ColorToken::Primary), ColorToken::OnPrimary);
        assert_eq!(on_tint(ColorToken::Label), ColorToken::OnPrimary);
        assert_eq!(on_tint(ColorToken::Danger), ColorToken::OnAccent);
        assert_eq!(on_tint(ColorToken::ModeAirplane), ColorToken::OnAccent);
    }

    #[test]
    fn over_composites_straight_alpha() {
        let clear = Rgba8::new(0, 0, 0, 0);
        let fill = Rgba8::new(120, 120, 128, 41);
        assert_eq!(over(fill, clear), fill, "a layer over nothing is the layer");
        let doubled = over(fill, fill);
        assert_eq!((doubled.r, doubled.g, doubled.b), (120, 120, 128));
        assert!(doubled.a > fill.a && doubled.a < 2 * fill.a);
        let opaque = Rgba8::new(10, 20, 30, 255);
        assert_eq!(over(opaque, fill), opaque, "an opaque layer covers");
    }

    #[test]
    fn roles_imply_their_tint() {
        assert_eq!(Role::Destructive.tint(), Some(ColorToken::Danger));
        assert_eq!(Role::Confirm.tint(), Some(ColorToken::Accent));
        assert_eq!(Role::Close.tint(), None);
    }
}
