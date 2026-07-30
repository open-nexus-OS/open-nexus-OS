// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
// The token vocabulary is stated TWICE and the two statements must agree.
//
// `dsl/core/src/registry.rs` holds the list the CHECKER validates a page
// against; `dsl/runtime/src/registry/tokens.rs` holds the match the RUNTIME
// resolves a name with. Nothing structurally ties them together, and each way
// they can drift fails silently in its own direction:
//
//   checker knows a name the runtime does not  →  the page compiles and the
//       node paints its default. This is exactly the failure RFC-0082 closed
//       for typos; a vocabulary edit can re-open it.
//   runtime knows a name the checker does not  →  the name is unsayable. Six
//       roles (`divider`, `glassHover`, `glassActive`, `toggleOnBg`,
//       `toggleOffBg`, `notifDot`) sat authored-but-unreachable that way, which
//       is why every hairline in a glass window was painted with the OPAQUE
//       `border` role instead.
//
// So this test asserts a BIJECTION, not just coverage: each checker name
// resolves, and the role it resolves to names itself back. `canonical_name` is
// an exhaustive match on purpose — adding a `ColorToken` variant breaks THIS
// FILE's compile until the variant is given a name, and the assertion below
// then fails until that name is also in the checker's list.

use nexus_dsl_core::registry::{token_vocabulary, COLOR_TOKENS};
use nexus_dsl_runtime::registry::{color_token, material_token};
use nexus_theme_tokens::ColorToken;

/// The one name a role answers to. Exhaustive by construction.
fn canonical_name(token: ColorToken) -> &'static str {
    match token {
        ColorToken::Surface => "surface",
        ColorToken::SurfaceVariant => "surfaceVariant",
        ColorToken::OnSurface => "onSurface",
        ColorToken::OnSurfaceVariant => "onSurfaceVariant",
        ColorToken::Accent => "accent",
        ColorToken::OnAccent => "onAccent",
        ColorToken::Border => "border",
        ColorToken::Shadow => "shadow",
        ColorToken::Background => "background",
        ColorToken::Primary => "primary",
        ColorToken::OnPrimary => "onPrimary",
        ColorToken::Danger => "danger",
        ColorToken::OnDanger => "onDanger",
        ColorToken::Warning => "warning",
        ColorToken::Success => "success",
        ColorToken::Info => "info",
        ColorToken::OnWarning => "onWarning",
        ColorToken::OnSuccess => "onSuccess",
        ColorToken::OnInfo => "onInfo",
        ColorToken::FocusRing => "focusRing",
        ColorToken::IslandBg => "islandBg",
        ColorToken::Scrim => "scrim",
        ColorToken::Destructive => "destructive",
        ColorToken::OnDestructive => "onDestructive",
        ColorToken::OnGlass => "onGlass",
        ColorToken::OnGlassMuted => "onGlassMuted",
        ColorToken::OnGlassStrong => "onGlassStrong",
        ColorToken::GlassIcon => "glassIcon",
        ColorToken::GlassPlaceholder => "glassPlaceholder",
        ColorToken::GlassFocus => "glassFocus",
        ColorToken::GlassFill => "glassFill",
        ColorToken::WallpaperTint => "wallpaperTint",
        ColorToken::WallpaperVignette => "wallpaperVignette",
        ColorToken::TextShadow => "textShadow",
        ColorToken::TextShadowStrong => "textShadowStrong",
        ColorToken::Transparent => "transparent",
        ColorToken::Divider => "divider",
        ColorToken::GlassHover => "glassHover",
        ColorToken::GlassActive => "glassActive",
        ColorToken::ToggleOnBg => "toggleOnBg",
        ColorToken::ToggleOffBg => "toggleOffBg",
        ColorToken::NotifDot => "notifDot",
        ColorToken::SliderTrack => "sliderTrack",
        ColorToken::SliderFill => "sliderFill",
        ColorToken::SliderIcon => "sliderIcon",
    }
}

