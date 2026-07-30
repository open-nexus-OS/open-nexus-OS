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

/// `Grid { columns: n }` is the engine's REAL fixed-column grid
/// (`LayoutNode::Grid`) — the first `.nx`-reachable grid. Until now every
/// "grid" in the shell was `.direction(row).wrap(true)`, and `flex_wrap` is
/// a field the engine never reads, so twelve tiles laid out as one
/// overflowing row. Pin the two behaviors that distinguish a grid from that
/// bug: children occupy exactly `columns` x-positions, and overflow moves
/// DOWN into new rows (distinct y rungs), row-major.
#[test]
fn grid_widget_places_children_in_fixed_columns_and_rows() {
    let view = mount(
        r#"Page Main {
    Grid { columns: 3, rowGap: 2
        Stack { }.width(40).height(40)
        Stack { }.width(40).height(40)
        Stack { }.width(40).height(40)
        Stack { }.width(40).height(40)
        Stack { }.width(40).height(40)
        Stack { }.width(40).height(40)
        Stack { }.width(40).height(40)
    }
    .gap(2)
}
"#,
    );
    let cells: Vec<_> = layout_boxes(&view)
        .iter()
        .filter(|b| b.rect.width.as_i32() == 40 && b.rect.height.as_i32() == 40)
        .map(|b| (b.rect.x.as_i32(), b.rect.y.as_i32()))
        .collect();
    assert_eq!(cells.len(), 7, "all seven cells laid out: {cells:?}");

    let mut xs: Vec<i32> = cells.iter().map(|(x, _)| *x).collect();
    xs.sort_unstable();
    xs.dedup();
    assert_eq!(xs.len(), 3, "three fixed column tracks: {cells:?}");

    let mut ys: Vec<i32> = cells.iter().map(|(_, y)| *y).collect();
    ys.sort_unstable();
    ys.dedup();
    assert_eq!(ys.len(), 3, "seven cells over three rows (3+3+1): {cells:?}");

    // Row-major: the 7th cell starts a new row in the FIRST column.
    let last = cells[6];
    assert_eq!(last.0, xs[0], "8th slot wraps to column 0: {cells:?}");
    assert_eq!(last.1, ys[2], "…on the third row: {cells:?}");
}

/// `skip`/`take`/`len` — the pager's page-slice builtins. One store list,
/// page cells sliced in expression position: `take(skip(xs, 2), 3)` renders
/// exactly items 2..5, and `len(xs)` guards the page-dot arms. (Only `tail`
/// existed before; the other combinators returned `Unsupported`, so a pager
/// had no way to render "page k of ONE list".)
#[test]
fn skip_take_len_slice_a_page_out_of_one_list() {
    let view = mount(
        r#"Store S {
    xs: List<Str> = ["a", "b", "c", "d", "e", "f", "g"],
}

Page Main {
    Stack {
        List(take(skip($state.xs, 2), 3)) { x in
            Stack {
                Text(x)
            }
            .key(x)
        }
        if len($state.xs) > 5 {
            Text("dot-1")
        }
        if len($state.xs) > 99 {
            Text("dot-2")
        }
    }
}
"#,
    );
    let texts = scene_texts(&view);
    assert_eq!(
        texts,
        vec![String::from("c"), String::from("d"), String::from("e"), String::from("dot-1")],
        "slice = items 2..5, len-guard shows dot-1 only"
    );
}

/// An UNPASSED component prop reads as its type's empty value — `Bool` =
/// false. The shell's tile family leans on this: `AppTile { … }` without
/// `running:` must render the not-running state (the running flag only
/// arrives once the window feed lands), never error or truthy-default.
#[test]
fn unpassed_bool_prop_defaults_to_false() {
    let view = mount(
        r#"Component T {
    props: {
        label: Str,
        running: Bool,
    }
    Stack {
        Text($props.label)
        if $props.running {
            Text("dot")
        }
    }
}

Page Main {
    T { label: "hi" }
}
"#,
    );
    assert_eq!(scene_texts(&view), vec![String::from("hi")], "no dot without running:");
}

/// `.columns(n)` on a `List` — the DATA-DRIVEN grid: the spliced items
/// become the grid cells (workspace 6-column grid, launcher 4-column grid).
/// Without this, a dynamic collection could never be a real grid — `Grid`
/// takes static children and `.wrap(true)` is the engine no-op.
#[test]
fn list_with_columns_modifier_lays_out_as_a_grid() {
    let view = mount(
        r#"Store S {
    xs: List<Str> = ["a", "b", "c", "d", "e"],
}

Page Main {
    List($state.xs) { x in
        Stack {
            Text(x)
        }
        .key(x)
        .width(40)
        .height(40)
    }
    .columns(2)
    .rowGap(2)
    .gap(2)
}
"#,
    );
    let cells: Vec<_> = layout_boxes(&view)
        .iter()
        .filter(|b| b.rect.width.as_i32() == 40 && b.rect.height.as_i32() == 40)
        .map(|b| (b.rect.x.as_i32(), b.rect.y.as_i32()))
        .collect();
    assert_eq!(cells.len(), 5, "five cells: {cells:?}");
    let mut xs: Vec<i32> = cells.iter().map(|(x, _)| *x).collect();
    xs.sort_unstable();
    xs.dedup();
    assert_eq!(xs.len(), 2, "two column tracks: {cells:?}");
    let mut ys: Vec<i32> = cells.iter().map(|(_, y)| *y).collect();
    ys.sort_unstable();
    ys.dedup();
    assert_eq!(ys.len(), 3, "five cells over three rows (2+2+1): {cells:?}");
}
