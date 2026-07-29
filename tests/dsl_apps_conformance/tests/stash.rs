// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
// stash on `WinAppWindow` (RFC-0084). The app had NO conformance test before
// this — it compiled only as a side effect of the boot lane. What is pinned
// here is the design-handoff behavior that is easy to break silently: which
// regions paint a panel, that the properties pane and the action bar both
// react to selection, and that settings mode really replaces the content
// instead of drawing over it.

// reason: test harness — a failed compile/mount step must panic loudly.
#![allow(clippy::expect_used, clippy::unwrap_used)]

mod common;

use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, NoIo, View};
use nexus_layout_types::{LayoutNode, SurfaceMaterial};

fn children(node: &LayoutNode) -> &[LayoutNode] {
    match node {
        LayoutNode::Stack(_, _, children) | LayoutNode::Grid(_, _, children) => children,
        _ => &[],
    }
}

/// Every string a user can READ in the scene: static text plus a text input's
/// value and its placeholder. Collecting only `Text` nodes made the search
/// assertions vacuous the moment the field became a real input widget — the
/// placeholder lives on `TextInput`, which is exactly what the user sees.
fn texts(node: &LayoutNode) -> Vec<String> {
    fn walk(node: &LayoutNode, out: &mut Vec<String>) {
        match node {
            LayoutNode::Text(text, _) => out.push(String::from(text.content.as_str())),
            LayoutNode::TextInput(input, _) => {
                let value = input.content.as_str();
                if !value.is_empty() {
                    out.push(String::from(value));
                }
                if let Some(placeholder) = &input.placeholder {
                    out.push(String::from(placeholder.as_str()));
                }
            }
            _ => {}
        }
        for child in children(node) {
            walk(child, out);
        }
    }
    let mut out = Vec::new();
    walk(node, &mut out);
    out
}

/// Runs `f` against a freshly mounted stash. Effects go to [`NoIo`], so the
/// listing stays in its honest loading state and the test exercises chrome +
/// state machine, not the filesystem.
fn with_stash<R>(
    f: impl FnOnce(
        &mut View<'_>,
        &dyn nexus_theme_tokens::Tokens,
        &FixtureEnv,
        &IdentityLocale<'_>,
    ) -> R,
) -> R {
    let bytes = common::compile("stash");
    let tokens = nexus_theme_tokens::BaseTokens;
    let device = FixtureEnv::desktop();
    // The program's own symbol + key tables, so `@t(...)` resolves to the
    // baked default-locale text instead of a row of empty strings.
    let symbols = common::program_symbols(&bytes);
    let keys = common::program_i18n_keys(&bytes);
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view = View::mount(&bytes, &tokens, &device, &locale).expect("stash mounts");
    f(&mut view, &tokens, &device, &locale)
}

/// Dispatches an event case by name with no payload.
fn fire(
    view: &mut View<'_>,
    tokens: &dyn nexus_theme_tokens::Tokens,
    device: &FixtureEnv,
    locale: &IdentityLocale<'_>,
    case: &str,
) {
    let (event, case_idx) = view
        .runtime()
        .event_case("StashEvent", case)
        .unwrap_or_else(|| panic!("stash declares `{case}`"));
    let mut host = NoIo;
    view.dispatch(tokens, device, locale, &mut host, event, case_idx, Vec::new())
        .unwrap_or_else(|e| panic!("dispatch {case}: {e:?}"));
}

#[test]
fn stash_compiles_and_mounts() {
    with_stash(|view, _, _, _| {
        let rendered = texts(view.scene());
        assert!(!rendered.is_empty(), "stash renders something");
    });
}

/// All three regions carry a `windowPane` surface, written by THIS APP in the
/// slot bodies — the scaffold paints nothing (see `window_kit.rs`). The levels
/// are the assertion, not the count: `panel` here would be the near-black dock
/// tile that made the file listing read as a black slab inside a grey window.
#[test]
fn every_region_carries_a_window_pane_the_app_wrote_itself() {
    use nexus_layout_types::GlassLevel;
    with_stash(|view, _, _, _| {
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
        let panes = levels.iter().filter(|l| **l == GlassLevel::WindowPane).count();
        assert_eq!(panes, 3, "sidebar, content and properties each write one pane: {levels:?}");
        assert!(
            levels.contains(&GlassLevel::WindowBar),
            "the floating action bar is the denser `windowBar` level: {levels:?}"
        );
        assert!(
            !levels.contains(&GlassLevel::Panel),
            "no region may fall back to the wallpaper-tile `panel` level: {levels:?}"
        );
    });
}

