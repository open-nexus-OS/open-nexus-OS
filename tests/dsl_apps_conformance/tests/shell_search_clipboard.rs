// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! TASK-0067B: the shell search against the REAL compiled shell at the desktop layout — the
//! top-bar magnifier opens the search (a modal layer), Clipboard lists the history from
//! `svc.clipboard.list`, a card's press copies it back (`svc.clipboard.restore`), ESC closes;
//! Files is inert. Also the SSOT for the live lane's injector targets
//! (`tools/qmp_inject_modal.py`, clipboard phase): this test is the gate that they still land.

// reason: test harness — a failed compile/mount step must panic loudly.
#![allow(clippy::expect_used, clippy::unwrap_used)]

mod common;

use nexus_dsl_runtime::interact::HandlerAction;
use nexus_dsl_runtime::{
    Damage, DismissReason, EditCommand, FixtureEnv, IdentityLocale, Value, View,
};
use nexus_layout_types::FxPx;
use nexus_theme_tokens::BaseTokens;

/// Display-space centres at 1280×800 in the DESKTOP layout: the magnifier in the top bar,
/// the search's Clipboard button, and the first (newest) clipboard card.
pub const INJECT_SEARCH_PILL: (i32, i32) = (1220, 18);
pub const INJECT_SEARCH_CLIPBOARD: (i32, i32) = (958, 142);
pub const INJECT_CLIP_CARD: (i32, i32) = (419, 240);
/// The word the lane types into the search field before Ctrl+A, Ctrl+C: no prefill item
/// contains it, so no card exists until the copy re-reads the history.
pub const INJECT_COPY_WORD: &str = "kiwi";

/// The history the fake authority holds, newest first: `(seq, preview)`.
const HISTORY: [(i64, &str); 3] = [
    (6, "Danke für die schnelle Antwort!"),
    (5, "Bestellnummer 4711-2026"),
    (4, "Musterstraße 12, 10115 Berlin"),
];

/// A fake service host: records every call, answers the clipboard from `history`
/// (`HISTORY` at the start).
struct ClipHost {
    seq_sym: u32,
    text_sym: u32,
    history: Vec<(i64, String)>,
    calls: Vec<String>,
}

impl ClipHost {
    fn new(symbols: &[String]) -> Self {
        let sym = |n: &str| symbols.iter().position(|s| s == n).expect(n) as u32;
        let history = HISTORY.iter().map(|(seq, text)| (*seq, (*text).to_string())).collect();
        Self { seq_sym: sym("seq"), text_sym: sym("text"), history, calls: Vec::new() }
    }

    fn entries(&self) -> Value {
        Value::List(
            self.history
                .iter()
                .map(|(seq, text)| {
                    let mut fields = vec![
                        (self.seq_sym, Value::Int(*seq)),
                        (self.text_sym, Value::Str(text.clone())),
                    ];
                    fields.sort_by_key(|(s, _)| *s);
                    Value::Record(fields)
                })
                .collect(),
        )
    }
}

impl nexus_dsl_runtime::EffectHost for ClipHost {
    fn call(&mut self, service: &str, method: &str, args: &[Value]) -> Result<Value, u32> {
        let arg = match args.first() {
            Some(Value::Str(s)) => format!("{s:?}"),
            Some(Value::Int(i)) => i.to_string(),
            _ => String::new(),
        };
        self.calls.push(format!("{service}.{method}({arg})"));
        match (service, method) {
            ("clipboard", "list") => Ok(self.entries()),
            ("bundlemgr", "enumerate") => Ok(Value::List(Vec::new())),
            _ => Ok(Value::Bool(true)),
        }
    }
}

fn handler_center(
    view: &View,
    symbols: &[String],
    event: &str,
    case: &str,
    payload: Option<&Value>,
) -> Option<(i32, i32)> {
    let boxes = common::layout_boxes(view);
    let (e, c) = view.runtime.event_case(event, case).expect("case");
    let tap = symbols.iter().position(|s| s == "Tap").expect("Tap") as u32;
    let (box_id, _) = view.handlers().iter().find(|(_, h)| {
        h.trigger == tap
            && matches!(&h.action, HandlerAction::Dispatch { event, case, payload: p }
                if *event == e && *case == c && payload.is_none_or(|want| p.first() == Some(want)))
    })?;
    let b = boxes.iter().find(|b| b.node_id == *box_id).expect("box");
    Some((b.rect.x.0 + b.rect.width.0 / 2, b.rect.y.0 + b.rect.height.0 / 2))
}

