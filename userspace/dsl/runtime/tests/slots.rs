// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! RFC-0084 slots at RUNTIME — the two laws, proven where they can actually
//! fail.
//!
//! **Caller scope**: a slot body reads the caller's `$props` and the caller's
//! loop bindings, not the callee's. The locals case is the one that fails if
//! the frame skips its snapshot, because `locals` are shared across the
//! component boundary.
//!
//! **Splice, not wrap**: a bound slot's N nodes become DIRECT children of the
//! receiving region. If the runtime wrapped them, the wrapper would reset the
//! flex context and `.grow(1)` on the region would stop stretching — the exact
//! regression the branch/ForEach splice paths already guard against.

// reason: test harness — a failed compile/mount step must panic loudly.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, View};
use nexus_layout_types::{FxPx, LayoutNode};

fn compile(src: &str) -> Vec<u8> {
    let file = nexus_dsl_core::parse_file(src).expect("parses");
    let (model, diags) = nexus_dsl_core::check_file(&file);
    assert!(!nexus_dsl_core::has_errors(&diags), "check: {diags:?}\n{src}");
    let canonical = nexus_dsl_core::format_file(&file);
    nexus_dsl_core::lower_file(&file, &model, &canonical).expect("lowers").nxir
}

/// Compiles, mounts and returns the scene's text contents in pre-order.
fn scene_texts(src: &str) -> Vec<String> {
    let nxir = compile(src);
    let tokens = nexus_theme_tokens::BaseTokens;
    let device = FixtureEnv::default();
    let symbols: Vec<String> = Vec::new();
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    texts(view.scene())
}

