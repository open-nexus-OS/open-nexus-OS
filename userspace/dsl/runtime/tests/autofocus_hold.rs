// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! `.autofocus(true)` holds the keyboard (TASK-0067B board round). A press on a control
//! beside an autofocus field — the search's category buttons — keeps the focus in the field.
//! Clearing it would hand it back one present later through `autofocus_box`, and a key typed
//! in that gap would land nowhere. A press on another field still moves the focus, and a
//! field without `.autofocus` keeps the old rule: a press on no field clears it.

// reason: test harness — a failed compile/mount step must panic loudly.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use nexus_dsl_runtime::interact::{HandlerAction, HandlerEntry};
use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, View};
use nexus_layout::LayoutBox;
use nexus_layout_types::FxPx;

const PAGE: &str = r#"Store S {
    query: Str = "",
    other: Str = "",
    presses: Int = 0,
}

Event E {
    Pressed,
    Typed,
}

reduce E {
    Pressed => state.presses = state.presses + 1,
    Typed => state.presses = state.presses,
}

Page P {
    Stack {
        Stack {
            TextField { label: "Search", value: $state.query }
            .autofocus(true)
        }
        on Change -> dispatch(Typed)
        Stack {
            Text("Clipboard")
        }
        .height(40)
        on Tap -> dispatch(Pressed)
        Stack {
            TextField { label: "Other", value: $state.other }
        }
        on Change -> dispatch(Typed)
    }
}"#;

fn compile(src: &str) -> Vec<u8> {
    let file = nexus_dsl_core::parse_file(src).expect("parses");
    let (model, diags) = nexus_dsl_core::check_file(&file);
    assert!(!nexus_dsl_core::has_errors(&diags), "check: {diags:?}");
    let canonical = nexus_dsl_core::format_file(&file);
    nexus_dsl_core::lower_file(&file, &model, &canonical).expect("lowers").nxir
}

fn layout(view: &View<'_>) -> Vec<LayoutBox> {
    nexus_layout::LayoutEngine::new()
        .layout_with_viewport(
            view.scene(),
            FxPx::new(320),
            Some(FxPx::new(240)),
            &nexus_text_baked::measure_text::BakedTextMeasure,
        )
        .expect("lays out")
        .boxes
}

fn centre(boxes: &[LayoutBox], box_id: usize) -> (FxPx, FxPx) {
    let b = boxes.iter().find(|b| b.node_id == box_id).expect("box");
    (FxPx::new(b.rect.x.0 + b.rect.width.0 / 2), FxPx::new(b.rect.y.0 + b.rect.height.0 / 2))
}

/// The box of the first handler matching `pick`.
fn handler_box(view: &View<'_>, pick: impl Fn(&HandlerEntry) -> bool) -> usize {
    view.handlers().iter().find(|(_, e)| pick(e)).map(|(b, _)| *b).expect("handler")
}

fn with_view(f: impl FnOnce(&mut View<'_>, &[LayoutBox], usize, usize, usize)) {
    let nxir = compile(PAGE);
    let (symbols, keys): (Vec<String>, Vec<u32>) = (Vec::new(), Vec::new());
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let tokens = nexus_theme_tokens::BaseTokens;
    let mut view = View::mount(&nxir, &tokens, &FixtureEnv::default(), &locale).expect("mounts");
    let boxes = layout(&view);
    let sym = |n: &str| view.runtime().symbols().iter().position(|s| s == n).unwrap() as u32;
    let (tap, change) = (sym("Tap"), sym("Change"));
    let search = view.autofocus_box().expect("the autofocus field asks for the keyboard");
    let button = handler_box(&view, |e| {
        e.trigger == tap && matches!(e.action, HandlerAction::Dispatch { .. })
    });
    let other = handler_box(&view, |e| {
        e.trigger == change && !e.autofocus && matches!(e.action, HandlerAction::Bind { .. })
    });
    f(&mut view, &boxes, search, button, other);
}

#[test]
fn a_press_beside_an_autofocus_field_keeps_its_focus() {
    with_view(|view, boxes, search, button, _| {
        let (x, y) = centre(boxes, search);
        let focused = view.focus_text_at(boxes, x, y, None).expect("focuses");
        assert_eq!(focused.box_id, search);
        let (x, y) = centre(boxes, button);
        let after = view.focus_text_at(boxes, x, y, None);
        assert_eq!(after.map(|s| s.box_id), Some(search), "the button press keeps the keyboard");
        assert_eq!(view.autofocus_box(), None, "nothing to hand back — the focus never left");
    });
}

#[test]
fn a_press_on_another_field_moves_the_focus_and_its_old_rule_applies() {
    with_view(|view, boxes, search, button, other| {
        let (x, y) = centre(boxes, search);
        let _ = view.focus_text_at(boxes, x, y, None).expect("focuses");
        let (x, y) = centre(boxes, other);
        let moved = view.focus_text_at(boxes, x, y, None).expect("moves");
        assert_ne!(moved.box_id, search, "another field takes the focus");
        // A field without `.autofocus` keeps the old rule: a press on no field clears it,
        // and the autofocus field asks for the keyboard again.
        let (x, y) = centre(boxes, button);
        assert_eq!(view.focus_text_at(boxes, x, y, None), None);
        assert_eq!(view.autofocus_box(), Some(search));
    });
}
