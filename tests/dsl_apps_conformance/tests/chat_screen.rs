// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
// The chat's main screen (the design artifacts' Chats view): the conversation list narrows by
// chip, opening a row fills the thread header, the empty draft sends nothing, and on the compact
// tier the list and the thread take turns (a pushed thread, its back button).

mod common;

use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, NoIo, Value, View};

struct Screen<'a> {
    view: View<'a>,
    device: FixtureEnv,
    symbols: Vec<String>,
    keys: Vec<u32>,
}

impl<'a> Screen<'a> {
    fn mount(nxir: &'a [u8], device: FixtureEnv) -> Self {
        let symbols = common::program_symbols(nxir);
        let keys = common::program_i18n_keys(nxir);
        let tokens = nexus_theme_tokens::BaseTokens;
        let mut view = {
            let locale = IdentityLocale { symbols: &symbols, keys: &keys };
            View::mount(nxir, &tokens, &device, &locale).expect("mounts")
        };
        let locale = IdentityLocale { symbols: &symbols, keys: &keys };
        view.run_initial_effects(&tokens, &device, &locale, &mut NoIo).expect("initial effects");
        Self { view, device, symbols, keys }
    }

    fn dispatch(&mut self, event: &str, case: &str, payload: Vec<Value>) {
        common::dispatch_with_keys(
            &mut self.view,
            &self.device,
            &mut NoIo,
            &self.symbols,
            &self.keys,
            event,
            case,
            payload,
        );
    }

    fn texts(&self) -> Vec<String> {
        common::scene_texts(&self.view)
    }

    fn shows(&self, text: &str) -> bool {
        self.texts().iter().any(|t| t == text)
    }
}

fn s(text: &str) -> Value {
    Value::Str(text.into())
}

#[test]
fn the_first_conversation_is_open_and_the_chips_narrow_the_list() {
    let nxir = common::compile("chat");
    let mut screen = Screen::mount(&nxir, FixtureEnv::tablet("landscape"));
    // The labels are the baked English catalog. Lena is open at launch: her address is in the thread header, her row in the list.
    assert!(screen.shows("@lena:nexus.chat"), "the first conversation is not open");
    assert!(screen.shows("Dad") && screen.shows("Nordbahn Support"), "rows missing");
    // Unread keeps the three conversations with unread messages; verified keeps the two verified.
    screen.dispatch("ListEvent", "SetChip", vec![s("unread")]);
    assert!(screen.shows("#product-team") && screen.shows("Atlas Store"));
    assert!(!screen.shows("Dad") && !screen.shows("Nordbahn Support"), "unread filter leaks");
    screen.dispatch("ListEvent", "SetChip", vec![s("verified")]);
    assert!(screen.shows("Nordbahn Support") && !screen.shows("#product-team"));
    // A space on the rail narrows too; the search text matches a conversation's name.
    screen.dispatch("ListEvent", "SetChip", vec![s("all")]);
    screen.dispatch("ListEvent", "SetSpace", vec![s("community")]);
    assert!(screen.shows("Studio Collective") && !screen.shows("Dad"));
    screen.dispatch("ListEvent", "SetSpace", vec![s("all")]);
}

#[test]
fn opening_a_row_fills_the_thread_header() {
    let nxir = common::compile("chat");
    let mut screen = Screen::mount(&nxir, FixtureEnv::tablet("landscape"));
    assert!(!screen.shows("+49 151 0000 0001"));
    screen.dispatch(
        "ThreadEvent",
        "OpenThread",
        vec![
            s("papa"),
            s("Dad"),
            s("PA"),
            s("+49 151 0000 0001"),
            s("SMS"),
            s("label"),
            s("RCS"),
            s("accent"),
            Value::Bool(false),
            Value::Bool(true),
            Value::Bool(false),
            s("family"),
            Value::Bool(true),
        ],
    );
    assert!(screen.shows("+49 151 0000 0001"), "the header did not take the conversation");
    // A conversation without merging has no "Hinweis" section.
    assert!(!screen.shows("Note") && !screen.shows("Matrix + phone number"), "note leaks");
}

#[test]
fn an_empty_draft_sends_nothing() {
    let nxir = common::compile("chat");
    let mut screen = Screen::mount(&nxir, FixtureEnv::tablet("landscape"));
    screen.dispatch("ChatEvent", "Send", vec![]);
    assert!(!screen.shows("You"), "an empty draft left a message behind");
}

#[test]
fn compact_shows_the_list_then_the_pushed_thread() {
    let nxir = common::compile("chat");
    let mut screen = Screen::mount(&nxir, FixtureEnv::phone("portrait"));
    // The list first: its search field's title is there, the composer is not.
    assert!(screen.shows("Chats") && !screen.shows("Send"), "compact opens on the thread");
    screen.dispatch(
        "ThreadEvent",
        "OpenThread",
        vec![
            s("team"),
            s("#product-team"),
            s("PT"),
            s("#produkt-team:nexus.chat"),
            s("Matrix"),
            s("success"),
            s(""),
            s("accent"),
            Value::Bool(false),
            Value::Bool(false),
            Value::Bool(false),
            s("work"),
            Value::Bool(true),
        ],
    );
    assert!(screen.shows("Send"), "the pushed thread (composer) is not shown");
    screen.dispatch("ThreadEvent", "BackToList", vec![]);
    assert!(!screen.shows("Send"), "the back button did not return to the list");
}
