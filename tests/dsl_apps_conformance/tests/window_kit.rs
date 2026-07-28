// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
// `WinAppWindow` (RFC-0084): the window scaffold whose regions are transparent,
// FULL STOP — there is no panel flag any more. A region that wants a surface
// gets one because the app wrote `Panel { … }` in the slot body.
//
// So the contract worth testing moved: it is no longer "the flag paints one
// panel" but "the scaffold paints NONE, and whatever the body writes survives
// the splice with the region's geometry intact".

// reason: test harness — a failed compile/mount step must panic loudly.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, View};
use nexus_layout_types::{FxPx, LayoutNode, SurfaceMaterial};

/// The kit component under test, inlined so the fixture stays one file (the
/// real app pulls it through `dependencies = ["window-kit"]`).
const KIT: &str = include_str!("../../../userspace/apps/window-kit/ui/components/WinAppWindow.nx");

fn program(page: &str) -> String {
    // Minimal store: the kit's overlay scrim/pane dispatch `WinPaneClose`/
    // `WinNoop`, which must be declared event cases in the mounting app.
    const STORE: &str = r#"Store S {
    x: Int = 0,
}

Event E {
    WinNoop,
    WinPaneClose,
}

reduce E {
    WinNoop => state.x = state.x,
    WinPaneClose => state.x = state.x,
}"#;
    format!("{KIT}\n{STORE}\n{page}\n")
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
        self.with_env(FixtureEnv::default(), f)
    }

    fn with_env<R>(&self, device: FixtureEnv, f: impl FnOnce(&View<'_>) -> R) -> R {
        let tokens = nexus_theme_tokens::BaseTokens;
        let symbols: Vec<String> = Vec::new();
        let keys: Vec<u32> = Vec::new();
        let locale = IdentityLocale { symbols: &symbols, keys: &keys };
        let view = View::mount(&self.nxir, &tokens, &device, &locale).expect("mounts");
        f(&view)
    }
}

/// A desktop env pinned to one size class (RFC-0084 P7 tier tests).
fn env(size_class: &'static str) -> FixtureEnv {
    let mut env = FixtureEnv::default();
    env.size_class = size_class;
    env
}

/// A page using the scaffold with the given props and content body.
fn page(flags: &str, content: &str) -> String {
    format!(
        r#"Page P {{
    WinAppWindow {{ {flags} }} {{
        sidebarLeft {{
            Text("nav")
        }}
        contentArea {{
{content}
        }}
        sidebarRight {{
            Text("props")
        }}
    }}
}}"#
    )
}

const SHOWN: &str = "showSidebar: true, showProps: true";

#[test]
fn the_scaffold_compiles_and_mounts() {
    Mounted::new(&page(SHOWN, r#"            Text("body")"#)).with(|view| {
        assert_eq!(texts(view.scene()), ["nav", "body", "props"]);
    });
}

#[test]
fn the_scaffold_paints_no_surface_of_its_own() {
    // The whole point, and the reason the three `*Panel` flags are gone: the
    // kit contributes geometry, never paint. Bodies of bare `Text` ⇒ zero glass
    // anywhere in the scene, at every tier.
    let mounted = Mounted::new(&page(SHOWN, r#"            Text("body")"#));
    for tier in ["wide", "regular", "compact"] {
        mounted.with_env(env(tier), |view| {
            assert_eq!(glass_count(view.scene()), 0, "{tier}: the kit painted a surface");
        });
    }
}

#[test]
fn a_body_may_write_any_number_of_panels() {
    // Zero, one, three — the app decides by writing markup, not by flipping a
    // flag. Three in one region is the settings case, which under the old
    // flag design needed `contentPanel: false` plus hand-rolled panels.
    let one = r#"            Panel {
                Text("body")
            }"#;
    Mounted::new(&page(SHOWN, one)).with(|view| {
        assert_eq!(glass_count(view.scene()), 1);
    });

    let three = r#"            Panel {
                Text("one")
            }
            Panel {
                Text("two")
            }
            Panel {
                Text("three")
            }"#;
    Mounted::new(&page(SHOWN, three)).with(|view| {
        assert_eq!(glass_count(view.scene()), 3);
        assert_eq!(texts(view.scene()), ["nav", "one", "two", "three", "props"]);
    });
}

/// A panel in a body can pick its own glass LEVEL. This is what the flag design
/// could not express without one prop per level, and it is what the file-manager
/// handoff needs: content and properties are `windowPane` (a pane on window
/// glass), not `panel` (a tile on the wallpaper).
#[test]
fn a_body_panel_chooses_its_own_glass_level() {
    use nexus_layout_types::GlassLevel;
    let body = r#"            Panel {
                Text("body")
            }
            .material(windowPane)"#;
    Mounted::new(&page(SHOWN, body)).with(|view| {
        let mut levels = Vec::new();
        fn walk(node: &LayoutNode, out: &mut Vec<GlassLevel>) {
            if let LayoutNode::Stack(_, visual, _) = node {
                if let SurfaceMaterial::Glass(level) = visual.material {
                    out.push(level);
                }
            }
            for child in children(node) {
                walk(child, out);
            }
        }
        walk(view.scene(), &mut levels);
        assert_eq!(levels, [GlassLevel::WindowPane]);
    });
}

