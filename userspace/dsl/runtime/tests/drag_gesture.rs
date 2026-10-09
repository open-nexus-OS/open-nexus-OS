// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The drag gesture (RFC-0095, TASK-0068). `DragStart` is hit-tested at the press; `DragMove`
//! and `DragEnd` then reach THAT node (`View::fire_on_box`) wherever the pointer goes — out of
//! its box included — and the reducer reads `device.dragX`/`dragY`/`dragStartX`/`dragStartY`
//! at dispatch time. A press beside the drag node starts nothing; a box without the handler,
//! or a trigger the page never names, dispatches nothing.

// reason: test harness — a failed compile/mount step must panic loudly.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, NoIo, View};
use nexus_layout::LayoutBox;
use nexus_layout_types::{FxPx, LayoutNode};

const PAGE: &str = r#"Store S {
    sx: Int = 0,
    sy: Int = 0,
    x: Int = 0,
    y: Int = 0,
    phase: Int = 0,
}

Event E {
    Began,
    Moved,
    Ended,
}

reduce E {
    Began => {
        state.sx = device.dragStartX;
        state.sy = device.dragStartY;
        state.phase = 1;
    },
    Moved => {
        state.x = device.dragX;
        state.y = device.dragY;
        state.phase = 2;
    },
    Ended => state.phase = 3,
}

Page P {
    Stack {
        Stack {
            Text("surface")
            if $state.phase == 1 && $state.sx == 40 && $state.sy == 30 {
                Text("began at the press")
            }
            if $state.phase == 2 && $state.x == 300 && $state.y == 220 {
                Text("moved outside")
            }
            if $state.phase == 3 {
                Text("ended")
            }
        }
        .height(100)
        on DragStart -> dispatch(Began)
        on DragMove -> dispatch(Moved)
        on DragEnd -> dispatch(Ended)
        Stack {
            Text("beside")
        }
        .height(40)
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

fn texts(view: &View<'_>) -> Vec<String> {
    fn walk(n: &LayoutNode, out: &mut Vec<String>) {
        match n {
            LayoutNode::Stack(_, _, c) | LayoutNode::Grid(_, _, c) => {
                c.iter().for_each(|ch| walk(ch, out))
            }
            LayoutNode::Text(t, _) => out.push(String::from(t.content.as_str())),
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(view.scene(), &mut out);
    out
}

fn env(drag: (i32, i32, i32, i32)) -> FixtureEnv {
    FixtureEnv { drag, ..FixtureEnv::default() }
}

#[test]
fn a_drag_reports_to_the_node_that_took_its_start_wherever_it_goes() {
    let nxir = compile(PAGE);
    let (symbols, keys): (Vec<String>, Vec<u32>) = (Vec::new(), Vec::new());
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let tokens = nexus_theme_tokens::BaseTokens;
    let mut view = View::mount(&nxir, &tokens, &env((0, 0, 0, 0)), &locale).expect("mounts");
    let boxes = layout(&view);
    let surface = view
        .hover_box_id_scrolled(&boxes, "DragStart", FxPx::new(40), FxPx::new(30), None)
        .expect("the press lands on the drag node");
    // The start: the press and the pointer as the host fills them.
    let start = env((44, 33, 40, 30));
    let d = view.fire_on_box(&tokens, &start, &locale, &mut NoIo, surface, "DragStart");
    assert!(d.expect("dispatches").is_some());
    assert!(texts(&view).contains(&String::from("began at the press")), "{:?}", texts(&view));
    // A move far outside the node's 100-pixel box still reaches it.
    let far = env((300, 220, 40, 30));
    let d = view.fire_on_box(&tokens, &far, &locale, &mut NoIo, surface, "DragMove");
    assert!(d.expect("dispatches").is_some());
    assert!(texts(&view).contains(&String::from("moved outside")), "{:?}", texts(&view));
    let d = view.fire_on_box(&tokens, &far, &locale, &mut NoIo, surface, "DragEnd");
    assert!(d.expect("dispatches").is_some());
    assert!(texts(&view).contains(&String::from("ended")));
}

#[test]
fn test_reject_a_press_beside_the_node_and_boxes_or_triggers_without_a_handler() {
    let nxir = compile(PAGE);
    let (symbols, keys): (Vec<String>, Vec<u32>) = (Vec::new(), Vec::new());
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let tokens = nexus_theme_tokens::BaseTokens;
    let mut view = View::mount(&nxir, &tokens, &env((0, 0, 0, 0)), &locale).expect("mounts");
    let boxes = layout(&view);
    let beside =
        view.hover_box_id_scrolled(&boxes, "DragStart", FxPx::new(40), FxPx::new(120), None);
    assert_eq!(beside, None, "a press beside the drag node starts no drag");
    let surface = view
        .hover_box_id_scrolled(&boxes, "DragStart", FxPx::new(40), FxPx::new(30), None)
        .expect("drag node");
    let e = env((1, 1, 1, 1));
    let other = boxes.iter().map(|b| b.node_id).find(|id| *id != surface).expect("another box");
    let d = view.fire_on_box(&tokens, &e, &locale, &mut NoIo, other, "DragMove");
    assert_eq!(d.expect("no error"), None, "a box without the handler dispatches nothing");
    let d = view.fire_on_box(&tokens, &e, &locale, &mut NoIo, surface, "Swipe");
    assert_eq!(d.expect("no error"), None, "a trigger the page never names dispatches nothing");
    assert!(!texts(&view).iter().any(|t| t == "ended" || t == "moved outside"));
}
