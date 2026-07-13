// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Crash repro: sweep hover hit-tests + partial re-renders across the REAL
// shell layout (the on-device MOVE-burst path that page-faulted the shell
// app-host). Must complete without panic for every pixel step.

mod common;

use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, Value, View};

struct Registry {
    id_sym: u32,
    label_sym: u32,
    icon_sym: u32,
}
impl nexus_dsl_runtime::EffectHost for Registry {
    fn call(
        &mut self,
        svc: &str,
        method: &str,
        _args: &[Value],
        _t: u32,
    ) -> Result<Value, u32> {
        if (svc, method) == ("bundlemgr", "enumerate") {
            let row = |id: &str, label: &str, icon: &str| {
                let mut fields = vec![
                    (self.id_sym, Value::Str(id.into())),
                    (self.label_sym, Value::Str(label.into())),
                    (self.icon_sym, Value::Str(icon.into())),
                ];
                fields.sort_by_key(|(s, _)| *s);
                Value::Record(fields)
            };
            return Ok(Value::List(vec![
                row("chat", "Chat", "message"),
                row("counter", "Counter", "calculator"),
                row("search", "Search", "magnifyingglass"),
                row("settings", "Settings", "gearshape"),
            ]));
        }
        Err(0)
    }
}

#[test]
fn hover_sweep_never_panics() {
    let nxir = common::compile("desktop-shell");
    let symbols = common::program_symbols(&nxir);
    let sym = |n: &str| symbols.iter().position(|s| s == n).expect(n) as u32;
    let mut host =
        Registry { id_sym: sym("id"), label_sym: sym("label"), icon_sym: sym("icon") };
    let tokens = nexus_theme_tokens::BaseTokens;
    let device = FixtureEnv::tablet("landscape");
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    view.run_initial_effects(&tokens, &device, &locale, &mut host).expect("effects");
    let boxes = common::layout_boxes(&view);

    // The device MOVE path: hover hit-test per sample along a diagonal sweep
    // (crosses tiles, dock, top bar), mirroring app-host `DslApp::hover`.
    // Crash repro: dense diagonal MOVE burst (the on-device path) — every
    // sample hit-tests + may cross handler boundaries; must never panic.
    let mut hovered: Option<usize> = None;
    for i in 0..400 {
        let x = 10 + i * 3;
        let y = 10 + i * 2;
        hovered = view.hover_box_id(
            &boxes,
            "Tap",
            nexus_layout_types::FxPx::new(x),
            nexus_layout_types::FxPx::new(y),
        );
    }
    let _ = hovered;

    // Calibration: hovering every box CENTER must resolve several distinct
    // interactive targets (tiles + dock + top bar) — pins that hit-testing
    // actually sees the shell's handlers, not an empty handler table.
    let mut targets = std::collections::BTreeSet::new();
    for bx in &boxes {
        let cx = bx.rect.x.0 + bx.rect.width.0 / 2;
        let cy = bx.rect.y.0 + bx.rect.height.0 / 2;
        if let Some(id) = view.hover_box_id(
            &boxes,
            "Tap",
            nexus_layout_types::FxPx::new(cx),
            nexus_layout_types::FxPx::new(cy),
        ) {
            targets.insert(id);
        }
    }
    assert!(
        targets.len() >= 4,
        "shell should expose several interactive targets, got {targets:?}"
    );

}
