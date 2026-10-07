// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! TASK-0066 lane targets (SSOT for `tools/qmp_inject_modal.py`'s tiling phase): on the
//! desktop session shell at 1280×800 the injector opens the launcher from the taskbar and
//! launches the settings app from the launcher's footer, then tiles that window with
//! Super+Ctrl chords. This test is the gate that the two presses still land.

// reason: test harness — a failed compile/mount step must panic loudly.
#![allow(clippy::expect_used, clippy::unwrap_used)]

mod common;

use nexus_dsl_runtime::interact::HandlerAction;
use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, Value, View};
use nexus_theme_tokens::BaseTokens;

/// The taskbar's launcher button and, with the launcher open, its footer's settings button.
pub const INJECT_LAUNCHER_BUTTON: (i32, i32) = (30, 772);
pub const INJECT_LAUNCH_SETTINGS: (i32, i32) = (914, 662);

struct NullHost;
impl nexus_dsl_runtime::EffectHost for NullHost {
    fn call(&mut self, _: &str, _: &str, _: &[Value]) -> Result<Value, u32> {
        Ok(Value::Bool(true))
    }
}

fn center_of(
    view: &View,
    boxes: &[nexus_layout::LayoutBox],
    symbols: &[String],
    event: &str,
    case: &str,
    payload: &str,
) -> (i32, i32) {
    let (e, c) = view.runtime.event_case(event, case).expect("case");
    let tap = symbols.iter().position(|s| s == "Tap").expect("Tap") as u32;
    let (box_id, _) = view
        .handlers()
        .iter()
        .find(|(_, h)| {
            h.trigger == tap
                && matches!(&h.action, HandlerAction::Dispatch { event, case, payload: p }
                    if *event == e && *case == c && p.first() == Some(&Value::Str(payload.to_string())))
        })
        .unwrap_or_else(|| panic!("no Tap handler dispatching {event}::{case}({payload})"));
    let b = boxes.iter().find(|b| b.node_id == *box_id).expect("box");
    (b.rect.x.0 + b.rect.width.0 / 2, b.rect.y.0 + b.rect.height.0 / 2)
}

#[test]
fn launcher_and_settings_launch_targets_are_where_the_injector_presses() {
    let nxir = common::compile("desktop-shell");
    let device = FixtureEnv::desktop();
    let symbols = common::program_symbols(&nxir);
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view = View::mount(&nxir, &BaseTokens, &device, &locale).expect("mounts");
    let boxes = common::layout_boxes(&view);
    let launcher = center_of(&view, &boxes, &symbols, "PanelEvent", "SetPanel", "launcher");
    assert_eq!(launcher, INJECT_LAUNCHER_BUTTON, "the taskbar launcher target moved");
    let mut host = NullHost;
    common::dispatch(
        &mut view,
        &device,
        &mut host,
        &symbols,
        "PanelEvent",
        "SetPanel",
        vec![Value::Str("launcher".to_string())],
    );
    let boxes = common::layout_boxes(&view);
    let settings = center_of(&view, &boxes, &symbols, "LauncherEvent", "Launch", "settings");
    assert_eq!(settings, INJECT_LAUNCH_SETTINGS, "the launcher's settings target moved");
}