fn tap(
    view: &mut View,
    device: &FixtureEnv,
    symbols: &[String],
    host: &mut ClipHost,
    (x, y): (i32, i32),
) {
    let boxes = common::layout_boxes(view);
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
        None,
    )
    .expect("tap runs");
}

fn search(view: &View, field: &str) -> Value {
    view.runtime.field("SearchStore", field).cloned().expect("search field")
}

#[test]
fn the_magnifier_opens_the_search_and_a_card_is_copied_back() {
    let nxir = common::compile("desktop-shell");
    let device = FixtureEnv::desktop();
    let symbols = common::program_symbols(&nxir);
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view = View::mount(&nxir, &BaseTokens, &device, &locale).expect("mounts");
    let mut host = ClipHost::new(&symbols);

    let pill =
        handler_center(&view, &symbols, "SearchEvent", "SearchOpen", None).expect("magnifier");
    assert_eq!(pill, INJECT_SEARCH_PILL, "the injector's magnifier target moved");
    tap(&mut view, &device, &symbols, &mut host, pill);
    assert_eq!(search(&view, "searchOpen"), Value::Bool(true));
    assert_eq!(view.overlays().modal_depth(), 1, "the search is the shell's modal");
    assert!(view.autofocus_box().is_some(), "the field asks for the keyboard");
    assert!(host.calls.iter().any(|c| c == "bundlemgr.enumerate(\"\")"), "{:?}", host.calls);

    let clip = Value::Str("clipboard".to_string());
    let button = handler_center(&view, &symbols, "SearchEvent", "SearchMode", Some(&clip))
        .expect("clipboard button");
    assert_eq!(button, INJECT_SEARCH_CLIPBOARD, "the injector's Clipboard target moved");
    tap(&mut view, &device, &symbols, &mut host, button);
    assert_eq!(search(&view, "searchMode"), clip);
    assert!(host.calls.iter().any(|c| c == "clipboard.list(\"\")"), "{:?}", host.calls);
    let texts = common::scene_texts(&view);
    assert!(texts.iter().any(|t| t == HISTORY[0].1), "the newest card shows: {texts:?}");

    let newest = Value::Int(HISTORY[0].0);
    let card = handler_center(&view, &symbols, "SearchEvent", "ClipRestore", Some(&newest))
        .expect("newest card");
    assert_eq!(card, INJECT_CLIP_CARD, "the injector's card target moved");
    tap(&mut view, &device, &symbols, &mut host, card);
    assert!(host.calls.iter().any(|c| c == "clipboard.restore(6)"), "{:?}", host.calls);
    assert_eq!(search(&view, "clipCopied"), Value::Bool(true));

    view.dismiss_top(&BaseTokens, &device, &locale, &mut host, DismissReason::Escape).expect("esc");
    assert_eq!(search(&view, "searchOpen"), Value::Bool(false));
    assert_eq!(view.overlays().modal_depth(), 0);
}

#[test]
fn test_reject_the_files_button_does_nothing_and_keeps_the_search_open() {
    let nxir = common::compile("desktop-shell");
    let device = FixtureEnv::desktop();
    let symbols = common::program_symbols(&nxir);
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view = View::mount(&nxir, &BaseTokens, &device, &locale).expect("mounts");
    let mut host = ClipHost::new(&symbols);
    let pill =
        handler_center(&view, &symbols, "SearchEvent", "SearchOpen", None).expect("magnifier");
    tap(&mut view, &device, &symbols, &mut host, pill);
    // The Files button sits between Apps and Clipboard; its press only absorbs.
    let apps = Value::Str("apps".to_string());
    let clip = Value::Str("clipboard".to_string());
    let a =
        handler_center(&view, &symbols, "SearchEvent", "SearchMode", Some(&apps)).expect("apps");
    let c =
        handler_center(&view, &symbols, "SearchEvent", "SearchMode", Some(&clip)).expect("clip");
    let files = ((a.0 + c.0) / 2, a.1);
    let before = host.calls.len();
    tap(&mut view, &device, &symbols, &mut host, files);
    assert_eq!(host.calls.len(), before, "no service call: {:?}", &host.calls[before..]);
    assert_eq!(search(&view, "searchOpen"), Value::Bool(true), "not a backdrop press");
    assert_eq!(search(&view, "searchMode"), Value::Str(String::new()));
}

#[test]
fn test_reject_no_magnifier_in_the_touch_profiles() {
    let nxir = common::compile("desktop-shell");
    let symbols = common::program_symbols(&nxir);
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    for device in [FixtureEnv::tablet("landscape"), FixtureEnv::phone("portrait")] {
        let view = View::mount(&nxir, &BaseTokens, &device, &locale).expect("mounts");
        assert!(
            handler_center(&view, &symbols, "SearchEvent", "SearchOpen", None).is_none(),
            "touch profiles search with the keyboard's clipboard, not the top bar"
        );
    }
}

