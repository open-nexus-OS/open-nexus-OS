// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! TASK-0074 P4: pixel goldens of the kit's overlay components — `WinAlert`, `WinModal`,
//! `WinToast` (the design handoff's Alert / Modal / System toast) — light and dark, through
//! the shared BGRA golden painter. Regenerate with `UPDATE_GOLDENS=1`.

// reason: test harness — a failed compile/mount step must panic loudly.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use dsl_goldens::compile;
use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, OverlayKind, Value, View};
use nexus_theme_tokens::{DarkTokens, LightTokens, Tokens};

const ALERT: &str = include_str!("../../../userspace/apps/window-kit/ui/components/WinAlert.nx");
const MODAL: &str = include_str!("../../../userspace/apps/window-kit/ui/components/WinModal.nx");
const TOAST: &str = include_str!("../../../userspace/apps/window-kit/ui/components/WinToast.nx");

const APP: &str = r#"
Store S {
    which: Str = "alert",
}

Event E {
    Show(Str),
    AlertConfirm(Str),
    AlertCancel(Str),
    ModalClose(Str),
    ToastDismiss(Str),
    WinNoop,
}

reduce E {
    Show(w) => state.which = w,
    AlertConfirm(id) => state.which = "",
    AlertCancel(id) => state.which = "",
    ModalClose(id) => state.which = "",
    ToastDismiss(id) => state.which = "",
    WinNoop => state.which = state.which,
}

Page P {
    Stack {
        Text("Behind the overlay").textSize(lg)
        Button { label: "Open" } on Tap -> dispatch(Show("alert"))
        if $state.which == "alert" {
            WinAlert {
                id: "a",
                title: "Shut down?",
                message: "The device will be powered off.",
                cancelLabel: "Cancel",
                confirmLabel: "Shut down",
                destructive: true,
            }
        } else {
            Stack { }
        }
        if $state.which == "modal" {
            WinModal { id: "m", title: "Rename item" } {
                body {
                    Text("A body of text the app owns.").textSize(sm)
                }
                footer {
                    Button { label: "Cancel" }.bg(surfaceVariant) on Tap -> dispatch(ModalClose("m"))
                    Button { label: "Save" } on Tap -> dispatch(WinNoop)
                }
            }
        } else {
            Stack { }
        }
        if $state.which == "toast" {
            WinToast {
                id: "t",
                icon: "power",
                title: "Shutdown not available",
                subtitle: "No power service is connected yet.",
            }
        } else {
            Stack { }
        }
    }
    .padding(4)
    .gap(2)
}
"#;

fn mount<'p>(nxir: &'p [u8], tokens: &dyn Tokens, which: &str) -> (View<'p>, Vec<String>) {
    let symbols = nexus_dsl_runtime::Runtime::mount(nxir).expect("pre-mount").symbols().to_vec();
    let keys: Vec<u32> = Vec::new();
    let mut view = {
        let locale = IdentityLocale { symbols: &symbols, keys: &keys };
        View::mount(nxir, tokens, &FixtureEnv::desktop(), &locale).expect("mounts")
    };
    let (e, c) = view.runtime.event_case("E", "Show").expect("Show");
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    view.dispatch(
        tokens,
        &FixtureEnv::desktop(),
        &locale,
        &mut nexus_dsl_runtime::NoIo,
        e,
        c,
        vec![Value::Str(which.to_string())],
    )
    .expect("show");
    (view, symbols)
}

fn program() -> String {
    format!("{ALERT}\n{MODAL}\n{TOAST}\n{APP}")
}

#[test]
fn kit_overlays_match_their_goldens_light_and_dark() {
    let nxir = compile(&program());
    for (theme, tokens) in [("light", &LightTokens as &dyn Tokens), ("dark", &DarkTokens)] {
        for which in ["alert", "modal", "toast"] {
            let (view, _) = mount(&nxir, tokens, which);
            ui_v10_goldens::check_golden(&format!("kit_{which}_{theme}"), view.scene()).unwrap();
        }
    }
}

/// The components declare the kinds the runtime acts on: alert and modal are modal layers
/// (depth 1), the toast is a transient with the handoff's 4 s lifetime.
#[test]
fn kit_overlays_declare_their_kinds() {
    let nxir = compile(&program());
    let (view, _) = mount(&nxir, &DarkTokens, "alert");
    assert_eq!(view.overlays().modal_depth(), 1);
    let (view, _) = mount(&nxir, &DarkTokens, "modal");
    assert_eq!(view.overlays().modal_depth(), 1);
    let (view, _) = mount(&nxir, &DarkTokens, "toast");
    assert_eq!(view.overlays().modal_depth(), 0);
    let toast = view.overlays().transients().next().expect("toast");
    assert_eq!(toast.kind, OverlayKind::Transient);
    assert_eq!(toast.dismiss_after_ms, Some(4000));
    let (view, _) = mount(&nxir, &DarkTokens, "");
    assert!(view.overlays().is_empty());
}
