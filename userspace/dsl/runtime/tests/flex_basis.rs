// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//! `.basis(n)` end to end: DSL source → check → lower → mount → layout.
//!
//! The engine unit tests (`nexus-layout/src/engine_tests.rs`) prove the
//! distribution maths. This file proves the WIRE: modId 54 survives lowering
//! and reaches `FlexItem::flex_basis`, because a missing arm in
//! `emit/modifiers.rs` is a silent no-op that no engine test can catch.
//!
//! Own file rather than a case in `layout_viewport.rs` — that file sits at 886
//! lines against a 955 ratchet.

use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, View};

fn compile(src: &str) -> Vec<u8> {
    let file = nexus_dsl_core::parse_file(src).expect("parses");
    let (model, diags) = nexus_dsl_core::check_file(&file);
    assert!(!nexus_dsl_core::has_errors(&diags), "check: {diags:?}");
    nexus_dsl_core::lower_file(&file, &model, src).expect("lowers").nxir
}

/// Lays the page out at `w`×`h` and returns the widths of the four key
/// WRAPPERS. The DSL has no `.id()` modifier — `LayoutBox::id` is set by kit
/// widgets — so the cells are addressed by their pre-order node ids: the row
/// itself is the page root (1) and each wrapper is followed by its text child,
/// giving 2 · 4 · 6 · 8.
fn key_widths(src: &str, w: i32, h: i32) -> Vec<i32> {
    let nxir = compile(src);
    let device = FixtureEnv::default();
    let tokens = nexus_theme_tokens::BaseTokens;
    let symbols: Vec<String> = Vec::new();
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    let layout = nexus_layout::LayoutEngine::new()
        .layout_with_viewport(
            view.scene(),
            nexus_layout_types::FxPx::new(w),
            Some(nexus_layout_types::FxPx::new(h)),
            &nexus_text_baked::measure_text::BakedTextMeasure,
        )
        .expect("lays out");
    [2usize, 4, 6, 8]
        .iter()
        .map(|node| {
            layout
                .boxes
                .iter()
                .find(|b| b.node_id == *node)
                .unwrap_or_else(|| panic!("key wrapper node {node} missing"))
                .rect
                .width
                .as_i32()
        })
        .collect()
}

/// A keypad row with deliberately unequal label widths.
fn row_src(basis: &str) -> String {
    let key = |label: &str, grow: &str| {
        format!("        Stack {{ Text(\"{label}\") }}.direction(row).grow({grow}){basis}\n")
    };
    format!(
        "Page Main {{\n    Stack {{\n{}{}{}{}    }}\n    .direction(row)\n    .gap(0)\n}}\n",
        key("AC", "1"),
        key("7", "1"),
        key("8", "1"),
        key("9", "1"),
    )
}

#[test]
fn basis_reaches_the_engine_through_the_wire() {
    // Without basis the two-character key is wider — the bug.
    let plain = key_widths(&row_src(""), 400, 200);
    let spread_plain = plain.iter().max().unwrap() - plain.iter().min().unwrap();
    assert!(
        spread_plain > 1,
        "without .basis the 'AC' key must stay wider, else this test proves nothing: {plain:?}"
    );

    // With .basis(0) every cell is equal (±1 for the indivisible remainder).
    let fitted = key_widths(&row_src(".basis(0)"), 400, 200);
    let spread = fitted.iter().max().unwrap() - fitted.iter().min().unwrap();
    assert!(spread <= 1, "modId 54 did not reach flex_basis: {fitted:?}");
    assert_eq!(fitted.iter().sum::<i32>(), 400, "the row must tile: {fitted:?}");
}

#[test]
fn basis_divides_evenly_at_every_width() {
    for w in [204, 392, 823, 1280] {
        let widths = key_widths(&row_src(".basis(0)"), w, 200);
        let spread = widths.iter().max().unwrap() - widths.iter().min().unwrap();
        assert!(spread <= 1, "uneven at {w}: {widths:?}");
        assert_eq!(widths.iter().sum::<i32>(), w, "must tile at {w}: {widths:?}");
    }
}

// ---------------------------------------------------------------- textFit

