// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
// What `.textSize(<token>)` ACTUALLY renders at.
//
// The type scale a page writes against (`[typography]` in the theme) and the
// ladder that gets baked (`FACES` in text-baked/build.rs) are two different
// lists, and `FontSize::nearest` is the lossy function between them. A token
// whose px has no rung silently resolves to a neighbour, so the number in the
// theme is a REQUEST, not a promise.
//
// This test writes the promise down. It exists because baking the 11px caption
// rung put the `sm` step (12) exactly between two rungs for the first time —
// the tie-break stopped being dead code and started deciding whether ~80
// shipped labels stayed at 13 or shrank to 11. Rounding up keeps them; this
// pins that, so the next rung anyone bakes has to face the same question
// instead of quietly re-flowing every app.

use nexus_text_baked::{FontSize, Weight};
use nexus_theme_tokens::{BaseTokens, Tokens, TypographyToken};

/// Every `.textSize` token, the px the theme authors for it, and the face it
/// really lands on. Regular weight; the SemiBold column is the same size
/// decision by construction (size wins over weight).
const RESOLUTION: &[(TypographyToken, i32, FontSize)] = &[
    (TypographyToken::Xs, 11, FontSize::Caption),
    // The tie. 12 is equidistant from 11 and 13 — up.
    (TypographyToken::Sm, 12, FontSize::Small),
    (TypographyToken::Base, 14, FontSize::Small),
    (TypographyToken::Md, 16, FontSize::Body),
    (TypographyToken::Lg, 18, FontSize::Body),
    (TypographyToken::Xl, 20, FontSize::Title),
    (TypographyToken::Xxl, 24, FontSize::Title),
    (TypographyToken::Xxxl, 30, FontSize::DisplaySemi),
    (TypographyToken::Display, 36, FontSize::DisplaySemi),
    (TypographyToken::Hero, 120, FontSize::Hero),
];

#[test]
fn every_type_token_resolves_to_the_documented_face() {
    for &(token, px, expected) in RESOLUTION {
        assert_eq!(
            BaseTokens.type_size(token).0,
            px,
            "the theme's px for this token moved — update the table, then check \
             whether the FACE below moved with it"
        );
        assert_eq!(
            FontSize::nearest(px, Weight::Regular),
            expected,
            "{px}px now renders on a different face. If you just baked a rung, \
             this is the collateral: every page written against this token \
             changes size. docs/dev/dsl/modifiers.md states the mapping."
        );
    }
}

/// The caption rung is the point of the exercise: `xs` must be its own size,
/// not a second name for `sm`. Without this the panels' 11px sub-labels render
/// as large as the 13px labels above them and the density inverts.
#[test]
fn xs_and_sm_are_actually_different_sizes() {
    let xs = BaseTokens.type_size(TypographyToken::Xs).0;
    let sm = BaseTokens.type_size(TypographyToken::Sm).0;
    assert_ne!(
        FontSize::nearest(xs, Weight::Regular),
        FontSize::nearest(sm, Weight::Regular),
        "xs ({xs}px) and sm ({sm}px) collapsed onto one face — a sub-label \
         cannot read as subordinate to its own label"
    );
    assert_ne!(
        FontSize::nearest(xs, Weight::SemiBold),
        FontSize::nearest(sm, Weight::SemiBold),
        "…and the same must hold at SemiBold, where tile labels live"
    );
}