/// TASK-0067B board round — the lane's copy step at the layout. Clipboard is chosen and the
/// typed word matches nothing: no card lies under the injector's card target. Ctrl+A selects
/// the word, Ctrl+C copies it (app-host's `edit_action`: `selected_text` → `svc.clipboard.write`
/// → the `ClipboardChanged` trigger) and the open search re-reads the history AT ONCE: the
/// copied word is the first card, exactly under `INJECT_CLIP_CARD`. The lane presses that
/// target after the copy, so its press finds a card only if the refresh ran.
#[test]
fn a_copy_in_the_open_search_refreshes_the_history_at_once() {
    let nxir = common::compile("desktop-shell");
    let device = FixtureEnv::desktop();
    let symbols = common::program_symbols(&nxir);
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view = View::mount(&nxir, &BaseTokens, &device, &locale).expect("mounts");
    let mut host = ClipHost::new(&symbols);
    tap(&mut view, &device, &symbols, &mut host, INJECT_SEARCH_PILL);
    tap(&mut view, &device, &symbols, &mut host, INJECT_SEARCH_CLIPBOARD);
    assert_eq!(search(&view, "searchMode"), Value::Str("clipboard".to_string()));

    // `.autofocus(true)` focuses the field through the tap path after a present.
    let field = view.autofocus_box().expect("the field asks for the keyboard");
    let boxes = common::layout_boxes(&view);
    let b = boxes.iter().find(|b| b.node_id == field).expect("field box");
    let (x, y) = (b.rect.x.0 + b.rect.width.0 / 2, b.rect.y.0 + b.rect.height.0 / 2);
    view.focus_text_at(&boxes, FxPx::new(x), FxPx::new(y), None).expect("focused");
    host.history.clear(); // the authority holds nothing that matches the word
    view.insert_text(&BaseTokens, &device, &locale, &mut host, INJECT_COPY_WORD).expect("types");
    let asked = format!("clipboard.list({INJECT_COPY_WORD:?})");
    assert!(host.calls.contains(&asked), "typing re-asks the history: {:?}", host.calls);
    let texts = common::scene_texts(&view);
    assert!(!texts.iter().any(|t| HISTORY.iter().any(|(_, h)| t == h)), "no card yet: {texts:?}");
    let copied_seq = Value::Int(7);
    let card = |view: &View| {
        handler_center(view, &symbols, "SearchEvent", "ClipRestore", Some(&copied_seq))
    };
    assert_eq!(card(&view), None, "nothing under the card target before the copy");

    // Ctrl+A, Ctrl+C — app-host's part of the copy.
    view.edit_text(&BaseTokens, &device, &locale, &mut host, EditCommand::SelectAll)
        .expect("selects");
    let copied = view.selected_text().expect("a selection to copy");
    assert_eq!(copied, INJECT_COPY_WORD);
    host.history.insert(0, (7, copied)); // `svc.clipboard.write` stored it as the newest
    let lists = host.calls.iter().filter(|c| c.starts_with("clipboard.list")).count();
    let damage = view
        .fire_trigger(&BaseTokens, &device, &locale, &mut host, "ClipboardChanged")
        .expect("fires");
    let relists = host.calls.iter().filter(|c| c.starts_with("clipboard.list")).count();
    assert_eq!(relists, lists + 1, "the history re-read once: {:?}", host.calls);
    // The panel read the list only through `len(...)` while it was empty: the change must
    // still re-emit (the runtime's dependency walk covers list operations).
    assert!(matches!(damage, Some(d) if d != Damage::None), "the panel re-emits: {damage:?}");
    assert_eq!(card(&view), Some(INJECT_CLIP_CARD), "the lane's press lands on the copied card");
    tap(&mut view, &device, &symbols, &mut host, INJECT_CLIP_CARD);
    assert!(host.calls.iter().any(|c| c == "clipboard.restore(7)"), "{:?}", host.calls);

    // Closed search: nothing listens, nothing is asked.
    view.dismiss_top(&BaseTokens, &device, &locale, &mut host, DismissReason::Escape).expect("esc");
    let n = host.calls.len();
    view.fire_trigger(&BaseTokens, &device, &locale, &mut host, "ClipboardChanged").expect("fires");
    assert_eq!(host.calls.len(), n, "no handler while the search is closed");
}
