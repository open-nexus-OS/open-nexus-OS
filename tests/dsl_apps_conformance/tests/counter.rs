// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Counter interaction conformance: a pointer tap on the "+" button MUST
// dispatch Inc and produce visible damage (Layout — the value text re-measures).
// Regression guard for the on-device "tap animates but the count stays 0"
// class (dispatch ran, `apply_changes` said Damage::None → stale UI).

mod common;

use nexus_dsl_runtime::{Damage, FixtureEnv, IdentityLocale, Value, View};

struct NoHost;
impl nexus_dsl_runtime::EffectHost for NoHost {
    fn call(&mut self, _s: &str, _m: &str, _a: &[Value], _t: u32) -> Result<Value, u32> {
        Err(0)
    }
}

use common::scene_texts;

#[test]
fn plus_tap_increments_and_damages() {
    let nxir = common::compile("counter");
    let symbols = common::program_symbols(&nxir);
    let tokens = nexus_theme_tokens::BaseTokens;
    let device = FixtureEnv::tablet("landscape");
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut host = NoHost;
    let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    view.run_initial_effects(&tokens, &device, &locale, &mut host).expect("effects");
    let boxes = common::layout_boxes(&view);

    // The "+" button = the page's LAST declared handler (toggle, "-", "+" in
    // authoring order; handlers() preserves emit order).
    let plus_id = view.handlers().last().map(|(box_id, _)| *box_id).expect("handlers exist");
    let (x, y) = boxes
        .iter()
        .find(|b| b.node_id == plus_id)
        .map(|b| (b.rect.x.0 + b.rect.width.0 / 2, b.rect.y.0 + b.rect.height.0 / 2))
        .expect("+ handler box found");

    assert!(scene_texts(&view).iter().any(|t| t == "0"), "starts at 0");

    // 1) Direct dispatch (control): Inc must be visible damage.
    let (e, c) = view.runtime().event_case("CounterEvent", "Inc").expect("Inc exists");
    let d = view
        .dispatch(&tokens, &device, &locale, &mut host, e, c, vec![])
        .expect("Inc dispatches");
    assert_ne!(d, Damage::None, "Inc must produce visible damage");
    assert!(scene_texts(&view).iter().any(|t| t == "1"), "count shows 1 after Inc");

    // 2) The pointer path the app-host tap() uses.
    let d2 = view
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
    assert!(
        matches!(d2, Some(Damage::Paint) | Some(Damage::Layout)),
        "pointer tap on + must dispatch with visible damage, got {d2:?}"
    );
    assert!(scene_texts(&view).iter().any(|t| t == "2"), "count shows 2 after tap");
}

/// The ON-DEVICE window geometry (floating window ≈ 320×254 content): the
/// exact repro of "tap animates but the count stays 0".
#[test]
fn plus_tap_at_device_window_size() {
    let nxir = common::compile("counter");
    let symbols = common::program_symbols(&nxir);
    let tokens = nexus_theme_tokens::BaseTokens;
    let device = FixtureEnv::tablet("landscape");
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut host = NoHost;
    let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    view.run_initial_effects(&tokens, &device, &locale, &mut host).expect("effects");
    let boxes = nexus_layout::LayoutEngine::new()
        .layout_with_viewport(
            view.scene(),
            nexus_layout_types::FxPx::new(320),
            Some(nexus_layout_types::FxPx::new(254)),
            &nexus_text_baked::measure_text::BakedTextMeasure,
        )
        .expect("lays out")
        .boxes;
    for (id, e) in view.handlers() {
        let b = boxes.iter().find(|b| b.node_id == *id);
        eprintln!(
            "handler box id={id} rect={:?} trigger={} off={}",
            b.map(|b| (b.rect.x.0, b.rect.y.0, b.rect.width.0, b.rect.height.0)),
            e.trigger,
            e.press_offset
        );
    }
    let plus_id = view.handlers().last().map(|(box_id, _)| *box_id).expect("handlers exist");
    let (x, y) = boxes
        .iter()
        .find(|b| b.node_id == plus_id)
        .map(|b| (b.rect.x.0 + b.rect.width.0 / 2, b.rect.y.0 + b.rect.height.0 / 2))
        .expect("+ handler box found");
    eprintln!("tapping + at ({x},{y})");
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
    assert!(
        matches!(d, Some(Damage::Paint) | Some(Damage::Layout)),
        "device-size tap on + must dispatch, got {d:?}"
    );
    assert!(scene_texts(&view).iter().any(|t| t == "1"), "count shows 1");
}
