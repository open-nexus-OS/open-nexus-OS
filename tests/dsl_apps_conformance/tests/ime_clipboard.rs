// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! TASK-0067B / TASK-0353: the keyboard's clipboard against the REAL compiled `ime-ui` — the
//! toolbar is one row, composing or not (only the clipboard and close are live; emoji, voice,
//! settings and more take no press, nor do Shift and "123" in the keys); the clipboard swaps
//! the keys for CARDS of the history (`svc.clipboard.list`), a card's press reads the item's
//! full text (`svc.clipboard.read`) and inserts it through imed (`svc.ime.insert`); while the
//! IME composes, opening the clipboard, closing the keyboard and switching the layout commit
//! the composition first (`svc.ime.action("enter")`, design artifacts: OnScreenKeyboard).

// reason: test harness — a failed compile/mount step must panic loudly.
#![allow(clippy::expect_used, clippy::unwrap_used)]

mod common;

use nexus_dsl_runtime::interact::HandlerAction;
use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, Value, View};
use nexus_layout_types::FxPx;
use nexus_theme_tokens::BaseTokens;

const HISTORY: [(i64, &str); 2] =
    [(9, "Danke für die schnelle Antwort!"), (8, "Bestellnummer 4711-2026")];
const FULL_TEXT: &str = "Danke für die schnelle Antwort! (the full item)";

struct OskHost {
    seq_sym: u32,
    text_sym: u32,
    calls: Vec<String>,
}

impl nexus_dsl_runtime::EffectHost for OskHost {
    fn call(&mut self, service: &str, method: &str, args: &[Value]) -> Result<Value, u32> {
        let arg = match args.first() {
            Some(Value::Str(s)) => format!("{s:?}"),
            Some(Value::Int(i)) => i.to_string(),
            _ => String::new(),
        };
        self.calls.push(format!("{service}.{method}({arg})"));
        match (service, method) {
            ("clipboard", "list") => Ok(Value::List(
                HISTORY
                    .iter()
                    .map(|(seq, text)| {
                        let mut f = vec![
                            (self.seq_sym, Value::Int(*seq)),
                            (self.text_sym, Value::Str((*text).to_string())),
                        ];
                        f.sort_by_key(|(s, _)| *s);
                        Value::Record(f)
                    })
                    .collect(),
            )),
            ("clipboard", "read") => Ok(Value::Str(FULL_TEXT.to_string())),
            ("ime", "rows") => Ok(Value::List(Vec::new())),
            _ => Ok(Value::Bool(true)),
        }
    }
}

fn taps(view: &View, symbols: &[String]) -> Vec<(usize, (u32, u32, Vec<Value>))> {
    let tap = symbols.iter().position(|s| s == "Tap").expect("Tap") as u32;
    view.handlers()
        .iter()
        .filter(|(_, h)| h.trigger == tap)
        .filter_map(|(id, h)| match &h.action {
            HandlerAction::Dispatch { event, case, payload } => {
                Some((*id, (*event, *case, payload.clone())))
            }
            _ => None,
        })
        .collect()
}

fn press(
    view: &mut View,
    device: &FixtureEnv,
    symbols: &[String],
    host: &mut OskHost,
    box_id: usize,
) {
    let boxes = common::layout_boxes(view);
    let b = boxes.iter().find(|b| b.node_id == box_id).expect("box");
    let (x, y) = (b.rect.x.0 + b.rect.width.0 / 2, b.rect.y.0 + b.rect.height.0 / 2);
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols, keys: &keys };
    view.pointer_scrolled(
        &BaseTokens,
        device,
        &locale,
        host,
        &boxes,
        "Tap",
        FxPx::new(x),
        FxPx::new(y),
        &[],
    )
    .expect("tap runs");
}

fn find(
    view: &View,
    symbols: &[String],
    event: &str,
    case: &str,
    arg: Option<&Value>,
) -> Option<usize> {
    let (e, c) = view.runtime.event_case(event, case).expect("case");
    taps(view, symbols)
        .into_iter()
        .find(|(_, (ev, ca, p))| *ev == e && *ca == c && arg.is_none_or(|a| p.first() == Some(a)))
        .map(|(id, _)| id)
}