fn texts(node: &LayoutNode) -> Vec<String> {
    fn walk(node: &LayoutNode, out: &mut Vec<String>) {
        match node {
            LayoutNode::Text(text, _) => out.push(String::from(text.content.as_str())),
            LayoutNode::Stack(_, _, children) | LayoutNode::Grid(_, _, children) => {
                for child in children {
                    walk(child, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(node, &mut out);
    out
}

/// Direct children of a node, if it is a container.
fn children(node: &LayoutNode) -> &[LayoutNode] {
    match node {
        LayoutNode::Stack(_, _, children) | LayoutNode::Grid(_, _, children) => children,
        _ => &[],
    }
}

// -------------------------------------------------------------- caller scope

#[test]
fn slot_body_reads_the_callers_props_not_the_callees() {
    // Both components have a prop called `tag`. The body is written inside
    // `Wrap`, so it must render Wrap's tag — even though it is emitted deep
    // inside `Box`, whose own `tag` is bound to something else.
    let texts = scene_texts(
        r#"
Component BoxC {
    props: {
        tag: Str,
    }
    slot body
    Stack {
        Text($props.tag)
        Slot body
    }
}

Component Wrap {
    props: {
        tag: Str,
    }
    Stack {
        BoxC { tag: "callee" } {
            body {
                Text($props.tag)
            }
        }
    }
}

Page P {
    Wrap { tag: "caller" }
}
"#,
    );
    assert_eq!(texts, ["callee", "caller"], "the body belongs to Wrap, not BoxC");
}

#[test]
fn slot_body_reads_the_callers_loop_binding() {
    // The callee runs its OWN `for` over a different list before reaching the
    // placeholder. Without a snapshot of the caller's locals, the shared
    // `locals` array would have been clobbered by then and every row would
    // render the callee's last item.
    let texts = scene_texts(
        r#"
Component BoxC {
    slot body
    Stack {
        for inner in ["x", "y"] {
            Text(inner)
        }
        Slot body
    }
}

Page P {
    Stack {
        for row in ["one", "two"] {
            BoxC { } {
                body {
                    Text(row)
                }
            }
        }
    }
}
"#,
    );
    // Each iteration: the callee's own two texts, then the caller's row.
    assert_eq!(texts, ["x", "y", "one", "x", "y", "two"]);
}

// ------------------------------------------------------------ splice, not wrap

#[test]
fn a_bound_slot_splices_its_nodes_as_direct_children() {
    let nxir = compile(
        r#"
Component Region {
    slot content
    Stack {
        Slot content
    }
    .grow(1)
}

Page P {
    Region { } {
        content {
            Text("a")
            Text("b")
        }
    }
}
"#,
    );
    let tokens = nexus_theme_tokens::BaseTokens;
    let device = FixtureEnv::default();
    let symbols: Vec<String> = Vec::new();
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");

    // The region Stack must hold the two texts DIRECTLY — a wrapper would show
    // up as a single Stack child and reset the flex context.
    let region = view.scene();
    let kids = children(region);
    assert_eq!(kids.len(), 2, "two spliced children, not one wrapper: {kids:#?}");
    assert!(
        kids.iter().all(|k| matches!(k, LayoutNode::Text(_, _))),
        "both children are the caller's texts: {kids:#?}"
    );
}

#[test]
fn a_spliced_region_still_grows() {
    // The regression the splice rule exists for: a wrapper between the region
    // and the bodies would eat the `.grow(1)`.
    let nxir = compile(
        r#"
Component Region {
    slot content
    Stack {
        Slot content
    }
    .grow(1)
}

Page P {
    Stack {
        Region { } {
            content {
                Text("a")
                Text("b")
            }
        }
    }
    .grow(1)
}
"#,
    );
    let tokens = nexus_theme_tokens::BaseTokens;
    let device = FixtureEnv::default();
    let symbols: Vec<String> = Vec::new();
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    let engine = nexus_layout::LayoutEngine::new();
    let layout = engine
        .layout_with_viewport(
            view.scene(),
            FxPx::new(1280),
            Some(FxPx::new(800)),
            &nexus_text_baked::measure_text::BakedTextMeasure,
        )
        .expect("lays out");
    let tallest = layout.boxes.iter().map(|b| b.rect.height.as_i32()).max().unwrap_or(0);
    assert!(tallest > 700, "the growing region should fill the 800px viewport, got {tallest}");
}

#[test]
fn an_unbound_slot_contributes_no_nodes() {
    let nxir = compile(
        r#"
Component Region {
    slot content
    Stack {
        Text("before")
        Slot content
        Text("after")
    }
}

Page P {
    Region { } {}
}
"#,
    );
    let tokens = nexus_theme_tokens::BaseTokens;
    let device = FixtureEnv::default();
    let symbols: Vec<String> = Vec::new();
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    let kids = children(view.scene());
    // Exactly two: NOT an empty box standing in for the unbound slot.
    assert_eq!(kids.len(), 2, "unbound slot must add nothing: {kids:#?}");
    assert_eq!(texts(view.scene()), ["before", "after"]);
}

#[test]
fn the_panel_opt_in_shape_selects_one_arm_and_keeps_the_body() {
    // The shape `WinAppWindow` needs: the SAME placeholder in both arms of an
    // `if`, so the region's background is a per-region decision.
    const SRC: &str = r#"
Component Region {
    props: {
        panel: Bool,
    }
    slot content
    if $props.panel {
        Panel {
            Slot content
        }
    } else {
        Stack {
            Slot content
        }
    }
}

Page P {
    Region { panel: PANEL } {
        content {
            Text("a")
            Text("b")
        }
    }
}
"#;
    for panel in ["true", "false"] {
        let texts = scene_texts(&SRC.replace("PANEL", panel));
        assert_eq!(texts, ["a", "b"], "panel={panel}: the body renders in either arm");
    }
}

// ------------------------------------------------------------------ handlers

#[test]
fn a_handler_inside_a_slot_body_hit_tests_and_dispatches() {
    let nxir = compile(
        r#"
Store S {
    hits: Int = 0,
}

Event E {
    Bump,
}

reduce E {
    Bump => state.hits += 1,
}

Component Region {
    slot content
    Stack {
        Slot content
    }
    .grow(1)
}

Page P {
    Region { } {
        content {
            Stack {
                Text("tap me")
            }
            .width(200)
            .height(100)
            on Tap -> dispatch(Bump)
        }
    }
}
"#,
    );
    let tokens = nexus_theme_tokens::BaseTokens;
    let device = FixtureEnv::default();
    let symbols: Vec<String> = Vec::new();
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    let engine = nexus_layout::LayoutEngine::new();
    let layout = engine
        .layout_with_viewport(
            view.scene(),
            FxPx::new(1280),
            Some(FxPx::new(800)),
            &nexus_text_baked::measure_text::BakedTextMeasure,
        )
        .expect("lays out");

    assert!(!view.handlers().is_empty(), "the slot body's handler must be collected");

    let mut host = nexus_dsl_runtime::NoIo;
    let damage = view
        .pointer(
            &tokens,
            &device,
            &locale,
            &mut host,
            &layout.boxes,
            "Tap",
            FxPx::new(10),
            FxPx::new(10),
        )
        .expect("pointer routes");
    assert!(damage.is_some(), "the tap must land on the handler inside the slot body");
}
