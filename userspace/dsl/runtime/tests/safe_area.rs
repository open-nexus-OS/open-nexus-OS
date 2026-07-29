// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The platform SAFE AREA — the shell status-bar rows a surface keeps clear.
//!
//! Mobile status-bar model: the bar is always on top and always usable. A
//! chromeless FULLSCREEN window therefore still spans to y=0 — its glass runs
//! under the translucent bar — while its CONTENT starts below it. Before this
//! existed, an app's own 40px chrome row sat in the bar's 36px dead strip
//! (`windowd input.rs` refuses presses there), leaving four usable pixels.
//!
//! The mechanism is padding on the scene ROOT, and the third test is why: a
//! wrapper NODE would have shifted every pre-order id by one and broken
//! handler box-ids, text collection and animation keying at once.

// reason: test harness — a failed compile/mount step must panic loudly.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, View};
use nexus_layout_types::FxPx;

/// The shell status bar (`windowd surface_presentation::SHELL_TOPBAR_H`).
const BAR: i32 = 36;

const PAGE: &str = r#"Store S {
    n: Int = 0,
}

Event E {
    Tap,
}

reduce E {
    Tap => state.n = state.n + 1,
}

Page P {
    Stack {
        Stack {
            Text("row")
        }
        .height(40)
        on Tap -> dispatch(Tap)
        Text("body")
    }
}"#;

fn compile(src: &str) -> Vec<u8> {
    let file = nexus_dsl_core::parse_file(src).expect("parses");
    let (model, diags) = nexus_dsl_core::check_file(&file);
    assert!(!nexus_dsl_core::has_errors(&diags), "check: {diags:?}\n{src}");
    let canonical = nexus_dsl_core::format_file(&file);
    nexus_dsl_core::lower_file(&file, &model, &canonical).expect("lowers").nxir
}

fn mounted(nxir: &[u8]) -> View<'_> {
    View::mount(
        nxir,
        &nexus_theme_tokens::BaseTokens,
        &FixtureEnv::default(),
        &IdentityLocale { symbols: &[], keys: &[] },
    )
    .expect("mounts")
}

fn layout(view: &View<'_>) -> nexus_layout::LayoutResult {
    nexus_layout::LayoutEngine::new()
        .layout_with_viewport(
            view.scene(),
            FxPx::new(320),
            Some(FxPx::new(240)),
            &nexus_text_baked::measure_text::BakedTextMeasure,
        )
        .expect("lays out")
}

/// The two halves of the contract at once: content moves down, background
/// does not. If the root box shrank instead, the app's glass would stop below
/// the bar and the bar would sit on the wallpaper — not the design.
#[test]
fn the_inset_moves_content_down_but_not_the_page_background() {
    let nxir = compile(PAGE);
    let mut view = mounted(&nxir);

    let before = layout(&view);
    let root_before = before.boxes.first().expect("a root box").rect;
    assert_eq!(root_before.y.0, 0);

    assert!(view.set_safe_area_top(FxPx::new(BAR)), "a Stack root takes the inset");
    let after = layout(&view);

    let root = after.boxes.first().expect("a root box").rect;
    assert_eq!(root.y.0, 0, "the page background still reaches the top edge");
    assert_eq!(root.height.0, root_before.height.0, "and still spans the surface");

    for b in after.boxes.iter().skip(1).filter(|b| b.rect.height.0 > 0) {
        assert!(
            b.rect.y.0 >= BAR,
            "every child clears the status bar: node {} at y={}",
            b.node_id,
            b.rect.y.0
        );
    }
}

/// THE reason this is padding and not a wrapper node. Node ids are assigned in
/// pre-order; an extra container would shift all of them by one, and
/// `path_to_box_id`, `collect_texts` and the animation table each walk the
/// scene with their own counter. Padding must leave every id untouched.
#[test]
fn the_inset_changes_no_node_ids() {
    let nxir = compile(PAGE);
    let mut view = mounted(&nxir);
    let before: Vec<usize> = layout(&view).boxes.iter().map(|b| b.node_id).collect();
    let handlers_before: Vec<usize> = view.handlers().iter().map(|(id, _)| *id).collect();

    view.set_safe_area_top(FxPx::new(BAR));

    let after: Vec<usize> = layout(&view).boxes.iter().map(|b| b.node_id).collect();
    let handlers_after: Vec<usize> = view.handlers().iter().map(|(id, _)| *id).collect();
    assert_eq!(before, after, "box ids are identical with and without the inset");
    assert_eq!(handlers_before, handlers_after, "and so are handler box ids");
    assert!(!handlers_before.is_empty(), "the page HAS a handler (guards the above)");
}

/// Setting it twice must not stack. The host pushes a rect on every geometry
/// change, so the same inset arrives again and again.
#[test]
fn applying_the_same_inset_twice_does_not_double_it() {
    let nxir = compile(PAGE);
    let mut view = mounted(&nxir);
    view.set_safe_area_top(FxPx::new(BAR));
    let once = layout(&view).boxes[1].rect.y.0;
    view.set_safe_area_top(FxPx::new(BAR));
    assert_eq!(layout(&view).boxes[1].rect.y.0, once);
    // …and going back to zero restores the original geometry.
    view.set_safe_area_top(FxPx::ZERO);
    assert_eq!(layout(&view).boxes[1].rect.y.0, 0);
}