/// Every name the checker accepts resolves at runtime, to a role that names
/// itself back. Both drift directions fail here.
#[test]
fn color_vocabulary_is_a_bijection() {
    for name in COLOR_TOKENS {
        let Some(token) = color_token(name) else {
            panic!(
                "`{name}` is in the checker's COLOR_TOKENS but `color_token` returns None — \
                 a page using it compiles and then paints the default"
            );
        };
        assert_eq!(
            canonical_name(token),
            *name,
            "`{name}` resolves to a role whose canonical name is `{}` — two names for one role, \
             or a copy/paste in one of the two lists",
            canonical_name(token)
        );
    }
}

/// The count is pinned so ADDING a role to `ColorToken` cannot quietly stop at
/// the enum. `canonical_name` already refuses to compile without an arm; this
/// catches the next step — an arm that never reaches the checker's list.
#[test]
fn every_color_role_is_sayable() {
    const ROLES: usize = 45;
    assert_eq!(
        COLOR_TOKENS.len(),
        ROLES,
        "the color vocabulary changed. If you ADDED a role: give it an arm in `canonical_name`, \
         a name in `dsl/core` COLOR_TOKENS, a `color_token` arm, a `ROLES` entry in \
         theme-tokens/build.rs, and a value in every resources/themes/*.nxtheme.toml. \
         Then bump ROLES here."
    );
}

/// The same seam exists for `.material(...)`: the checker's closed vocabulary
/// versus `material_token`. A level the checker allows but the runtime drops
/// falls back to an OPAQUE surface — a glass pane rendered as a flat slab.
#[test]
fn material_vocabulary_is_a_bijection() {
    let vocab = token_vocabulary("material").expect("`material` has a closed vocabulary");
    for name in vocab {
        assert!(
            material_token(name).is_some(),
            "`{name}` is an allowed `.material(...)` token but `material_token` returns None — \
             the node would render opaque"
        );
    }
    assert!(
        material_token("windowPane").is_some() && material_token("windowBar").is_some(),
        "the two window levels the handoff needs must resolve"
    );
    assert!(material_token("notAMaterial").is_none());
}

/// And a THIRD statement of the same seam: motion. `dsl/core`'s
/// `MOTION_TOKENS` is what `.transition(x)` validates against;
/// `MotionToken::from_name` is what the runtime stamps an intent with. A name
/// only the checker knows compiles clean and animates NOTHING — the quietest
/// of the three failures, because a missing entrance reads as "the platform
/// has no exit animations either", which is true and therefore unsuspicious.
#[test]
fn motion_vocabulary_is_a_bijection() {
    use animation::MotionToken;

    for name in nexus_dsl_core::registry::MOTION_TOKENS {
        assert!(
            MotionToken::from_name(name).is_some(),
            "`{name}` is an allowed motion token but the runtime cannot resolve it — \
             every `.transition({name})` in the tree would be a silent no-op"
        );
    }
    for token in MotionToken::ALL {
        assert!(
            nexus_dsl_core::registry::is_motion_token(token.name()),
            "`{}` exists in the physics table but no page may say it",
            token.name()
        );
    }
    assert_eq!(
        nexus_dsl_core::registry::MOTION_TOKENS.len(),
        MotionToken::ALL.len(),
        "the two motion lists disagree in length"
    );
}

/// The two slide tokens must not collapse into each other. They differ only in
/// the side they travel FROM, and that difference lives in the HOST's
/// `target_for` — so the only thing assertable here is that they are distinct
/// tokens sharing one property. If this ever reads as one token, a drop-down
/// is rising out of the bar it hangs from.
#[test]
fn the_two_slide_tokens_are_distinct() {
    use animation::{AnimProp, MotionToken};

    let up = MotionToken::from_name("slideUp").expect("slideUp resolves");
    let down = MotionToken::from_name("slideDown").expect("slideDown resolves");
    assert_ne!(up, down);
    assert_ne!(up.id(), down.id());
    assert_eq!(up.primary_prop(), AnimProp::TranslateY);
    assert_eq!(
        down.primary_prop(),
        AnimProp::TranslateY,
        "slideDown must drive translateY — the `_ => Opacity` wildcard in \
         `primary_prop` swallows a forgotten arm without a compile error"
    );
}
