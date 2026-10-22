// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
// A long session never runs an app's program reads dry (TASK-0344). The runtime
// once read an app's program through ONE capnp reader for the app's life; the
// reader's traversal budget only counts down, so settings answered every read
// `Malformed` after 28 menu toggles and the greeter's field after 482
// keystrokes (board cycle 62: taps that do nothing, typing that stops, then the
// chat's crash). Each operation reads through a fresh reader now — these
// sessions run thirty and ten times past where the old one died.

mod common;

use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, NoIo, Value, View};

#[test]
fn a_long_session_never_runs_the_program_reads_dry() {
    let device = FixtureEnv::tablet("landscape");
    let tokens = nexus_theme_tokens::BaseTokens;
    let mut host = NoIo;

    // settings: the window menu opened and closed — every toggle a dispatch
    // and an emit of the whole page.
    let nxir = common::compile("settings");
    let symbols = common::program_symbols(&nxir);
    let keys = common::program_i18n_keys(&nxir);
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("settings mounts");
    let (e, c) = view.runtime().event_case("WindowEvent", "WinMenu").expect("WinMenu");
    for toggle in 0..1_000 {
        let menu = if toggle % 2 == 0 { "app" } else { "" };
        let result = view.dispatch(
            &tokens,
            &device,
            &locale,
            &mut host,
            e,
            c,
            vec![Value::Str(menu.into())],
        );
        assert!(result.is_ok(), "settings toggle {toggle}: {result:?}");
    }

    // greeter: a character typed and deleted, again and again.
    let nxir = common::compile("greeter");
    let symbols = common::program_symbols(&nxir);
    let keys = common::program_i18n_keys(&nxir);
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("greeter mounts");
    let boxes = common::layout_boxes(&view);
    let field = view
        .handlers()
        .iter()
        .find(|(_, h)| matches!(h.action, nexus_dsl_runtime::interact::HandlerAction::Bind { .. }))
        .map(|(id, _)| *id)
        .expect("the greeter has a text field");
    let rect = boxes.iter().find(|b| b.node_id == field).expect("field box").rect;
    let (cx, cy) = (rect.x.0 + rect.width.0 / 2, rect.y.0 + rect.height.0 / 2);
    let at = |v: i32| nexus_layout_types::FxPx::new(v);
    assert!(view.focus_text_at(&boxes, at(cx), at(cy), &[]).is_some(), "focus lands");
    for key in 0..5_000 {
        let result = if key % 2 == 0 {
            view.insert_text(&tokens, &device, &locale, &mut host, "a").map(|_| ())
        } else {
            view.backspace_text(&tokens, &device, &locale, &mut host).map(|_| ())
        };
        assert!(result.is_ok(), "greeter keystroke {key}: {result:?}");
    }
}
