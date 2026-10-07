// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! TASK-0067B: the focused field's caret and selection (the `nexus-textedit` engine inside
//! the runtime). Typing lands at the caret and replaces a selection; the arrows, Shift and
//! Ctrl+A move and select; the selection is what a copy takes and what a cut removes; a
//! password field never yields its text. Every value change still runs the page's
//! `on Change` (the live-search contract).

// reason: test harness — a failed compile/mount step must panic loudly.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use nexus_dsl_runtime::{EditCommand, EditResult, FixtureEnv, IdentityLocale, NoIo, Value, View};
use nexus_layout_types::FxPx;

const PAGE: &str = r#"Store S {
    query: Str = "",
    secret: Str = "",
    hits: Int = 0,
}

Event E {
    QueryChanged,
}

reduce E {
    QueryChanged => state.hits = state.hits + 1,
}

Page P {
    Stack {
        Stack {
            TextField { label: "Search", value: $state.query }
        }
        on Change -> dispatch(QueryChanged)
        TextField { label: "Secret", value: $state.secret, secure: true }
    }
}"#;

fn compile(src: &str) -> Vec<u8> {
    let file = nexus_dsl_core::parse_file(src).expect("parses");
    let (model, diags) = nexus_dsl_core::check_file(&file);
    assert!(!nexus_dsl_core::has_errors(&diags), "check: {diags:?}");
    let canonical = nexus_dsl_core::format_file(&file);
    nexus_dsl_core::lower_file(&file, &model, &canonical).expect("lowers").nxir
}

struct Field<'v, 'p> {
    view: &'v mut View<'p>,
}

impl Field<'_, '_> {
    fn type_text(&mut self, text: &str) {
        let tokens = nexus_theme_tokens::BaseTokens;
        let (symbols, keys) = (Vec::new(), Vec::new());
        let locale = IdentityLocale { symbols: &symbols, keys: &keys };
        self.view
            .insert_text(&tokens, &FixtureEnv::default(), &locale, &mut NoIo, text)
            .expect("insert");
    }

    fn cmd(&mut self, cmd: EditCommand) -> EditResult {
        let tokens = nexus_theme_tokens::BaseTokens;
        let (symbols, keys) = (Vec::new(), Vec::new());
        let locale = IdentityLocale { symbols: &symbols, keys: &keys };
        self.view.edit_text(&tokens, &FixtureEnv::default(), &locale, &mut NoIo, cmd).expect("edit")
    }

    fn cut(&mut self) -> EditResult {
        let tokens = nexus_theme_tokens::BaseTokens;
        let (symbols, keys) = (Vec::new(), Vec::new());
        let locale = IdentityLocale { symbols: &symbols, keys: &keys };
        self.view.cut_selection(&tokens, &FixtureEnv::default(), &locale, &mut NoIo).expect("cut")
    }

    fn field(&self, name: &str) -> Value {
        self.view.runtime().field("S", name).cloned().expect("field")
    }
}

/// Mounts the page and focuses the text field whose box is the `nth` focusable one.
fn with_focus(nth: usize, f: impl FnOnce(&mut Field<'_, '_>)) {
    let nxir = compile(PAGE);
    let tokens = nexus_theme_tokens::BaseTokens;
    let (symbols, keys): (Vec<String>, Vec<u32>) = (Vec::new(), Vec::new());
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view = View::mount(&nxir, &tokens, &FixtureEnv::default(), &locale).expect("mounts");
    let layout = nexus_layout::LayoutEngine::new()
        .layout_with_viewport(
            view.scene(),
            FxPx::new(320),
            Some(FxPx::new(240)),
            &nexus_text_baked::measure_text::BakedTextMeasure,
        )
        .expect("lays out");
    // Try each laid-out box until the nth distinct field takes focus.
    let mut seen = Vec::new();
    for b in layout.boxes.iter().filter(|b| b.rect.width.0 > 0 && b.rect.height.0 > 0) {
        let (x, y) = (b.rect.x.0 + b.rect.width.0 / 2, b.rect.y.0 + b.rect.height.0 / 2);
        if let Some(s) = view.focus_text_at(&layout.boxes, FxPx::new(x), FxPx::new(y), None) {
            if !seen.contains(&s.box_id) {
                seen.push(s.box_id);
                if seen.len() == nth + 1 {
                    break;
                }
            }
        }
    }
    assert_eq!(seen.len(), nth + 1, "field {nth} takes focus");
    f(&mut Field { view: &mut view });
}

#[test]
fn typing_lands_at_the_caret_and_replaces_the_selection() {
    with_focus(0, |f| {
        f.type_text("Hallo Welt");
        assert_eq!(f.cmd(EditCommand::Left), EditResult::Caret, "a move is a caret repaint");
        f.cmd(EditCommand::Left);
        f.cmd(EditCommand::Left);
        f.cmd(EditCommand::Left);
        f.type_text("schöne ");
        assert_eq!(f.field("query"), Value::Str("Hallo schöne Welt".into()));
        let snap = f.view.text_focus().expect("focused");
        assert_eq!((snap.caret, snap.selection), (13, None));
        // Shift+Left ×4 selects "öne " backwards; typing replaces it.
        for _ in 0..4 {
            f.cmd(EditCommand::SelectLeft);
        }
        assert_eq!(f.view.text_focus().map(|s| s.selection), Some(Some((9, 13))));
        assert_eq!(f.view.selected_text().as_deref(), Some("öne "));
        f.type_text("i ");
        assert_eq!(f.field("query"), Value::Str("Hallo schi Welt".into()));
    });
}

#[test]
fn select_all_copy_and_cut_work_on_the_whole_value() {
    with_focus(0, |f| {
        f.type_text("kopier mich");
        let hits = f.field("hits");
        assert_eq!(f.cmd(EditCommand::SelectAll), EditResult::Caret);
        assert_eq!(f.view.selected_text().as_deref(), Some("kopier mich"), "what Ctrl+C takes");
        assert_eq!(f.field("hits"), hits, "selecting is no change");
        assert!(matches!(f.cut(), EditResult::Value(_)), "a cut changes the value");
        assert_eq!(f.field("query"), Value::Str(String::new()));
        assert_eq!(f.view.selected_text(), None, "nothing left selected");
        assert_ne!(f.field("hits"), hits, "the cut ran the page's on Change");
    });
}

#[test]
fn backspace_and_delete_act_at_the_caret() {
    with_focus(0, |f| {
        f.type_text("abcd");
        f.cmd(EditCommand::Home);
        f.cmd(EditCommand::Delete);
        assert_eq!(f.field("query"), Value::Str("bcd".into()));
        f.cmd(EditCommand::End);
        f.cmd(EditCommand::Backspace);
        assert_eq!(f.field("query"), Value::Str("bc".into()));
    });
}

#[test]
fn test_reject_copy_and_cut_out_of_a_password_field() {
    with_focus(1, |f| {
        assert!(f.view.text_focus().is_some_and(|s| s.secure), "the secret field is focused");
        f.type_text("geheim");
        f.cmd(EditCommand::SelectAll);
        assert_eq!(f.view.selected_text(), None, "a password never leaves its field");
        assert_eq!(f.cut(), EditResult::None);
        assert_eq!(f.field("secret"), Value::Str("geheim".into()), "the cut did nothing");
    });
}
