// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! TASK-0066: the kit's window menu in a REAL app (settings): opening the app-chip dropdown
//! shows the tile rows, and every tile forwards exactly its zone verb through the app's
//! `WinAct` effect (`svc.settings.set("window.control", "zone.<name>")`) — the channel windowd's
//! one tiling verb hangs on. The inert "move to another device" entry forwards nothing.

// reason: test harness — a failed compile/mount step must panic loudly.
#![allow(clippy::expect_used, clippy::unwrap_used)]

mod common;

use nexus_dsl_runtime::interact::HandlerAction;
use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, Value, View};
use nexus_layout_types::FxPx;
use nexus_theme_tokens::BaseTokens;

/// Records every `settings.set` the app forwards.
#[derive(Default)]
struct Recorder {
    sets: Vec<(String, String)>,
}

impl nexus_dsl_runtime::EffectHost for Recorder {
    fn call(&mut self, service: &str, method: &str, args: &[Value]) -> Result<Value, u32> {
        if service == "settings" && method == "set" {
            if let (Some(Value::Str(k)), Some(Value::Str(v))) = (args.first(), args.get(1)) {
                self.sets.push((k.clone(), v.clone()));
            }
        }
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

#[test]
fn every_tile_in_the_window_menu_forwards_its_zone_verb() {
    let nxir = common::compile("settings");
    let device = FixtureEnv::desktop();
    let symbols = common::program_symbols(&nxir);
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view = View::mount(&nxir, &BaseTokens, &device, &locale).expect("mounts");
    let mut host = Recorder::default();
    // Open the app-chip dropdown the way the chip does.
    common::dispatch(
        &mut view,
        &device,
        &mut host,
        &symbols,
        "WindowEvent",
        "WinMenu",
        vec![Value::Str("app".to_string())],
    );
    let (e, c) = view.runtime.event_case("WindowEvent", "WinAct").expect("WinAct");
    let tap = symbols.iter().position(|s| s == "Tap").expect("Tap") as u32;
    for zone in ZONES {
        let boxes = common::layout_boxes(&view);
        let (box_id, _) = view
            .handlers()
            .iter()
            .find(|(_, h)| {
                h.trigger == tap
                    && matches!(&h.action, HandlerAction::Dispatch { event, case, payload }
                        if *event == e && *case == c && payload.first() == Some(&Value::Str(zone.to_string())))
            })
            .unwrap_or_else(|| panic!("menu tile {zone} is on screen"));
        let b = boxes.iter().find(|b| b.node_id == *box_id).expect("box");
        let (x, y) = (b.rect.x.0 + b.rect.width.0 / 2, b.rect.y.0 + b.rect.height.0 / 2);
        assert!(b.rect.width.0 >= 24 && b.rect.height.0 >= 24, "{zone} tile is a real target");
        view.pointer_scrolled(
            &BaseTokens,
            &device,
            &locale,
            &mut host,
            &boxes,
            "Tap",
            FxPx::new(x),
            FxPx::new(y),
            None,
        )
        .expect("tap runs");
        assert_eq!(
            host.sets.last(),
            Some(&("window.control".to_string(), zone.to_string())),
            "{zone} forwarded through window.control"
        );
        // The menu closes on an action; reopen for the next tile.
        common::dispatch(
            &mut view,
            &device,
            &mut host,
            &symbols,
            "WindowEvent",
            "WinMenu",
            vec![Value::Str("app".to_string())],
        );
    }
    assert_eq!(host.sets.len(), ZONES.len());
    // The inert entry: no handler reaches the host.
    let before = host.sets.len();
    let boxes = common::layout_boxes(&view);
    let inert = view
        .handlers()
        .iter()
        .find(|(_, h)| matches!(&h.action, HandlerAction::Dispatch { payload, .. } if payload.first() == Some(&Value::Str("device.move".to_string()))));
    assert!(inert.is_none(), "the disabled entry registers no handler");
    let _ = boxes;
    assert_eq!(host.sets.len(), before);
}
