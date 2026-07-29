// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Typing into a bound text field must run the page's `on Change ->
//! dispatch(E)`.
//!
//! Every page in the tree writes live search the same way —
//! `Stack { TextField { value: $state.q } } on Change -> dispatch(QueryChanged)`
//! (both launchers, the file manager) — and until this landed the dispatch
//! never ran. `Change` was consulted only to resolve focus and to pick the
//! I-beam cursor, so a keystroke moved the store field and stopped there: no
//! reducer arm, no `@effect`, no re-issued query. Live search did not work
//! anywhere in the OS.
//!
//! The second-order damage is worse than the missing filter. An event that
//! NOTHING dispatches is a ROOT effect (`initial::root_effect_events`), and
//! `run_initial_effects` dispatches roots through the REDUCER — so the app's
//! filter event fired once at mount, rewriting the initial state, and never
//! again.

// reason: test harness — a failed compile/mount step must panic loudly.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, NoIo, Value, View};
use nexus_layout_types::FxPx;

/// A page whose search field is wrapped exactly the way the shipped apps wrap
/// theirs. `hits` counts reducer runs, so the assertions cannot pass by the
/// binding alone.
const PAGE: &str = r#"Store S {
    query: Str = "",
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
    }
}"#;

fn compile(src: &str) -> Vec<u8> {
    let file = nexus_dsl_core::parse_file(src).expect("parses");
    let (model, diags) = nexus_dsl_core::check_file(&file);
    assert!(!nexus_dsl_core::has_errors(&diags), "check: {diags:?}\n{src}");
    let canonical = nexus_dsl_core::format_file(&file);
    nexus_dsl_core::lower_file(&file, &model, &canonical).expect("lowers").nxir
}

struct Mounted {
    nxir: Vec<u8>,
}

impl Mounted {
    fn new(src: &str) -> Self {
        Self { nxir: compile(src) }
    }

    fn with<R>(&self, f: impl FnOnce(&mut View<'_>, &IdentityLocale<'_>) -> R) -> R {
        let tokens = nexus_theme_tokens::BaseTokens;
        let device = FixtureEnv::default();
        let symbols: Vec<String> = Vec::new();
        let keys: Vec<u32> = Vec::new();
        let locale = IdentityLocale { symbols: &symbols, keys: &keys };
        let mut view = View::mount(&self.nxir, &tokens, &device, &locale).expect("mounts");
        f(&mut view, &locale)
    }
}

/// Lays out, taps the field's centre to focus it, then types `text`.
/// Returns `(query, hits)`.
fn type_into(view: &mut View<'_>, locale: &IdentityLocale<'_>, text: &str) -> (String, i64) {
    let tokens = nexus_theme_tokens::BaseTokens;
    let device = FixtureEnv::default();
    let engine = nexus_layout::LayoutEngine::new();
    let layout = engine
        .layout_with_viewport(
            view.scene(),
            FxPx::new(320),
            Some(FxPx::new(240)),
            &nexus_text_baked::measure_text::BakedTextMeasure,
        )
        .expect("lays out");

    // The bound field is the innermost Change handler; focus by tapping it.
    let field = view.text_focus();
    assert!(field.is_none(), "nothing focused before the tap");
    let hit = layout
        .boxes
        .iter()
        .find(|b| b.rect.width.0 > 0 && b.rect.height.0 > 0 && b.node_id > 1)
        .expect("a laid-out box");
    let (x, y) = (
        FxPx::new(hit.rect.x.0 + hit.rect.width.0 / 2),
        FxPx::new(hit.rect.y.0 + hit.rect.height.0 / 2),
    );
    view.focus_text_at(&layout.boxes, x, y, None).expect("the field takes focus");

    for ch in text.chars() {
        let mut buf = [0u8; 4];
        view.insert_text(&tokens, &device, locale, &mut NoIo, ch.encode_utf8(&mut buf))
            .expect("insert");
    }

    let query = match view.runtime().field("S", "query") {
        Some(Value::Str(s)) => s.clone(),
        other => panic!("query is not a Str: {other:?}"),
    };
    let hits = match view.runtime().field("S", "hits") {
        Some(Value::Int(n)) => *n,
        other => panic!("hits is not an Int: {other:?}"),
    };
    (query, hits)
}

/// THE regression. Before this landed `hits` stayed 0 — the binding moved and
/// nothing else happened, which is why the file manager's filter only appeared
/// on the next unrelated interaction ("dauert ewig").
#[test]
fn typing_runs_the_enclosing_change_dispatch() {
    Mounted::new(PAGE).with(|view, locale| {
        let (query, hits) = type_into(view, locale, "abc");
        assert_eq!(query, "abc", "the binding still writes");
        assert_eq!(hits, 3, "one dispatch per keystroke, not zero");
    });
}

/// Backspace is an edit too — a filter must widen again when the user deletes.
#[test]
fn backspace_runs_the_dispatch_as_well() {
    Mounted::new(PAGE).with(|view, locale| {
        let (_, hits_after_typing) = type_into(view, locale, "ab");
        let tokens = nexus_theme_tokens::BaseTokens;
        let device = FixtureEnv::default();
        view.backspace_text(&tokens, &device, locale, &mut NoIo).expect("backspace");
        let hits = match view.runtime().field("S", "hits") {
            Some(Value::Int(n)) => *n,
            other => panic!("hits is not an Int: {other:?}"),
        };
        assert_eq!(hits, hits_after_typing + 1);
    });
}

/// Focus survives the re-emit each keystroke causes, and so must the resolved
/// dispatch — it is re-resolved in `revalidate_text_focus` by handler PATH,
/// because box ids do not survive an emit but paths do. Without that, only the
/// first character would dispatch.
#[test]
fn the_dispatch_survives_the_re_emit_every_keystroke_causes() {
    Mounted::new(PAGE).with(|view, locale| {
        let (_, hits) = type_into(view, locale, "abcdef");
        assert!(view.text_focus().is_some(), "focus survives");
        assert_eq!(hits, 6, "every keystroke dispatched, not just the first");
    });
}

/// A field with NO enclosing dispatch must keep working exactly as before —
/// the greeter's passphrase field is this shape, and it must not gain a
/// phantom event.
#[test]
fn a_field_without_a_wrapper_dispatch_only_writes_its_binding() {
    const BARE: &str = r#"Store S {
    query: Str = "",
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
        TextField { label: "Search", value: $state.query }
    }
}"#;
    Mounted::new(BARE).with(|view, locale| {
        let (query, hits) = type_into(view, locale, "ab");
        assert_eq!(query, "ab");
        assert_eq!(hits, 0, "no wrapper, no dispatch");
    });
}
