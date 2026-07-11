// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

mod common;

use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, Value, View};

struct SettingsSpy {
    sets: Vec<(String, String)>,
}
impl nexus_dsl_runtime::EffectHost for SettingsSpy {
    fn call(
        &mut self,
        svc: &str,
        method: &str,
        args: &[Value],
        _timeout_ms: u32,
    ) -> Result<Value, u32> {
        match (svc, method) {
            ("settings", "set") => {
                if let (Some(Value::Str(k)), Some(Value::Str(v))) = (args.first(), args.get(1)) {
                    self.sets.push((k.clone(), v.clone()));
                }
                Ok(Value::Bool(true))
            }
            _ => Err(0),
        }
    }
}

/// The section chips + the Personalisierung light/dark tiles: tapping through
/// the panes reaches `svc.settings.set("ui.theme.mode", …)` — the REAL theme
/// chain below the IPC boundary.
#[test]
fn settings_theme_toggle_reaches_settings_set() {
    let nxir = common::compile("settings");
    let tokens = nexus_theme_tokens::BaseTokens;
    let device = FixtureEnv::tablet("landscape");
    let symbols: Vec<String> = Vec::new();
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    let mut host = SettingsSpy { sets: Vec::new() };

    // Select the Personalisierung section deterministically, then tap the
    // pane buttons (below the chip row) — light/dark dispatch SetTheme.
    let program_symbols = common::program_symbols(&nxir);
    common::dispatch(
        &mut view,
        &device,
        &mut host,
        &program_symbols,
        "SettingsEvent",
        "Select",
        vec![Value::Int(1)],
    );
    let boxes = common::layout_boxes(&view);
    let ids: Vec<usize> = view.handlers().iter().map(|(id, _)| *id).collect();
    for id in ids {
        let Some(b) = boxes.iter().find(|b| b.node_id == id) else { continue };
        if b.rect.width.as_i32() <= 0 || b.rect.height.as_i32() <= 0 {
            continue;
        }
        if b.rect.y.as_i32() < 100 {
            continue; // toolbar + chip row
        }
        let cx = b.rect.x + nexus_layout_types::FxPx::new(b.rect.width.as_i32() / 2);
        let cy = b.rect.y + nexus_layout_types::FxPx::new(b.rect.height.as_i32() / 2);
        let locale = IdentityLocale { symbols: &symbols, keys: &keys };
        let _ = view.pointer(&tokens, &device, &locale, &mut host, &boxes, "Tap", cx, cy);
        if host.sets.iter().any(|(k, _)| k == "ui.theme.mode") {
            break;
        }
    }
    assert!(
        host.sets.iter().any(|(k, _)| k == "ui.theme.mode"),
        "no theme set reached the host: {:?}",
        host.sets
    );
}