#[test]
fn a_hidden_region_contributes_nothing() {
    let flags = "showSidebar: false, showProps: false";
    Mounted::new(&page(flags, r#"            Text("body")"#)).with(|view| {
        // The sidebar and properties BODIES are still bound by the caller;
        // the scaffold simply never places their placeholders.
        assert_eq!(texts(view.scene()), ["body"]);
    });
}

#[test]
fn writing_a_panel_into_a_body_keeps_the_region_rects() {
    // The regions are fixed geometry: sidebar 240 · content fills · properties
    // 260, whatever the bodies contain. An app can add or drop a surface
    // without re-tuning the window's proportions.
    //
    // What legitimately moves is the content INSIDE a panelled region — a
    // `Panel` brings its own padding, which is the whole point of it.
    let geometry = |body: &str| -> Vec<(i32, i32, i32, i32)> {
        let mounted = Mounted::new(&page(SHOWN, body));
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
    let regions = |body: &str| -> Vec<(i32, i32, i32, i32)> {
        let mut rects: Vec<_> = geometry(body)
            .into_iter()
            .skip(1)
            // Full-height boxes below the root, minus the row WRAPPER the
            // responsive split added (P7): the wrapper spans the full width.
            .filter(|(_, y, w, h)| *y == 0 && *h == 800 && *w < 1280)
            .collect();
        rects.sort_unstable();
        rects
    };
    let bare = regions(r#"            Text("body")"#);
    let panelled = regions(
        r#"            Panel {
                Text("body")
            }
            .grow(1)"#,
    );
    assert_eq!(
        bare,
        [(0, 0, 240, 800), (240, 0, 780, 800), (1020, 0, 260, 800)],
        "sidebar 240 · content fills the rest · properties 260"
    );
    assert_eq!(bare, panelled, "a body's panel must not move or resize a region");
}

#[test]
fn responsive_tiers_move_panes_out_of_flow() {
    // RFC-0084 P7 (tiers 640/1024): wide keeps all three zones inline,
    // regular drops the inline properties pane, compact drops the inline
    // sidebar too — content alone owns the width.
    let mounted = Mounted::new(&page(SHOWN, r#"            Text("body")"#));
    mounted.with_env(env("wide"), |view| {
        assert_eq!(texts(view.scene()), ["nav", "body", "props"]);
    });
    mounted.with_env(env("regular"), |view| {
        assert_eq!(texts(view.scene()), ["nav", "body"], "regular: properties leave the flow");
    });
    mounted.with_env(env("compact"), |view| {
        assert_eq!(texts(view.scene()), ["body"], "compact: content alone");
    });
}

#[test]
fn overlay_flags_bring_the_panes_back() {
    // The out-of-flow panes come back as OVERLAY panes (caller-controlled
    // flags). The kit no longer wraps them: the pane IS the app's own body, so
    // a body of bare `Text` floats WITHOUT a surface. That is the documented
    // consequence of the app owning its panels — asserted here rather than
    // hidden, because it is the one thing a caller has to remember.
    let flags = "showSidebar: true, showProps: true, sidebarOverlay: true, propsOverlay: true";
    let mounted = Mounted::new(&page(flags, r#"            Text("body")"#));
    mounted.with_env(env("compact"), |view| {
        assert_eq!(texts(view.scene()), ["body", "nav", "props"], "both panes overlay");
        assert_eq!(glass_count(view.scene()), 0, "bare bodies ⇒ unframed panes");
    });
    mounted.with_env(env("regular"), |view| {
        assert_eq!(texts(view.scene()), ["nav", "body", "props"], "sidebar inline, props overlay");
    });
    mounted.with_env(env("wide"), |view| {
        assert_eq!(texts(view.scene()), ["nav", "body", "props"], "wide ignores overlay flags");
    });
}

/// …and the fix for that consequence, which is what a real app does: put a
/// `Panel` in the body and the overlay pane is framed at every tier.
#[test]
fn a_panelled_body_frames_its_overlay_pane() {
    let flags = "showSidebar: true, showProps: true, sidebarOverlay: true, propsOverlay: true";
    let body = r#"            Panel {
                Text("body")
            }"#;
    // `page` panels only the CONTENT body, so the sidebar/properties bodies
    // stay bare — the count therefore isolates the content panel itself.
    let mounted = Mounted::new(&page(flags, body));
    mounted.with_env(env("compact"), |view| {
        assert_eq!(glass_count(view.scene()), 1, "the content panel, once");
    });
}
