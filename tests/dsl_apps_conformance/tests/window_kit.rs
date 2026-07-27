// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
// `WinAppWindow` (RFC-0084): the window scaffold whose regions are transparent
// by default. The contract worth testing is the CONFIGURABILITY — a region
// takes zero, one or several panels, the developer decides per region, and
// flipping a panel on does not move anything.

// reason: test harness — a failed compile/mount step must panic loudly.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, View};
use nexus_layout_types::{FxPx, LayoutNode, SurfaceMaterial};

/// The kit component under test, inlined so the fixture stays one file (the
/// real app pulls it through `dependencies = ["window-kit"]`).
const KIT: &str = include_str!("../../../userspace/apps/window-kit/ui/components/WinAppWindow.nx");

fn program(page: &str) -> String {
    format!("{KIT}\n{page}\n")
}

fn compile(src: &str) -> Vec<u8> {
    let file = nexus_dsl_core::parse_file(src).expect("parses");
    let (model, diags) = nexus_dsl_core::check_file(&file);
    assert!(!nexus_dsl_core::has_errors(&diags), "check: {diags:?}");
    let canonical = nexus_dsl_core::format_file(&file);
    nexus_dsl_core::lower_file(&file, &model, &canonical).expect("lowers").nxir
}

fn children(node: &LayoutNode) -> &[LayoutNode] {
    match node {
        LayoutNode::Stack(_, _, children) | LayoutNode::Grid(_, _, children) => children,
        _ => &[],
    }
}

/// How many nodes in the scene paint a glass surface — i.e. how many `Panel`s
/// actually made it through.
fn glass_count(node: &LayoutNode) -> usize {
    fn walk(node: &LayoutNode, out: &mut usize) {
        if let LayoutNode::Stack(_, visual, _) = node {
            if matches!(visual.material, SurfaceMaterial::Glass(_)) {
                *out += 1;
            }
        }
        for child in children(node) {
            walk(child, out);
        }
    }
    let mut out = 0;
    walk(node, &mut out);
    out
}

fn texts(node: &LayoutNode) -> Vec<String> {
    fn walk(node: &LayoutNode, out: &mut Vec<String>) {
        if let LayoutNode::Text(text, _) = node {
            out.push(String::from(text.content.as_str()));
        }
        for child in children(node) {
            walk(child, out);
        }
    }
    let mut out = Vec::new();
    walk(node, &mut out);
    out
}

struct Mounted {
    nxir: Vec<u8>,
}

impl Mounted {
    fn new(page: &str) -> Self {
        Self { nxir: compile(&program(page)) }
    }

    fn with<R>(&self, f: impl FnOnce(&View<'_>) -> R) -> R {
        let tokens = nexus_theme_tokens::BaseTokens;
        let device = FixtureEnv::default();
        let symbols: Vec<String> = Vec::new();
        let keys: Vec<u32> = Vec::new();
        let locale = IdentityLocale { symbols: &symbols, keys: &keys };
        let view = View::mount(&self.nxir, &tokens, &device, &locale).expect("mounts");
        f(&view)
    }
}

/// A page using the scaffold with the given panel flags and content body.
fn page(flags: &str, content: &str) -> String {
    format!(
        r#"Page P {{
    WinAppWindow {{ {flags} }} {{
        sidebar {{
            Text("nav")
        }}
        content {{
{content}
        }}
        properties {{
            Text("props")
        }}
    }}
}}"#
    )
}

const ALL_OFF: &str =
    "sidebarPanel: false, contentPanel: false, propsPanel: false, showSidebar: true, showProps: true";

#[test]
fn the_scaffold_compiles_and_mounts() {
    Mounted::new(&page(ALL_OFF, r#"            Text("body")"#)).with(|view| {
        assert_eq!(texts(view.scene()), ["nav", "body", "props"]);
    });
}

#[test]
fn every_region_is_transparent_by_default() {
    // The whole point: nothing paints a background unless the app asked.
    Mounted::new(&page(ALL_OFF, r#"            Text("body")"#)).with(|view| {
        assert_eq!(glass_count(view.scene()), 0, "no region paints glass on its own");
    });
}

