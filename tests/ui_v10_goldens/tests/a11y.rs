// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Accessibility lints for the design-system primitives: WCAG contrast on the
//! resolved foreground/background pairs, and minimum touch-target sizes.

use nexus_theme_tokens::{BaseTokens, ColorToken, DarkTokens, Tokens};
use nexus_widget_badge::{Badge, BadgeVariant};
use nexus_widget_button::{ButtonVariant, GlassButton};
use nexus_widget_toggle::GlassToggle;
use ui_v10_goldens::{contrast_ratio, root_size, swatch, CONTRAST_TEXT, CONTRAST_UI, MIN_TOUCH};

#[test]
fn body_text_pairs_meet_aa() {
    // OnSurface/Surface and OnPrimary/Primary must clear normal-text AA (4.5).
    for t in [&BaseTokens as &dyn Tokens, &DarkTokens as &dyn Tokens] {
        let text = contrast_ratio(t.color(ColorToken::OnSurface), t.color(ColorToken::Surface));
        assert!(text >= CONTRAST_TEXT, "OnSurface/Surface contrast {text:.2} < {CONTRAST_TEXT}");
        let primary = contrast_ratio(t.color(ColorToken::OnPrimary), t.color(ColorToken::Primary));
        assert!(
            primary >= CONTRAST_TEXT,
            "OnPrimary/Primary contrast {primary:.2} < {CONTRAST_TEXT}"
        );
    }
}

#[test]
fn filled_control_pairs_meet_ui_contrast() {
    // Filled control fg/bg must clear UI-component contrast (3.0) in both themes.
    for t in [&BaseTokens as &dyn Tokens, &DarkTokens as &dyn Tokens] {
        let pairs = [
            (ColorToken::OnAccent, ColorToken::Accent), // Default/Active button
            (ColorToken::OnDanger, ColorToken::Danger), // Destructive
            (ColorToken::OnSuccess, ColorToken::Success), // Success badge
            (ColorToken::OnWarning, ColorToken::Warning), // Warning badge
        ];
        for (fg, bg) in pairs {
            let c = contrast_ratio(t.color(fg), t.color(bg));
            assert!(c >= CONTRAST_UI, "{fg:?}/{bg:?} contrast {c:.2} < {CONTRAST_UI}");
        }
    }
}

#[test]
fn component_foregrounds_track_their_variant() {
    // Sanity: the component's exposed foreground is the token we lint against.
    let t = BaseTokens;
    assert_eq!(
        GlassButton::new().variant(ButtonVariant::Destructive).foreground(&t),
        t.color(ColorToken::OnDanger)
    );
    assert_eq!(
        Badge::new().variant(BadgeVariant::Success).foreground(&t),
        t.color(ColorToken::OnSuccess)
    );
}

#[test]
fn interactive_targets_meet_minimum() {
    let t = BaseTokens;
    // A button with a realistic 20px content + padding.
    let button = GlassButton::new().content(swatch(20)).build(&t);
    let (bw, bh) = root_size(&button);
    assert!(bw >= MIN_TOUCH && bh >= MIN_TOUCH, "button {bw}x{bh} < {MIN_TOUCH}");

    // The switch track.
    let (tw, th) = root_size(&GlassToggle::new().checked(true).build(&t));
    assert!(tw >= MIN_TOUCH && th >= MIN_TOUCH, "toggle {tw}x{th} < {MIN_TOUCH}");

    // The segmented control (each slot is a tap target; the pill sets the height).
    let seg = nexus_widget_segment::Segment::new()
        .active(0)
        .options(vec![swatch(24), swatch(24)])
        .build(&t);
    let (_sw, sh) = root_size(&seg);
    assert!(sh >= MIN_TOUCH, "segment height {sh} < {MIN_TOUCH}");

    // NOTE: bare Checkbox/Radio indicators (20px) are intentionally NOT touch-target
    // linted — their tap target is the surrounding labeled row, not the glyph.
}
