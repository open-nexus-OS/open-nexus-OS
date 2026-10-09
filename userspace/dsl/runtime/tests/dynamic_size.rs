// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Sizes that follow the store (RFC-0095, TASK-0068): `.width($state.w)` / `.height($state.h)`
//! take the expression's Int as raw px — and a dispatch that changes it re-lays the node out
//! (a LAYOUT dependency). Before, the checker accepted the form and the emitter dropped it: a
//! silent no-op. A value that is not an Int sizes nothing; a negative one clamps to 0. (The
//! column is `.align(start)`: a stretched cross axis overrides any width, literal or not.)

// reason: test harness — a failed compile/mount step must panic loudly.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use nexus_dsl_runtime::{Damage, FixtureEnv, IdentityLocale, NoIo, View};
use nexus_layout::LayoutBox;
use nexus_layout_types::FxPx;

const PAGE: &str = r#"Store S {
    w: Int = 123,
    h: Int = 45,
    label: Str = "wide",
}

Event E {
    Grow,
    Shrink,
}

reduce E {
    Grow => {
        state.w = 200;
        state.h = 60;
    },
    Shrink => state.w = 0 - 5,
}

Page P {
    Stack {
        Stack { Text("sized") }
        .width($state.w)
        .height($state.h)
        on Tap -> dispatch(Grow)
        Stack { Text("same") }
        .width($state.label)
        on Tap -> dispatch(Shrink)
        Stack { Text("same") }
    }
    .align(start)
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

#[test]
fn a_size_bound_to_the_store_follows_it() {
    let nxir = compile(PAGE);
    let (symbols, keys): (Vec<String>, Vec<u32>) = (Vec::new(), Vec::new());
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let tokens = nexus_theme_tokens::BaseTokens;
    let env = FixtureEnv::default();
    let mut view = View::mount(&nxir, &tokens, &env, &locale).expect("mounts");
    let boxes = layout(&view);
    let sized = boxes.iter().find(|b| b.node_id == 2).expect("the sized box");
    assert_eq!((sized.rect.width, sized.rect.height), (FxPx::new(123), FxPx::new(45)));
    let plain = boxes.iter().find(|b| b.node_id == 4).expect("the Str-sized box");
    let control = boxes.iter().find(|b| b.node_id == 6).expect("the box without a width");
    assert_eq!(plain.rect.width, control.rect.width, "a Str sizes nothing: content width");
    let damage = view
        .pointer(&tokens, &env, &locale, &mut NoIo, &boxes, "Tap", FxPx::new(10), FxPx::new(10))
        .expect("dispatches");
    assert_eq!(damage, Some(Damage::Layout), "a size change is layout damage");
    let boxes = layout(&view);
    let sized = boxes.iter().find(|b| b.node_id == 2).expect("the sized box");
    assert_eq!((sized.rect.width, sized.rect.height), (FxPx::new(200), FxPx::new(60)));
}

#[test]
fn test_reject_a_negative_size_clamps_to_zero() {
    let nxir = compile(PAGE);
    let (symbols, keys): (Vec<String>, Vec<u32>) = (Vec::new(), Vec::new());
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let tokens = nexus_theme_tokens::BaseTokens;
    let env = FixtureEnv::default();
    let mut view = View::mount(&nxir, &tokens, &env, &locale).expect("mounts");
    let boxes = layout(&view);
    let plain = boxes.iter().find(|b| b.node_id == 4).expect("the Str-sized box");
    let (x, y) = (plain.rect.x.0 + 4, plain.rect.y.0 + 4);
    let _ = view
        .pointer(&tokens, &env, &locale, &mut NoIo, &boxes, "Tap", FxPx::new(x), FxPx::new(y))
        .expect("dispatches");
    let boxes = layout(&view);
    let sized = boxes.iter().find(|b| b.node_id == 2).expect("the sized box");
    assert_eq!(sized.rect.width, FxPx::new(0), "−5 px clamps to 0, never wraps");
}
