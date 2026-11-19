// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//! What the runtime writes for the motion surface of TASK-0354 P10/P13: the absent state of an
//! `.animate` lives in the scene (the node's own opacity 0), `.rotate` is the glyph's rest
//! rotation, `.motion` stamps a `Bound` intent keyed by the node's PLACE (the two branches of an
//! `if` at one place are one node), and `.press` reaches the handler.

use nexus_dsl_runtime::interact::PressRole;
use nexus_dsl_runtime::{AnimKind, FixtureEnv, IdentityLocale, View};

fn mount(src: &str) -> View<'static> {
    let file = nexus_dsl_core::parse_file(src).expect("parses");
    let (model, diags) = nexus_dsl_core::check_file(&file);
    assert!(!nexus_dsl_core::has_errors(&diags), "check: {diags:?}");
    let nxir = nexus_dsl_core::lower_file(&file, &model, src).expect("lowers").nxir;
    let nxir: &'static [u8] = Box::leak(nxir.into_boxed_slice());
    let symbols: Vec<String> = Vec::new();
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    View::mount(nxir, &nexus_theme_tokens::BaseTokens, &FixtureEnv::default(), &locale)
        .expect("mounts")
}

fn boxes(view: &View) -> Vec<nexus_layout::LayoutBox> {
    nexus_layout::LayoutEngine::new()
        .layout_with_viewport(
            view.scene(),
            nexus_layout_types::FxPx::new(400),
            Some(nexus_layout_types::FxPx::new(300)),
            &nexus_text_baked::measure_text::BakedTextMeasure,
        )
        .expect("lays out")
        .boxes
}

fn page(open: bool) -> String {
    format!(
        r#"Store S {{
    open: Bool = {open},
    n: Int = 0,
}}
Event E {{
    Go,
}}
reduce E {{
    Go => {{
        state.n = 1;
    }},
}}
Page Main {{
    Stack {{
        Stack {{ }}
            .width(8)
            .height(8)
            .bg(label)
            .animate(fade, value: $state.open)
        Stack {{ }}
            .width(8)
            .height(8)
            .bg(label)
            .effect(wiggle, trigger: $state.n)
        if $state.open {{
            Icon {{ symbol: "chevron.down", size: 12 }}
                .rotate(180)
                .motion(quick, springSoft)
        }} else {{
            Icon {{ symbol: "chevron.down", size: 12 }}
                .rotate(0)
                .motion(quick, springSoft)
        }}
        Stack {{ }}
            .width(40)
            .height(40)
            .press(tile)
            .label("tile")
        on Tap -> dispatch(Go)
    }}
}}
"#
    )
}

fn anim_node(view: &View, kind: AnimKind) -> usize {
    view.animations().iter().find(|(_, i)| i.kind == kind).map(|(id, _)| *id).expect("stamped")
}

#[test]
fn an_absent_animate_rests_invisible_in_the_scene() {
    let view = mount(&page(false));
    let id = anim_node(&view, AnimKind::Animate);
    let b = boxes(&view).into_iter().find(|b| b.node_id == id).expect("box");
    assert_eq!(b.opacity, 0, "absent: the node's own opacity 0, written by the runtime");

    let view = mount(&page(true));
    let id = anim_node(&view, AnimKind::Animate);
    let b = boxes(&view).into_iter().find(|b| b.node_id == id).expect("box");
    assert_eq!(b.opacity, 255, "present: drawn");
}

#[test]
fn an_effect_is_never_absent() {
    let view = mount(&page(false));
    let id = anim_node(&view, AnimKind::Effect);
    let b = boxes(&view).into_iter().find(|b| b.node_id == id).expect("box");
    assert_eq!(b.opacity, 255, "a zero trigger is no absence");
}

#[test]
fn a_rotation_is_the_glyphs_rest() {
    for (open, deg) in [(true, 180), (false, 0)] {
        let view = mount(&page(open));
        let id = anim_node(&view, AnimKind::Bound);
        let b = boxes(&view).into_iter().find(|b| b.node_id == id).expect("box");
        assert_eq!(b.visual.rotate_deg, deg, "open={open}");
    }
}

/// The two branches of an `if` at one place are ONE node to `.motion`: switching branches is a
/// change of that node (it travels), not a new node appearing.
#[test]
fn a_motion_is_keyed_by_the_place_not_the_branch() {
    let bound = |open| {
        let view = mount(&page(open));
        let i = view.animations().iter().find(|(_, i)| i.kind == AnimKind::Bound).expect("bound").1;
        (i.token, i.value)
    };
    let (closed, open) = (bound(false), bound(true));
    assert_eq!(closed, open, "same pair, same identity across the branch switch");
    let pair = nexus_dsl_runtime::anim::unpack_bound(closed.0).expect("a packed pair");
    assert_eq!(
        pair,
        (
            nexus_theme_tokens::MotionDurationToken::Quick,
            nexus_theme_tokens::MotionCurveToken::SpringSoft
        )
    );
    assert_ne!(closed.1, 0, "an identity is never absent");
}

#[test]
fn a_declared_press_reaches_the_handler() {
    let view = mount(&page(false));
    let presses: Vec<_> = view.handlers().iter().map(|(_, h)| h.press).collect();
    assert_eq!(presses, vec![Some(PressRole::Tile)]);
}

/// `LevelBars` (TASK-0354 P13): the island's four bars as one kit widget whose loop the host
/// runs — four 3 px bars at their rest heights, bottom-aligned, and one `LOOP_LEVEL` intent.
#[test]
fn level_bars_are_four_bars_with_one_level_loop() {
    let view = mount("Page Main {\n    Stack {\n        LevelBars { }\n    }\n}\n");
    let loops: Vec<_> =
        view.animations().iter().filter(|(_, i)| i.kind == AnimKind::Loop).collect();
    assert_eq!(loops.len(), 1);
    assert_eq!(loops[0].1.value, nexus_dsl_runtime::LOOP_LEVEL);
    let all = boxes(&view);
    let row = all.iter().find(|b| b.node_id == loops[0].0).expect("the row");
    let bars: Vec<_> = (1..=4)
        .map(|k| all.iter().find(|b| b.node_id == row.node_id + k).expect("a bar"))
        .collect();
    let heights: Vec<i32> = bars.iter().map(|b| b.rect.height.0).collect();
    assert_eq!(heights, vec![8, 12, 6, 10]);
    let bottoms: Vec<i32> = bars.iter().map(|b| b.rect.y.0 + b.rect.height.0).collect();
    assert!(bottoms.windows(2).all(|w| w[0] == w[1]), "bottom-aligned: {bottoms:?}");
    assert!(bars.iter().all(|b| b.rect.width.0 == 3 && b.opacity == 148));
}
