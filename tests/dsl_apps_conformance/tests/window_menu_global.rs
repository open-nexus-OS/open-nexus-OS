// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! TASK-0066 (board cycle 2026-10-07): the window menu is the WINDOW's, not an app's. `WinAppWindow`
//! mounts the kit's `WinAppMenu` for every window-kit app — opening the app chip in settings AND
//! in the file manager shows the same eleven tile verbs, and the app's own menus mount no backdrop
//! above it while it is open (a later overlay would swallow the taps).

// reason: test harness — a failed compile/mount step must panic loudly.
#![allow(clippy::expect_used, clippy::unwrap_used)]

mod common;

use nexus_dsl_runtime::interact::HandlerAction;
use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, Value, View};
use nexus_theme_tokens::BaseTokens;

struct Sink;

impl nexus_dsl_runtime::EffectHost for Sink {
    fn call(&mut self, _service: &str, _method: &str, _args: &[Value]) -> Result<Value, u32> {
        Ok(Value::Bool(true))
    }
}

const ZONES: [&str; 11] = [
    "zone.left-half",
    "zone.right-half",
    "zone.top-left",
    "zone.top-right",
    "zone.bottom-left",
    "zone.bottom-right",
    "zone.fill",
    "zone.left-right",
    "zone.top-bottom",
    "zone.quarters",
    "zone.return",
];

/// Each app declares the kit's events in its own enum.
const APPS: [(&str, &str); 2] = [("settings", "WindowEvent"), ("stash", "StashEvent")];

/// The `WinAct` payloads of every Tap handler on screen.
fn tap_acts(view: &View, symbols: &[String], events: &str) -> Vec<String> {
    let (e, c) = view.runtime.event_case(events, "WinAct").expect("WinAct");
    let tap = symbols.iter().position(|s| s == "Tap").expect("Tap") as u32;
    view.handlers()
        .iter()
        .filter_map(|(_, h)| match &h.action {
            HandlerAction::Dispatch { event, case, payload }
                if h.trigger == tap && *event == e && *case == c =>
            {
                match payload.first() {
                    Some(Value::Str(s)) => Some(s.clone()),
                    _ => None,
                }
            }
            _ => None,
        })
        .collect()
}

/// The `WinMenu("")` backdrop layers on screen (one = the kit's own).
fn menu_backdrops(view: &View, symbols: &[String], events: &str) -> usize {
    let (e, c) = view.runtime.event_case(events, "WinMenu").expect("WinMenu");
    let tap = symbols.iter().position(|s| s == "Tap").expect("Tap") as u32;
    view.handlers()
        .iter()
        .filter(|(_, h)| {
            h.trigger == tap
                && matches!(&h.action, HandlerAction::Dispatch { event, case, payload }
                    if *event == e && *case == c && payload.first() == Some(&Value::Str(String::new())))
        })
        .count()
}

#[test]
fn every_window_kit_app_opens_the_same_window_menu() {
    for (app, events) in APPS {
        let nxir = common::compile(app);
        let device = FixtureEnv::desktop();
        let symbols = common::program_symbols(&nxir);
        let keys: Vec<u32> = Vec::new();
        let locale = IdentityLocale { symbols: &symbols, keys: &keys };
        let mut view = View::mount(&nxir, &BaseTokens, &device, &locale).expect("mounts");
        let mut host = Sink;
        let closed = tap_acts(&view, &symbols, events);
        for zone in ZONES {
            assert!(!closed.contains(&zone.to_string()), "{app}: {zone} hidden while closed");
        }
        common::dispatch(
            &mut view,
            &device,
            &mut host,
            &symbols,
            events,
            "WinMenu",
            vec![Value::Str("app".to_string())],
        );
        let open = tap_acts(&view, &symbols, events);
        for zone in ZONES {
            assert_eq!(
                open.iter().filter(|a| a.as_str() == zone).count(),
                1,
                "{app}: exactly one {zone} tile in the open menu"
            );
        }
        assert_eq!(
            menu_backdrops(&view, &symbols, events),
            1,
            "{app}: only the kit's backdrop is mounted"
        );
    }
}

#[test]
fn test_reject_window_menu_tiles_outside_the_open_menu() {
    // A closed menu registers no zone verb at all — no hidden hit box can tile a window.
    let nxir = common::compile("stash");
    let device = FixtureEnv::desktop();
    let symbols = common::program_symbols(&nxir);
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let view = View::mount(&nxir, &BaseTokens, &device, &locale).expect("mounts");
    assert!(tap_acts(&view, &symbols, "StashEvent").iter().all(|a| !a.starts_with("zone.")));
}