#[test]
fn the_toolbar_opens_the_cards_and_a_card_is_inserted() {
    let nxir = common::compile("ime-ui");
    let device = FixtureEnv::tablet("landscape");
    let symbols = common::program_symbols(&nxir);
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view = View::mount(&nxir, &BaseTokens, &device, &locale).expect("mounts");
    let sym = |n: &str| symbols.iter().position(|s| s == n).expect(n) as u32;
    let mut host = OskHost { seq_sym: sym("seq"), text_sym: sym("text"), calls: Vec::new() };

    // Idle: the toolbar's live entries are the clipboard and close; the bottom row has its
    // globe, space, "." and return (the host serves no rows here).
    let toggle =
        find(&view, &symbols, "OskClipEvent", "ClipToggle", None).expect("clipboard entry");
    assert_eq!(
        taps(&view, &symbols).len(),
        6,
        "toolbar: clipboard + close; bottom row: 4 — the disabled entries take no press"
    );

    press(&mut view, &device, &symbols, &mut host, toggle);
    assert!(host.calls.iter().any(|c| c == "clipboard.list(\"\")"), "{:?}", host.calls);
    assert_eq!(
        view.runtime.field("OskClipStore", "oskPanel").cloned(),
        Some(Value::Str("clipboard".into()))
    );
    let texts = common::scene_texts(&view);
    assert!(texts.iter().any(|t| t == HISTORY[0].1), "the newest card shows: {texts:?}");

    let card = find(&view, &symbols, "OskClipEvent", "ClipPaste", Some(&Value::Int(HISTORY[0].0)))
        .expect("newest card");
    press(&mut view, &device, &symbols, &mut host, card);
    let tail: Vec<_> = host.calls.iter().rev().take(2).rev().cloned().collect();
    assert_eq!(
        tail,
        ["clipboard.read(9)".to_string(), format!("ime.insert({FULL_TEXT:?})")],
        "the FULL text is read, then inserted"
    );

    // The clipboard entry toggles back to the keys.
    let toggle =
        find(&view, &symbols, "OskClipEvent", "ClipToggle", None).expect("clipboard entry");
    press(&mut view, &device, &symbols, &mut host, toggle);
    assert_eq!(
        view.runtime.field("OskClipStore", "oskPanel").cloned(),
        Some(Value::Str(String::new()))
    );
}

fn mount<'a>(nxir: &'a [u8], device: &FixtureEnv, symbols: &[String]) -> View<'a> {
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols, keys: &keys };
    View::mount(nxir, &BaseTokens, device, &locale).expect("mounts")
}

fn osk_host(symbols: &[String]) -> OskHost {
    let sym = |n: &str| symbols.iter().position(|s| s == n).expect(n) as u32;
    OskHost { seq_sym: sym("seq"), text_sym: sym("text"), calls: Vec::new() }
}

/// Puts the keyboard into a running composition: a preedit and two candidates.
fn compose(view: &mut View, device: &FixtureEnv, host: &mut OskHost, symbols: &[String]) {
    common::dispatch(
        view,
        device,
        host,
        symbols,
        "ImeStripEvent",
        "Preedit",
        vec![Value::Str("ka".to_string())],
    );
    let mut cands = vec![Value::Str("か".to_string()), Value::Str("カ".to_string())];
    cands.resize(8, Value::Str(String::new()));
    common::dispatch(view, device, host, symbols, "ImeStripEvent", "Cands", cands);
}

fn last_calls(host: &OskHost, n: usize) -> Vec<String> {
    host.calls.iter().rev().take(n).rev().cloned().collect()
}