#[test]
fn panels_are_opt_in_per_region() {
    let cases = [
        ("sidebarPanel: true,  contentPanel: false, propsPanel: false", 1),
        ("sidebarPanel: false, contentPanel: true,  propsPanel: false", 1),
        ("sidebarPanel: false, contentPanel: false, propsPanel: true", 1),
        ("sidebarPanel: true,  contentPanel: true,  propsPanel: true", 3),
    ];
    for (panels, expected) in cases {
        let flags = format!("{panels}, showSidebar: true, showProps: true");
        Mounted::new(&page(&flags, r#"            Text("body")"#)).with(|view| {
            assert_eq!(
                glass_count(view.scene()),
                expected,
                "flags `{panels}` should paint {expected} panel(s)"
            );
        });
    }
}

#[test]
fn a_region_takes_several_stacked_panels() {
    // The settings case: the app puts its OWN panels in, the scaffold adds
    // none. `contentPanel: false` plus three panels in the body = three.
    let flags = "sidebarPanel: false, contentPanel: false, propsPanel: false, showSidebar: true, showProps: true";
    let body = r#"            Panel {
                Text("one")
            }
            Panel {
                Text("two")
            }
            Panel {
                Text("three")
            }"#;
    Mounted::new(&page(flags, body)).with(|view| {
        assert_eq!(glass_count(view.scene()), 3);
        assert_eq!(texts(view.scene()), ["nav", "one", "two", "three", "props"]);
    });
}

#[test]
fn a_hidden_region_contributes_nothing() {
    let flags = "sidebarPanel: false, contentPanel: false, propsPanel: false, showSidebar: false, showProps: false";
    Mounted::new(&page(flags, r#"            Text("body")"#)).with(|view| {
        // The sidebar and properties BODIES are still bound by the caller;
        // the scaffold simply never places their placeholders.
        assert_eq!(texts(view.scene()), ["body"]);
    });
}

#[test]
fn flipping_a_panel_on_keeps_the_region_rects() {
    // Both arms of every `if` share their OUTER geometry: the three zones sit
    // at the same place and keep the same width whether or not they paint a
    // background, so an app can change its mind about a panel without
    // re-tuning the window's proportions.
    //
    // What legitimately moves is the content INSIDE a panelled region — a
    // `Panel` brings its own padding, which is the whole point of it.
    let geometry = |flags: &str| -> Vec<(i32, i32, i32, i32)> {
        let mounted = Mounted::new(&page(flags, r#"            Text("body")"#));
        mounted.with(|view| {
            let engine = nexus_layout::LayoutEngine::new();
            let layout = engine
                .layout_with_viewport(
                    view.scene(),
                    FxPx::new(1280),
                    Some(FxPx::new(800)),
                    &nexus_text_baked::measure_text::BakedTextMeasure,
                )
                .expect("lays out");
            layout
                .boxes
                .iter()
                .map(|b| {
                    (
                        b.rect.x.as_i32(),
                        b.rect.y.as_i32(),
                        b.rect.width.as_i32(),
                        b.rect.height.as_i32(),
                    )
                })
                .collect()
        })
    };
    // The three regions are the full-height boxes below the root.
    let regions = |flags: &str| -> Vec<(i32, i32, i32, i32)> {
        let mut rects: Vec<_> = geometry(flags)
            .into_iter()
            .skip(1)
            .filter(|(_, y, _, h)| *y == 0 && *h == 800)
            .collect();
        rects.sort_unstable();
        rects
    };
    let off = regions(ALL_OFF);
    let on = regions(
        "sidebarPanel: true, contentPanel: true, propsPanel: true, showSidebar: true, showProps: true",
    );
    assert_eq!(
        off,
        [(0, 0, 240, 800), (240, 0, 780, 800), (1020, 0, 260, 800)],
        "sidebar 240 · content fills the rest · properties 260"
    );
    assert_eq!(off, on, "panel opt-in must not move or resize a region");
}
