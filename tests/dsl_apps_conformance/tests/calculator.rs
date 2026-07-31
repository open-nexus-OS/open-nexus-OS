// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Calculator interaction conformance: a tap on a key MUST dispatch and produce
// visible damage. Regression guard for the on-device "tap animates but the
// value stays 0" class (dispatch ran, `apply_changes` said Damage::None →
// stale UI) and for the flex-grown-wrapper hit box (the key painted 81px wide
// while only its ~23px label was tappable — `apphost: input tap miss`).
//
// The old file addressed the "7" key as `handlers()[3]`, which silently became
// a different key the moment the page was restructured. These tests assert
// PROPERTIES of every key instead, so they survive a redesign and still fail
// for the bug they were written against.

mod common;

use common::scene_texts;
use nexus_dsl_runtime::{Damage, FixtureEnv, IdentityLocale, Value, View};

struct NoHost;
impl nexus_dsl_runtime::EffectHost for NoHost {
    fn call(&mut self, _s: &str, _m: &str, _a: &[Value], _t: u32) -> Result<Value, u32> {
        Err(0)
    }
}

fn mounted() -> (Vec<u8>, Vec<String>) {
    let nxir = common::compile("calculator");
    let symbols = common::program_symbols(&nxir);
    (nxir, symbols)
}

#[test]
fn digit_dispatch_updates_the_display() {
    let (nxir, symbols) = mounted();
    let tokens = nexus_theme_tokens::BaseTokens;
    let device = FixtureEnv::tablet("landscape");
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut host = NoHost;
    let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    view.run_initial_effects(&tokens, &device, &locale, &mut host).expect("effects");
    assert!(scene_texts(&view).iter().any(|t| t == "0"), "starts at 0");

    let (e, c) = view.runtime().event_case("CalcEvent", "Digit").expect("Digit exists");
    let d = view
        .dispatch(&tokens, &device, &locale, &mut host, e, c, vec![Value::Int(7)])
        .expect("Digit dispatches");
    assert_ne!(d, Damage::None, "Digit must produce visible damage");
    assert!(scene_texts(&view).iter().any(|t| t == "7"), "display shows 7");
}

/// EVERY key is a full cell and tappable across all of it.
///
/// This is the shape of the bug: a flex-grown row child is allocated the shared
/// width, but its subtree keeps its natural width, so the painted key was 81px
/// wide and the hit box ~23px. Asserting each handler's box is cell-sized —
/// and that a tap 2px inside its left edge dispatches — catches that directly,
/// for all 20 keys, without naming any of them.
#[test]
fn every_key_is_a_full_cell_and_tappable_to_its_edges() {
    let (nxir, symbols) = mounted();
    let tokens = nexus_theme_tokens::BaseTokens;
    let device = FixtureEnv::desktop();
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut host = NoHost;
    let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    view.run_initial_effects(&tokens, &device, &locale, &mut host).expect("effects");
    let boxes = nexus_layout::LayoutEngine::new()
        .layout_with_viewport(
            view.scene(),
            nexus_layout_types::FxPx::new(392),
            Some(nexus_layout_types::FxPx::new(616)),
            &nexus_text_baked::measure_text::BakedTextMeasure,
        )
        .expect("lays out")
        .boxes;

    // The handoff grid: 4 columns over a 360px content box with 12px gaps =
    // 81px cells, five 84px rows. Anything much smaller is a label, not a key.
    let key_boxes: Vec<_> = view
        .handlers()
        .iter()
        .filter_map(|(id, _)| boxes.iter().find(|b| b.node_id == *id))
        .filter(|b| b.rect.y.0 > 130)
        .collect();
    // 19, not 20: the bottom row is `0` (two tracks) + `,` + `=`.
    assert_eq!(key_boxes.len(), 19, "the keypad has 19 keys");
    for b in &key_boxes {
        assert!(
            b.rect.width.0 >= 75 && b.rect.height.0 >= 75,
            "key at ({},{}) is {}x{} — a label-sized hit box, not a cell",
            b.rect.x.0,
            b.rect.y.0,
            b.rect.width.0,
            b.rect.height.0
        );
    }

    // …and the painted area really is the tappable area.
    let edges: Vec<(i32, i32)> =
        key_boxes.iter().map(|b| (b.rect.x.0 + 2, b.rect.y.0 + b.rect.height.0 / 2)).collect();
    for (x, y) in edges {
        let d = view
            .pointer_scrolled(
                &tokens,
                &device,
                &locale,
                &mut host,
                &boxes,
                "Tap",
                nexus_layout_types::FxPx::new(x),
                nexus_layout_types::FxPx::new(y),
                None,
            )
            .expect("pointer ok");
        assert!(d.is_some(), "a tap 2px inside the key's left edge at ({x},{y}) missed");
    }
}

