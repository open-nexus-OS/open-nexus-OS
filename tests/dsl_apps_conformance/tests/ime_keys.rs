// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! RFC-0075 Phase 8b / TASK-0357: the keyboard's key area against the REAL compiled `ime-ui`
//! with real rows — the four `OskEvent::Rows*` lists in the shape `svc.ime.rows` returns
//! (`{label, key, action}` records from the keymaps SSOT). Every cell shows its label and its
//! tap sends its key; the keymap's action cell (⌫) sends its action and never an empty key; and
//! Shift and ⌫ come out alike, one and a half letter keys wide, whatever the number of letters
//! in the last row (the design artifacts' OnScreenKeyboard: weights ×2, a key 2, Shift and ⌫ 3).

// reason: test harness — a failed compile/mount step must panic loudly.
#![allow(clippy::expect_used, clippy::unwrap_used)]

mod common;

use std::collections::BTreeSet;

use nexus_dsl_runtime::interact::HandlerAction;
use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, Value, View};
use nexus_theme_tokens::BaseTokens;

/// The host: the rows arrive as events here, so no service answers anything of note.
struct Host;

impl nexus_dsl_runtime::EffectHost for Host {
    fn call(&mut self, service: &str, method: &str, _: &[Value]) -> Result<Value, u32> {
        match (service, method) {
            ("ime", "rows") | ("clipboard", "list") => Ok(Value::List(Vec::new())),
            _ => Ok(Value::Bool(true)),
        }
    }
}

/// A row as `svc.ime.rows` returns it: records of label, key and action.
fn row(symbols: &[String], cells: &[(String, String, String)]) -> Value {
    let sym = |n: &str| symbols.iter().position(|s| s == n).expect(n) as u32;
    Value::List(
        cells
            .iter()
            .map(|(label, key, action)| {
                let mut f = vec![
                    (sym("label"), Value::Str(label.clone())),
                    (sym("key"), Value::Str(key.clone())),
                    (sym("action"), Value::Str(action.clone())),
                ];
                f.sort_by_key(|(s, _)| *s);
                Value::Record(f)
            })
            .collect(),
    )
}

fn chars(s: &str) -> Vec<(String, String, String)> {
    s.chars().map(|c| (c.to_string(), c.to_string(), String::new())).collect()
}

/// The keyboard with its four rows loaded; the last row is `last` letters and ⌫.
fn keyboard<'a>(nxir: &'a [u8], symbols: &[String], device: &FixtureEnv, last: usize) -> View<'a> {
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols, keys: &keys };
    let mut view = View::mount(nxir, &BaseTokens, device, &locale).expect("mounts");
    let mut r3: Vec<_> = chars("zxcvbnmß").into_iter().take(last).collect();
    r3.push(("⌫".into(), String::new(), "backspace".into()));
    for (case, cells) in [
        ("Rows0", chars("1234567890")),
        ("Rows1", chars("qwertyuiop")),
        ("Rows2", chars("asdfghjkl")),
        ("Rows3", r3),
    ] {
        let rows = row(symbols, &cells);
        common::dispatch(&mut view, device, &mut Host, symbols, "OskEvent", case, vec![rows]);
    }
    view
}

/// Every tap that dispatches: (node id, event, case, the first payload when it is a string).
fn taps(view: &View, symbols: &[String]) -> Vec<(usize, u32, u32, Option<String>)> {
    let tap = symbols.iter().position(|s| s == "Tap").expect("Tap") as u32;
    view.handlers()
        .iter()
        .filter(|(_, h)| h.trigger == tap)
        .filter_map(|(id, h)| match &h.action {
            HandlerAction::Dispatch { event, case, payload } => {
                let arg = match payload.first() {
                    Some(Value::Str(s)) => Some(s.clone()),
                    _ => None,
                };
                Some((*id, *event, *case, arg))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn every_key_shows_its_label_and_sends_its_key() {
    let nxir = common::compile("ime-ui");
    let symbols = common::program_symbols(&nxir);
    for device in
        [FixtureEnv::phone("portrait"), FixtureEnv::tablet("landscape"), FixtureEnv::desktop()]
    {
        let view = keyboard(&nxir, &symbols, &device, 7);
        let texts = common::scene_texts(&view);
        let taps = taps(&view, &symbols);
        let (key_event, key_case) = view.runtime.event_case("OskEvent", "Key").expect("Key");
        let sent: BTreeSet<String> = taps
            .iter()
            .filter(|t| t.1 == key_event && t.2 == key_case)
            .filter_map(|t| t.3.clone())
            .collect();
        for c in "1234567890qwertyuiopasdfghjklzxcvbnm".chars() {
            let c = c.to_string();
            assert!(texts.contains(&c), "{c} is drawn: {texts:?}");
            assert!(sent.contains(&c), "{c} sends Key({c}): {sent:?}");
        }
        assert!(!sent.contains(""), "the action cell never sends an empty key");
        let (act_event, act_case) = view.runtime.event_case("OskEvent", "Action").expect("Action");
        assert!(
            taps.iter().any(|t| t.1 == act_event
                && t.2 == act_case
                && t.3.as_deref() == Some("backspace")),
            "⌫ sends Action(\"backspace\")"
        );
    }
}

#[test]
fn shift_and_backspace_are_one_and_a_half_keys_for_any_letter_count() {
    let nxir = common::compile("ime-ui");
    let symbols = common::program_symbols(&nxir);
    let device = FixtureEnv::tablet("landscape");
    for last in 6..=8 {
        let view = keyboard(&nxir, &symbols, &device, last);
        let (act_event, act_case) = view.runtime.event_case("OskEvent", "Action").expect("Action");
        let (key_event, key_case) = view.runtime.event_case("OskEvent", "Key").expect("Key");
        let taps = taps(&view, &symbols);
        let node = |event: u32, case: u32, arg: &str| {
            taps.iter()
                .find(|t| t.1 == event && t.2 == case && t.3.as_deref() == Some(arg))
                .expect(arg)
                .0
        };
        let boxes = common::layout_boxes(&view);
        let rect = |id: usize| boxes.iter().find(|b| b.node_id == id).expect("box").rect;
        let bs = rect(node(act_event, act_case, "backspace"));
        let letter = rect(node(key_event, key_case, "z"));
        // Shift is the row's first cell: the leftmost box of the row's height that ends left
        // of ⌫ (the row itself spans both).
        let shift = boxes
            .iter()
            .filter(|b| b.rect.y == bs.y && b.rect.height == bs.height)
            .filter(|b| b.rect.x.0 + b.rect.width.0 <= bs.x.0)
            .min_by_key(|b| b.rect.x.0)
            .expect("Shift box")
            .rect;
        assert!(shift.x.0 < bs.x.0, "{last} letters: Shift sits left of ⌫");
        assert!(
            (shift.width.0 - bs.width.0).abs() <= 1,
            "{last} letters: Shift {} wide, ⌫ {}",
            shift.width.0,
            bs.width.0
        );
        // Weight 3 against a letter's 2, less the letters' gaps: about 1.6 letters wide for
        // every count; a letters' weight that missed their count gives 1.8 or more.
        let ratio = f64::from(shift.width.0) / f64::from(letter.width.0);
        assert!(
            (1.45..=1.7).contains(&ratio),
            "{last} letters: Shift is {ratio:.2} letter keys wide ({} / {})",
            shift.width.0,
            letter.width.0
        );
    }
}