#[test]
fn settings_mode_replaces_the_content_and_hides_the_properties_pane() {
    with_stash(|view, tokens, device, locale| {
        let before = texts(view.scene());
        assert!(
            before.iter().any(|t| t == "Properties"),
            "the properties pane is up to start with: {before:?}"
        );

        fire(view, tokens, device, locale, "SettingsOpen");
        let during = texts(view.scene());
        assert!(
            during.iter().any(|t| t == "Show hidden files"),
            "settings content is in: {during:?}"
        );
        assert!(
            !during.iter().any(|t| t == "Properties"),
            "the properties pane is gone, not merely covered: {during:?}"
        );
        assert!(
            !during.iter().any(|t| t == "Name"),
            "the file column headers are gone too: {during:?}"
        );

        fire(view, tokens, device, locale, "SettingsClose");
        let after = texts(view.scene());
        assert!(after.iter().any(|t| t == "Properties"), "back arrow returns: {after:?}");
    });
}

#[test]
fn the_properties_pane_follows_the_selection() {
    with_stash(|view, tokens, device, locale| {
        let empty = texts(view.scene());
        assert!(
            empty.iter().any(|t| t.starts_with("Select a file")),
            "no selection = the empty hint: {empty:?}"
        );

        let (event, case) =
            view.runtime().event_case("StashEvent", "Pick").expect("stash declares Pick");
        let mut host = NoIo;
        let payload = vec![
            nexus_dsl_runtime::Value::Str(String::from("IMG_4521.jpg")),
            nexus_dsl_runtime::Value::Str(String::from("IMG_4521.jpg")),
            nexus_dsl_runtime::Value::Str(String::from("jpg")),
            nexus_dsl_runtime::Value::Str(String::from("4,2 MB")),
            nexus_dsl_runtime::Value::Str(String::from("12.05.2026")),
        ];
        view.dispatch(tokens, device, locale, &mut host, event, case, payload).expect("picks");

        let picked = texts(view.scene());
        assert!(picked.iter().any(|t| t == "4,2 MB"), "the size lands in the pane: {picked:?}");
        assert!(
            !picked.iter().any(|t| t.starts_with("Select a file")),
            "the empty hint is gone: {picked:?}"
        );
    });
}

#[test]
fn the_action_bar_swaps_its_items_on_selection() {
    with_stash(|view, tokens, device, locale| {
        let idle = texts(view.scene());
        assert!(idle.iter().any(|t| t == "New"), "no selection = create/sort: {idle:?}");
        assert!(!idle.iter().any(|t| t == "Move"), "no file actions yet: {idle:?}");

        let (event, case) = view.runtime().event_case("StashEvent", "Pick").expect("Pick");
        let mut host = NoIo;
        let payload = vec![
            nexus_dsl_runtime::Value::Str(String::from("a.txt")),
            nexus_dsl_runtime::Value::Str(String::from("a.txt")),
            nexus_dsl_runtime::Value::Str(String::from("txt")),
            nexus_dsl_runtime::Value::Str(String::from("1 KB")),
            nexus_dsl_runtime::Value::Str(String::from("01.01.2026")),
        ];
        view.dispatch(tokens, device, locale, &mut host, event, case, payload).expect("picks");

        let selected = texts(view.scene());
        assert!(selected.iter().any(|t| t == "Move"), "file actions in: {selected:?}");
        assert!(!selected.iter().any(|t| t == "New"), "create/sort out: {selected:?}");
    });
}

#[test]
fn the_search_field_opens_and_clears() {
    // Dispatch what the BUTTON dispatches. This used to fire a `SearchToggle`
    // event that no widget in the app sends, which is how a real defect hid
    // behind a green test: because nothing dispatched it, `SearchToggle` was a
    // ROOT effect, `run_initial_effects` dispatched it at mount, and the field
    // opened by itself on every launch. Testing the event the toolbar actually
    // emits is what makes this test able to fail.
    with_stash(|view, tokens, device, locale| {
        let magnifier = |view: &mut nexus_dsl_runtime::View<'_>| {
            let (event, case) =
                view.runtime().event_case("StashEvent", "WinTool").expect("stash declares WinTool");
            let mut host = NoIo;
            view.dispatch(
                tokens,
                device,
                locale,
                &mut host,
                event,
                case,
                vec![nexus_dsl_runtime::Value::Str(String::from("magnifyingglass"))],
            )
            .expect("toggles the search field");
        };

        let closed = texts(view.scene());
        assert!(
            !closed.iter().any(|t| t == "Search this folder"),
            "no field before the magnifier: {closed:?}"
        );

        magnifier(view);
        let open = texts(view.scene());
        assert!(
            open.iter().any(|t| t == "Search this folder"),
            "the magnifier opens the field: {open:?}"
        );

        magnifier(view);
        let closed_again = texts(view.scene());
        assert!(
            !closed_again.iter().any(|t| t == "Search this folder"),
            "toggling closes it again: {closed_again:?}"
        );
    });
}