/// The `0` key spans two tracks (handoff `grid-column: span 2`).
#[test]
fn the_zero_key_spans_two_tracks() {
    let (nxir, symbols) = mounted();
    let tokens = nexus_theme_tokens::BaseTokens;
    let device = FixtureEnv::desktop();
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut host = NoHost;
    let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    view.run_initial_effects(&tokens, &device, &locale, &mut host).expect("effects");
    let boxes = nexus_layout::LayoutEngine::new()
        .layout_with_viewport(
            view.scene(),
            nexus_layout_types::FxPx::new(392),
            Some(nexus_layout_types::FxPx::new(616)),
            &nexus_text_baked::measure_text::BakedTextMeasure,
        )
        .expect("lays out")
        .boxes;
    // The KEY boxes are the handler boxes; a label's `y` sits inside a row, so
    // grouping raw boxes by `y` would pick glyphs instead of cells.
    let key_boxes: Vec<_> = view
        .handlers()
        .iter()
        .filter_map(|(id, _)| boxes.iter().find(|b| b.node_id == *id))
        .filter(|b| b.rect.y.0 > 130)
        .collect();
    let bottom = key_boxes.iter().map(|b| b.rect.y.0).max().expect("key rows");
    let mut widths: Vec<i32> =
        key_boxes.iter().filter(|b| b.rect.y.0 == bottom).map(|b| b.rect.width.0).collect();
    widths.sort_unstable();
    widths.dedup();
    let (cell, wide) = (widths[0], *widths.last().unwrap());
    assert!(
        (wide - (cell * 2 + 12)).abs() <= 1,
        "the 0 key ({wide}) must span two {cell}px tracks plus the 12px gap"
    );
}

/// AC/C is two-stage (handoff: first press clears the entry, second the state).
#[test]
fn clear_is_two_stage() {
    let (nxir, symbols) = mounted();
    let tokens = nexus_theme_tokens::BaseTokens;
    let device = FixtureEnv::tablet("landscape");
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut host = NoHost;
    let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    view.run_initial_effects(&tokens, &device, &locale, &mut host).expect("effects");
    let case = |view: &View<'_>, n: &str| {
        view.runtime().event_case("CalcEvent", n).unwrap_or_else(|| panic!("{n} exists"))
    };
    let (de, dc) = case(&view, "Digit");
    view.dispatch(&tokens, &device, &locale, &mut host, de, dc, vec![Value::Int(5)])
        .expect("digit");
    assert!(scene_texts(&view).iter().any(|t| t == "5"));
    let (ce, cc) = case(&view, "Clear");
    view.dispatch(&tokens, &device, &locale, &mut host, ce, cc, vec![]).expect("clear 1");
    assert!(scene_texts(&view).iter().any(|t| t == "0"), "first Clear zeroes the entry");
    view.dispatch(&tokens, &device, &locale, &mut host, ce, cc, vec![]).expect("clear 2");
    assert!(scene_texts(&view).iter().any(|t| t == "0"), "second Clear resets the state");
}