#[test]
fn the_clipboard_commits_a_running_composition_first() {
    let nxir = common::compile("ime-ui");
    let device = FixtureEnv::tablet("landscape");
    let symbols = common::program_symbols(&nxir);
    let mut view = mount(&nxir, &device, &symbols);
    let mut host = osk_host(&symbols);
    compose(&mut view, &device, &mut host, &symbols);

    // The toolbar stays while the IME composes; its clipboard entry commits first.
    assert!(
        find(&view, &symbols, "OskClipEvent", "ClipToggle", None).is_none(),
        "while composing the clipboard opens through the committing event"
    );
    let open = find(&view, &symbols, "OskClipEvent", "ClipCommitOpen", None)
        .expect("the clipboard entry stays while composing");
    press(&mut view, &device, &symbols, &mut host, open);
    assert_eq!(
        last_calls(&host, 2),
        ["ime.action(\"enter\")".to_string(), "clipboard.list(\"\")".to_string()],
        "the composition is committed, then the history is listed"
    );
    assert_eq!(
        view.runtime.field("OskClipStore", "oskPanel").cloned(),
        Some(Value::Str("clipboard".into()))
    );
}

#[test]
fn closing_and_switching_commit_a_running_composition_first() {
    let nxir = common::compile("ime-ui");
    let device = FixtureEnv::tablet("landscape");
    let symbols = common::program_symbols(&nxir);
    let mut view = mount(&nxir, &device, &symbols);
    let mut host = osk_host(&symbols);
    compose(&mut view, &device, &mut host, &symbols);

    let cycle = find(&view, &symbols, "OskEvent", "CommitCycle", None).expect("globe key");
    press(&mut view, &device, &symbols, &mut host, cycle);
    assert_eq!(
        last_calls(&host, 2),
        ["ime.action(\"enter\")".to_string(), "ime.cycle(\"\")".to_string()],
        "imed resets the engine on a switch — the composition is committed before it"
    );

    let close = find(&view, &symbols, "OskEvent", "CommitDismiss", None).expect("close key");
    press(&mut view, &device, &symbols, &mut host, close);
    assert_eq!(
        last_calls(&host, 2),
        ["ime.action(\"enter\")".to_string(), "settings.set(\"window.control\")".to_string()],
        "the composition is committed, then the keyboard hides"
    );
}

#[test]
fn test_reject_unbacked_controls_take_no_press() {
    let nxir = common::compile("ime-ui");
    let device = FixtureEnv::tablet("landscape");
    let symbols = common::program_symbols(&nxir);
    let mut view = mount(&nxir, &device, &symbols);
    let mut host = osk_host(&symbols);
    let s = |t: &str| Value::Str(t.to_string());

    // Idle: exactly the six live controls take a press — emoji, voice, settings, more, Shift
    // and "123" are drawn disabled and have no handler at all.
    let idle: [(&str, &str, Option<Value>); 6] = [
        ("OskClipEvent", "ClipToggle", None),
        ("OskEvent", "Dismiss", None),
        ("OskEvent", "CycleLayout", None),
        ("OskEvent", "Key", Some(s(" "))),
        ("OskEvent", "Key", Some(s("."))),
        ("OskEvent", "Action", Some(s("enter"))),
    ];
    for (event, case, arg) in &idle {
        assert!(find(&view, &symbols, event, case, arg.as_ref()).is_some(), "{event}::{case}");
    }
    assert_eq!(taps(&view, &symbols).len(), idle.len(), "no press beyond the live controls");

    // Composing: the same controls through their committing events, plus one per candidate.
    compose(&mut view, &device, &mut host, &symbols);
    let composing: [(&str, &str, Option<Value>); 8] = [
        ("OskClipEvent", "ClipCommitOpen", None),
        ("OskEvent", "CommitDismiss", None),
        ("OskEvent", "CommitCycle", None),
        ("OskEvent", "Key", Some(s(" "))),
        ("OskEvent", "Key", Some(s("."))),
        ("OskEvent", "Action", Some(s("enter"))),
        ("OskEvent", "Select", Some(Value::Int(0))),
        ("OskEvent", "Select", Some(Value::Int(1))),
    ];
    for (event, case, arg) in &composing {
        assert!(find(&view, &symbols, event, case, arg.as_ref()).is_some(), "{event}::{case}");
    }
    assert_eq!(
        taps(&view, &symbols).len(),
        composing.len(),
        "an empty candidate slot takes no press either"
    );
    assert!(host.calls.is_empty(), "nothing was pressed: {:?}", host.calls);
}