/// Lays out a `.textFit` keypad cell and returns the font size the LAYOUT
/// chose for its label (`LayoutBox::text_px`).
fn fitted_px(pct: u32, min: i32, max: i32, label: &str, w: i32, h: i32) -> i32 {
    let src = format!(
        "Page Main {{\n    Stack {{\n        Stack {{ Text(\"{label}\") }}\n            \
         .direction(row)\n            .align(center)\n            .justify(center)\n            \
         .grow(1)\n            .basis(0)\n            .textFit({pct}, {min}, {max})\n    }}\n    \
         .direction(row)\n    .gap(0)\n}}\n"
    );
    let nxir = compile(&src);
    let device = FixtureEnv::default();
    let tokens = nexus_theme_tokens::BaseTokens;
    let symbols: Vec<String> = Vec::new();
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    let layout = nexus_layout::LayoutEngine::new()
        .layout_with_viewport(
            view.scene(),
            nexus_layout_types::FxPx::new(w),
            Some(nexus_layout_types::FxPx::new(h)),
            &nexus_text_baked::measure_text::BakedTextMeasure,
        )
        .expect("lays out");
    let requested = layout
        .boxes
        .iter()
        .find_map(|b| b.text_px)
        .unwrap_or_else(|| panic!("modId 55 never reached the layout"));
    // The RESOLVED rung, not the request: the painter draws the nearest baked
    // face, so that is the size a user actually sees. Asserting the request
    // would let a ladder gap (every size 30..72 once collapsed onto 36) pass
    // as "the label grew".
    let font =
        nexus_text_baked::FontSize::nearest(requested.as_i32(), nexus_text_baked::Weight::Regular);
    font.px()
}

#[test]
fn textfit_reaches_the_engine_through_the_wire() {
    // 30% of a 100px-tall cell = 30px requested, which the ladder renders at
    // its 36px rung.
    assert_eq!(fitted_px(30, 11, 120, "7", 400, 100), 36);
}

/// The requirement in one test: a bigger WINDOW must mean a bigger label,
/// in steps, with no breakpoints anywhere.
#[test]
fn label_grows_in_steps_with_the_window() {
    // A key cell of the given height, capped at the largest Latin rung.
    let sizes: Vec<i32> =
        [60, 90, 120, 150, 200].iter().map(|h| fitted_px(30, 11, 52, "7", 400, *h)).collect();
    for pair in sizes.windows(2) {
        assert!(pair[1] >= pair[0], "the label must never SHRINK as the window grows: {sizes:?}");
    }
    assert!(sizes.last() > sizes.first(), "it must actually grow: {sizes:?}");
    // Genuinely STEPPED, and more than one step — the ladder gap between 36
    // and 120 used to make every one of these land on the same face.
    let mut distinct: Vec<i32> = sizes.clone();
    distinct.dedup();
    assert!(distinct.len() >= 3, "expected several visible steps, got {sizes:?}");
}

/// A long number must SHRINK to stay inside its box — the handoff's
/// 52 / 38 / 30 display ramp, derived instead of hard-coded.
#[test]
fn a_longer_number_steps_the_display_down() {
    let short = fitted_px(85, 16, 52, "0", 344, 104);
    let long = fitted_px(85, 16, 52, "1.234.567.890.123", 344, 104);
    assert!(long < short, "long={long} must be smaller than short={short}");
    assert!(long >= 16, "never below the floor: {long}");
}

/// The requirement, stated as the calculator actually experiences it: a key
/// at the handoff's own size gets the handoff's own type step, and a key in a
/// big window gets a genuinely bigger one.
#[test]
fn keypad_label_matches_the_handoff_and_grows_from_there() {
    // Handoff: 392x616 window, ~98x91 key, 23px label. 21 is the baked rung
    // nearest 23 — the ladder cannot be more exact than its rungs.
    assert_eq!(fitted_px(30, 11, 52, "7", 98, 91), 21);
    // A key twice as tall must land on a visibly larger rung.
    let big = fitted_px(30, 11, 52, "7", 320, 182);
    assert!(big >= 44, "a 182px-tall key should reach 44/52, got {big}");
    // A tiny window degrades to the floor instead of overflowing.
    assert_eq!(fitted_px(30, 11, 52, "7", 51, 20), 11);
}