/// The mount-time defect the test above now covers, stated directly: running
/// the program's ROOT effects must not change what the user sees. An `@effect`
/// on an event nothing dispatches is a root, and `run_initial_effects`
/// dispatches roots THROUGH THE REDUCER — so an orphaned event with a reducer
/// arm silently rewrites the initial state.
#[test]
fn running_the_initial_effects_does_not_open_the_search_field() {
    with_stash(|view, tokens, device, locale| {
        let mut host = NoIo;
        view.run_initial_effects(tokens, device, locale, &mut host).expect("initial effects run");
        let shown = texts(view.scene());
        assert!(
            !shown.iter().any(|t| t == "Search this folder"),
            "the app must boot with the search CLOSED: {shown:?}"
        );
    });
}

#[test]
fn the_view_mode_switches_between_list_and_grid() {
    with_stash(|view, tokens, device, locale| {
        let list = texts(view.scene());
        assert!(list.iter().any(|t| t == "Date"), "list mode shows column headers: {list:?}");

        let (event, case) = view.runtime().event_case("StashEvent", "WinTool").expect("WinTool");
        let mut host = NoIo;
        view.dispatch(
            tokens,
            device,
            locale,
            &mut host,
            event,
            case,
            vec![nexus_dsl_runtime::Value::Str(String::from("square.grid.2x2"))],
        )
        .expect("switches to grid");

        let grid = texts(view.scene());
        assert!(!grid.iter().any(|t| t == "Date"), "grid mode drops them: {grid:?}");
    });
}

/// Tapping the open search field ANYWHERE ON THE VISIBLE STRIP must claim text
/// focus. This is the step the whole live-filter chain hangs on: app-host only
/// announces `OP_SURFACE_TEXT_FOCUS` when `focus_text_at` returns a snapshot,
/// and windowd drops every imed commit whose surface id has no recorded focus
/// route. A `None` here is invisible in the scene and fatal at runtime — no
/// caret, keystrokes routed nowhere, the list never filters.
///
/// The tap deliberately lands near the RIGHT EDGE, not the centre: `TextField`
/// builds a bare `TextInput` whose `FlexItem` defaults to `flex_grow: 0`, so a
/// field that only had `.grow(1)` on its WRAPPER painted a full-width strip
/// over a ~180px hit box. A centre tap passed while every realistic click on
/// the strip did nothing.
#[test]
fn tapping_the_open_search_field_claims_text_focus() {
    with_stash(|view, tokens, device, locale| {
        let (event, case) =
            view.runtime().event_case("StashEvent", "WinTool").expect("stash declares WinTool");
        let mut host = NoIo;
        view.dispatch(
            tokens,
            device,
            locale,
            &mut host,
            event,
            case,
            vec![nexus_dsl_runtime::Value::Str(String::from("magnifyingglass"))],
        )
        .expect("opens the search field");

        let boxes = common::layout_boxes(view);
        let (strip_id, input_id) =
            text_input_and_strip(view.scene()).expect("the open field has a TextInput");
        let find = |id: usize| {
            boxes.iter().find(|b| b.node_id == id).map(|b| b.rect).expect("node is laid out")
        };
        let (strip, field) = (find(strip_id), find(input_id));

        // Three quarters across the strip: past the placeholder text, short of
        // the clear button. This is the ordinary "click the search box" spot,
        // and the one a hugging input leaves inert.
        let (tx, ty) = (
            nexus_layout_types::FxPx::new(strip.x.0 + strip.width.0 * 3 / 4),
            nexus_layout_types::FxPx::new(strip.y.0 + strip.height.0 / 2),
        );
        assert!(
            view.focus_text_at(&boxes, tx, ty, None).is_some(),
            "a tap three quarters across the strip ({},{}) claims focus; \
             the input covers only x {}..{} of a {}..{} strip",
            tx.0,
            ty.0,
            field.x.0,
            field.x.0 + field.width.0,
            strip.x.0,
            strip.x.0 + strip.width.0,
        );
    });
}

/// `(strip id, TextInput id)` for the first `TextInput` in the scene, in the
/// same pre-order `LayoutEngine::place_node` uses — so both index straight into
/// `boxes`. The "strip" is the nearest ancestor holding more than the field
/// alone (here: magnifier · field · clear), i.e. the box the user reads as the
/// search box. Single-child wrappers in between are invisible to them.
fn text_input_and_strip(root: &LayoutNode) -> Option<(usize, usize)> {
    fn walk(node: &LayoutNode, strip: usize, next: &mut usize) -> Option<(usize, usize)> {
        let id = *next;
        *next += 1;
        if matches!(node, LayoutNode::TextInput(..)) {
            return Some((strip, id));
        }
        let kids = children(node);
        let strip_below = if kids.len() > 1 { id } else { strip };
        for child in kids {
            if let Some(found) = walk(child, strip_below, next) {
                return Some(found);
            }
        }
        None
    }
    walk(root, 0, &mut 0)
}
