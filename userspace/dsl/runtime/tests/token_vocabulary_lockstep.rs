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
// So this test asserts a BIJECTION, not just coverage: each checker name resolves, and the role
// it resolves to names itself back (RFC-0116: the typed tokens' own name table,
// `ColorToken::name`, is the one place a role is spelled — and it is an exhaustive match, so a
// new variant breaks the theme crate's compile until it has a name).

use nexus_dsl_core::registry::{
    token_vocabulary, COLOR_TOKENS, RETIRED_TOKENS, SPACING_TOKENS, TEXT_STYLES,
};
use nexus_dsl_runtime::registry::{
    border_width, color_token, figures, font_weight, material, radius, shadow, spacing_token,
    text_align, text_style,
};
use nexus_theme_tokens::{ColorToken, TextStyleToken};

/// Every name the checker accepts resolves at runtime, to a role that names itself back, in
/// the typed tokens' order. Both drift directions fail here.
#[test]
fn color_vocabulary_is_a_bijection() {
    let typed: Vec<&str> = ColorToken::ALL.iter().map(|t| t.name()).collect();
    assert_eq!(COLOR_TOKENS, typed.as_slice(), "the checker's roles are the typed roles, in order");
    for name in COLOR_TOKENS {
        let Some(token) = color_token(name) else {
            panic!(
                "`{name}` is in the checker's COLOR_TOKENS but `color_token` returns None — \
                 a page using it compiles and then paints the default"
            );
        };
        assert_eq!(token.name(), *name);
    }
}

/// The count is pinned so ADDING a role cannot quietly stop at the enum.
#[test]
fn every_color_role_is_sayable() {
    const ROLES: usize = 36;
    assert_eq!(
        COLOR_TOKENS.len(),
        ROLES,
        "the colour vocabulary changed. A role is added in RFC-0116 first, then: a `ColorToken` \
         variant with its name, a `ROLES` entry in theme-tokens/build.rs, a value in every \
         resources/themes/*.nxtheme.toml, the name in `dsl/core` COLOR_TOKENS. Then bump ROLES."
    );
}

/// A retired name resolves to nothing at runtime either — the checker's message is the only
/// place it still appears.
#[test]
fn retired_names_do_not_resolve() {
    for (vocabulary, old, _) in RETIRED_TOKENS {
        if *vocabulary == "color" {
            assert!(color_token(old).is_none(), "retired role `{old}` still resolves");
        }
        if *vocabulary == "material" {
            assert!(material(old).is_none(), "retired material `{old}` still resolves");
        }
    }
}

/// The thirteen text styles: the checker's names are the typed styles' names.
#[test]
fn text_style_vocabulary_is_a_bijection() {
    let typed: Vec<&str> = TextStyleToken::ALL.iter().map(|t| t.name()).collect();
    assert_eq!(TEXT_STYLES, typed.as_slice());
    for name in TEXT_STYLES {
        assert_eq!(text_style(name).map(TextStyleToken::name), Some(*name));
    }
}

/// Every closed vocabulary the checker allows resolves at runtime — a name only the checker
/// knows compiles clean and renders the default (an opaque slab for a material, no shadow, a
/// zero radius).
#[test]
fn every_closed_vocabulary_resolves() {
    let resolves: &[(&str, fn(&str) -> bool)] = &[
        ("material", |n| material(n).is_some()),
        ("shadow", |n| shadow(n).is_some()),
        ("rounded", |n| radius(n).is_some()),
        ("corners", |n| radius(n).is_some()),
        ("border", |n| border_width(n).is_some()),
        ("fontWeight", |n| font_weight(n).is_some()),
        ("textAlign", |n| text_align(n).is_some()),
        ("figures", |n| figures(n).is_some()),
        ("padding", |n| spacing_token(n).is_some()),
        ("gap", |n| spacing_token(n).is_some()),
    ];
    for (modifier, ok) in resolves {
        let vocab = token_vocabulary(modifier).expect("a closed vocabulary");
        for name in vocab {
            assert!(ok(name), "`.{modifier}({name})` is allowed but resolves to nothing");
        }
    }
    for name in SPACING_TOKENS {
        assert!(spacing_token(name).is_some());
    }
    assert!(material("windowPane").is_none() && material("notAMaterial").is_none());
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
