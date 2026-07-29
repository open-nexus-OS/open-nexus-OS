// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//! Kit exposure: every design-system widget the DSL can name mounts, lays out
//! and paints what its props say.
//!
//! Split out of `layout_viewport.rs` — that file is about the app-host's
//! bounded-viewport contract, this one is about the registry seam
//! (`dsl/core/src/registry.rs` names a widget, `dsl/runtime/src/registry/
//! widgets.rs` builds it). A widget with a registry entry and NO runtime arm
//! compiles, mounts and silently renders a bare Stack, so "it paints its own
//! content" is the assertion that has to exist.

use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, View};

fn compile(src: &str) -> Vec<u8> {
    let file = nexus_dsl_core::parse_file(src).expect("parses");
    let (model, diags) = nexus_dsl_core::check_file(&file);
    assert!(!nexus_dsl_core::has_errors(&diags), "check: {diags:?}");
    nexus_dsl_core::lower_file(&file, &model, src).expect("lowers").nxir
}

fn mount(src: &str) -> View<'static> {
    let nxir: &'static [u8] = Box::leak(compile(src).into_boxed_slice());
    let device = FixtureEnv::default();
    let symbols: Vec<String> = Vec::new();
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    View::mount(nxir, &nexus_theme_tokens::BaseTokens, &device, &locale).expect("mounts")
}

/// Every Text content in the scene, in pre-order.
fn scene_texts(view: &View) -> Vec<String> {
    fn walk(node: &nexus_layout_types::LayoutNode, out: &mut Vec<String>) {
        match node {
            nexus_layout_types::LayoutNode::Stack(_, _, children)
            | nexus_layout_types::LayoutNode::Grid(_, _, children) => {
                for c in children {
                    walk(c, out);
                }
            }
            nexus_layout_types::LayoutNode::Text(t, _) => {
                out.push(String::from(t.content.as_str()))
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(view.scene(), &mut out);
    out
}

fn layout_boxes(view: &View) -> Vec<nexus_layout::LayoutBox> {
    nexus_layout::LayoutEngine::new()
        .layout_with_viewport(
            view.scene(),
            nexus_layout_types::FxPx::new(1280),
            Some(nexus_layout_types::FxPx::new(800)),
            &nexus_text_baked::measure_text::BakedTextMeasure,
        )
        .expect("lays out")
        .boxes
}

/// Kit exposure (TASK-0073/0074): every design-system widget the DSL exposes
/// compiles, mounts and lays out — the DSL `Button`/`Badge`/… IS the kit
/// builder (one SSOT), so this is the "our button is really our button" pin.
#[test]
fn design_kit_widgets_mount_through_the_dsl() {
    let view = mount(
        r#"Page Main {
    Stack {
        Badge { label: "Neu" }
        Chip { label: "Filter" }
        Avatar { initials: "JS" }
        Checkbox { checked: true, label: "AGB akzeptieren" }
        Slider { value: 40 }
            .label("Lautstärke")
        Spinner
        ProgressBar { value: 64 }
        Toast { message: "Gespeichert" }
        Banner { title: "Status", message: "Synchronisiert" }
        Skeleton
        ListItem { title: "WLAN", subtitle: "Verbunden", showChevron: true }
        Toolbar { title: "Einstellungen" }
        SearchBar { value: "", placeholder: "Suchen" }
        Select { value: "Deutsch", placeholder: "Wählen" }
        Breadcrumbs { items: ["Einstellungen", "Verbindungen"] }
    }
}
"#,
    );
    // Every widget produced real geometry (no zero-sized kit stubs).
    let sized = layout_boxes(&view)
        .iter()
        .filter(|b| b.rect.width.as_i32() > 0 && b.rect.height.as_i32() > 0)
        .count();
    assert!(sized >= 10, "expected at least 10 sized boxes, got {sized}");
}

/// `Select` and `Breadcrumbs` were kit crates the DSL could not name, so a
/// page needing them hand-rolled a lookalike. Now that they are registered,
/// pin what they actually PAINT: the select shows its value (not its
/// placeholder), and the breadcrumb renders every crumb with the `›`
/// separators between them. A missing runtime arm would fall through to a
/// bare Stack and both assertions would read back empty.
#[test]
fn select_and_breadcrumbs_render_their_content() {
    let view = mount(
        r#"Page Main {
    Stack {
        Select { value: "Deutsch — QWERTZ", placeholder: "Layout wählen" }
        Breadcrumbs { items: ["Einstellungen", "Verbindungen", "Datennutzung"] }
    }
}
"#,
    );
    let found = scene_texts(&view);

    assert!(found.iter().any(|t| t == "Deutsch — QWERTZ"), "select value missing: {found:?}");
    assert!(
        !found.iter().any(|t| t == "Layout wählen"),
        "placeholder must yield to the value: {found:?}"
    );
    for crumb in ["Einstellungen", "Verbindungen", "Datennutzung"] {
        assert!(found.iter().any(|t| t == crumb), "crumb {crumb} missing: {found:?}");
    }
    assert_eq!(
        found.iter().filter(|t| t.as_str() == "›").count(),
        2,
        "three crumbs need two separators: {found:?}"
    );
}

/// A `Breadcrumbs` whose `items` is not a list must still render something
/// rather than vanish — the DSL cannot type-check widget props, so a scalar
/// slipping in is an authoring mistake the runtime has to survive visibly.
#[test]
fn breadcrumbs_degrades_a_scalar_items_prop_to_one_crumb() {
    let view = mount("Page Main { Breadcrumbs { items: \"Einstellungen\" } }");
    assert_eq!(scene_texts(&view), vec![String::from("Einstellungen")]);
}
