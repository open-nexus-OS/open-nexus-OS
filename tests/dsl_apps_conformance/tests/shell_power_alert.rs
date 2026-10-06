// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The shell's first modal (TASK-0074): the Control Center's power button opens the kit's
//! `WinAlert`; cancel/ESC close it; confirm answers with the kit's `WinToast` (honest text —
//! there is no power service yet) that times out. Against the REAL compiled shell at the
//! desktop layout, and the SSOT for the live-lane injector's click targets: the constants
//! below are what `tools/qmp_visible_input_inject.py` presses — this test is the gate that
//! they still land on the pill and the button.

// reason: test harness — a failed compile/mount step must panic loudly.
#![allow(clippy::expect_used, clippy::unwrap_used)]

mod common;

use nexus_dsl_runtime::interact::HandlerAction;
use nexus_dsl_runtime::{DismissReason, FixtureEnv, IdentityLocale, Value, View};
use nexus_layout_types::FxPx;
use nexus_theme_tokens::BaseTokens;

/// Display-space centers at 1280×800 in the DESKTOP layout — the session shell the QEMU proof
/// lanes log into (`windowd: session shell visible (product=default)`): the Control-Center
/// pill in the top bar, the power button in the open panel's header, and a background spot
/// (the workspace) a press must NOT reach while the alert is open.
pub const INJECT_CC_PILL: (i32, i32) = (1253, 18);
pub const INJECT_POWER_BUTTON: (i32, i32) = (1169, 71);
pub const INJECT_BACKGROUND: (i32, i32) = (400, 400);
/// The alert's Confirm button ("Shut down") while the alert is open.
pub const INJECT_CONFIRM_BUTTON: (i32, i32) = (714, 428);

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
    payload: Option<&str>,
) -> (i32, i32) {
    let (e, c) = view.runtime.event_case(event, case).expect("case");
    let tap = symbols.iter().position(|s| s == "Tap").expect("Tap") as u32;
    let (box_id, _) = view
        .handlers()
        .iter()
        .find(|(_, h)| {
            h.trigger == tap
                && match &h.action {
                    HandlerAction::Dispatch { event, case, payload: p } => {
                        *event == e
                            && *case == c
                            && payload
                                .is_none_or(|want| p.first() == Some(&Value::Str(want.to_string())))
                    }
                    _ => false,
                }
        })
        .unwrap_or_else(|| panic!("no Tap handler dispatching {event}::{case}"));
    let b = boxes.iter().find(|b| b.node_id == *box_id).expect("box");
    (b.rect.x.0 + b.rect.width.0 / 2, b.rect.y.0 + b.rect.height.0 / 2)
}

fn tap(view: &mut View, device: &FixtureEnv, symbols: &[String], (x, y): (i32, i32)) {
    let boxes = common::layout_boxes(view);
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols, keys: &keys };
    let mut host = NullHost;
    view.pointer_scrolled(
        &BaseTokens,
        device,
        &locale,
        &mut host,
        &boxes,
        "Tap",
        FxPx::new(x),
        FxPx::new(y),
        None,
    )
    .expect("tap runs");
}

fn power(view: &View, field: &str) -> Value {
    view.runtime.field("PowerStore", field).cloned().expect("power field")
}

#[test]
fn power_button_opens_the_alert_and_cancel_escape_confirm_close_it() {
    let nxir = common::compile("desktop-shell");
    let device = FixtureEnv::desktop();
    let symbols = common::program_symbols(&nxir);
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view = View::mount(&nxir, &BaseTokens, &device, &locale).expect("mounts");
    assert_eq!(power(&view, "powerPrompt"), Value::Bool(false));
    assert_eq!(view.overlays().modal_depth(), 0);

    // The pill, then the power button — the injector's two presses.
    let boxes = common::layout_boxes(&view);
    let pill = center_of(&view, &boxes, &symbols, "PanelEvent", "SetPanel", Some("control"));
    assert_eq!(pill, INJECT_CC_PILL, "the injector's Control-Center pill target moved");
    tap(&mut view, &device, &symbols, pill);
    let boxes = common::layout_boxes(&view);
    let button = center_of(&view, &boxes, &symbols, "PowerEvent", "PowerAsk", None);
    assert_eq!(button, INJECT_POWER_BUTTON, "the injector's power-button target moved");
    tap(&mut view, &device, &symbols, button);
    assert_eq!(power(&view, "powerPrompt"), Value::Bool(true));
    assert_eq!(view.overlays().modal_depth(), 1, "the alert is the shell's modal");

    // Background press while open: the alert absorbs — nothing opens, nothing closes.
    tap(&mut view, &device, &symbols, INJECT_BACKGROUND);
    tap(&mut view, &device, &symbols, pill);
    assert_eq!(power(&view, "powerPrompt"), Value::Bool(true));
    assert_eq!(view.take_dismissed(), None);

    // ESC cancels.
    let mut host = NullHost;
    view.dismiss_top(&BaseTokens, &device, &locale, &mut host, DismissReason::Escape).expect("esc");
    assert_eq!(view.take_dismissed(), Some(DismissReason::Escape));
    assert_eq!(power(&view, "powerPrompt"), Value::Bool(false));
    assert_eq!(power(&view, "toast"), Value::Str(String::new()));

    // Cancel button cancels.
    tap(&mut view, &device, &symbols, button);
    let boxes = common::layout_boxes(&view);
    let cancel = center_of(&view, &boxes, &symbols, "PowerEvent", "AlertCancel", Some("power"));
    tap(&mut view, &device, &symbols, cancel);
    assert_eq!(power(&view, "powerPrompt"), Value::Bool(false));

    // Confirm answers with the toast (honest: no power service), which times out.
    tap(&mut view, &device, &symbols, button);
    let boxes = common::layout_boxes(&view);
    let confirm = center_of(&view, &boxes, &symbols, "PowerEvent", "AlertConfirm", Some("power"));
    assert_eq!(confirm, INJECT_CONFIRM_BUTTON, "the injector's Confirm target moved");
    tap(&mut view, &device, &symbols, confirm);
    assert_eq!(power(&view, "powerPrompt"), Value::Bool(false));
    assert_eq!(power(&view, "toast"), Value::Str("power".to_string()));
    assert_eq!(view.overlays().modal_depth(), 0);
    let toast = view.overlays().transients().next().cloned().expect("toast up");
    assert_eq!(toast.dismiss_after_ms, Some(4000));
    view.dismiss_at(&BaseTokens, &device, &locale, &mut host, &toast.path, DismissReason::Timeout)
        .expect("timeout");
    assert_eq!(view.take_dismissed(), Some(DismissReason::Timeout));
    assert_eq!(power(&view, "toast"), Value::Str(String::new()));
    assert!(view.overlays().is_empty());
}
