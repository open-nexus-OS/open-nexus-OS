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
