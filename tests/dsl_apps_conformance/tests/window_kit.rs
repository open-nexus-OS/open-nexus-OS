// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
// The window kit (RFC-0084; TASK-0353 P7 — the design artifacts' window primitives) as one
// program: every component inlined, a minimal store of the kit's contract, and pages that use
// them. Two kinds of claims live here.
//
// - The SCAFFOLD contract (`WinAppWindow`): inline regions paint nothing — a region that wants a
//   surface gets one because the app wrote it (a `WinPane`, a `Panel`) — and whatever the body
//   writes survives the splice with the region's geometry intact; the tiers move the panes out
//   of flow, and out of flow the kit frames them on the overlay surface.
// - The DESIGN contract of each part, as geometry and behaviour: the 52 px chrome with its
//   circles and a toolbar centred on the window, the window menu under the chip, the app menu
//   under the ⋮, menu rows, the alert that closes on a backdrop tap, the modal, the pane, the
//   rows and the floating action bar.

// reason: test harness — a failed compile/mount step must panic loudly.
#![allow(clippy::expect_used, clippy::unwrap_used)]

mod common;

use nexus_dsl_runtime::interact::HandlerAction;
use nexus_dsl_runtime::{DismissReason, FixtureEnv, IdentityLocale, Value, View};
use nexus_layout::LayoutBox;
use nexus_layout_types::{FxPx, LayoutNode, Material, Position};
use nexus_theme_tokens::BaseTokens;

/// Every kit component (an app pulls them through `dependencies = ["window-kit"]`).
/// The kit's every component file, read from its folder (sorted): a new part of the kit needs
/// no list here.
fn kit() -> String {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../userspace/apps/window-kit/ui/components");
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
        .expect("the kit's components")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "nx"))
        .collect();
    files.sort();
    files
        .iter()
        .map(|p| std::fs::read_to_string(p).expect("a kit file"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The kit's store contract (fields the chrome reads, every event a kit part dispatches).
const STORE: &str = r#"Store S {
    x: Int = 0,
    menu: Str = "",
    leftOpen: Int = 1,
    rightOpen: Int = 1,
    paneL: Int = 0,
    paneR: Int = 0,
    shown: Bool = true,
}

Event E {
    WinNoop,
    WinPaneClose,
    WinMenu(Str),
    WinAct(Str),
    WinMenuPick(Str),
    WinLeft,
    WinRight,
    WinTool(Str),
    WinNav(Str),
    ModalClose(Str),
    AlertConfirm(Str),
    AlertCancel(Str),
    ToastDismiss(Str),
    SheetCancel(Str),
}

reduce E {
    WinNoop => state.x = state.x,
    WinPaneClose => state.x = state.x,
    WinMenu(m) => state.menu = m,
    WinAct(a) => state.x = 1,
    WinMenuPick(id) => state.x = 2,
    WinLeft => state.leftOpen = 0,
    WinRight => state.rightOpen = 0,
    WinTool(t) => state.x = 3,
    WinNav(d) => state.x = 4,
    ModalClose(id) => state.shown = false,
    AlertConfirm(id) => state.x = 5,
    AlertCancel(id) => state.shown = false,
    ToastDismiss(id) => state.shown = false,
    SheetCancel(id) => state.shown = false,
}"#;

fn compile(page: &str) -> Vec<u8> {
    let src = format!("{}\n{STORE}\n{page}\n", kit());
    let file = nexus_dsl_core::parse_file(&src).expect("parses");
    let (model, diags) = nexus_dsl_core::check_file(&file);
    assert!(!nexus_dsl_core::has_errors(&diags), "check: {diags:?}");
    let canonical = nexus_dsl_core::format_file(&file);
    nexus_dsl_core::lower_file(&file, &model, &canonical).expect("lowers").nxir
}

/// The effect host: the kit's window effects have nowhere to go in a fixture.
struct Sink;

impl nexus_dsl_runtime::EffectHost for Sink {
    fn call(&mut self, _: &str, _: &str, _: &[Value]) -> Result<Value, u32> {
        Ok(Value::Bool(true))
    }
}

/// A desktop env pinned to one size class (RFC-0084 P7 tier tests).
fn env(size_class: &'static str) -> FixtureEnv {
    let mut env = FixtureEnv::default();
    env.size_class = size_class;
    env
}

/// A mounted fixture: the view, its symbols and the device it was mounted for.
struct Kit<'p> {
    view: View<'p>,
    symbols: Vec<String>,
    device: FixtureEnv,
}

impl<'p> Kit<'p> {
    fn mount(nxir: &'p [u8], device: FixtureEnv) -> Self {
        let symbols = common::program_symbols(nxir);
        let keys: Vec<u32> = Vec::new();
        let view = {
            let locale = IdentityLocale { symbols: &symbols, keys: &keys };
            View::mount(nxir, &BaseTokens, &device, &locale).expect("mounts")
        };
        Self { view, symbols, device }
    }

    fn scene(&self) -> &LayoutNode {
        self.view.scene()
    }

    fn boxes(&self, w: i32, h: i32) -> Vec<LayoutBox> {
        nexus_layout::LayoutEngine::new()
            .layout_with_viewport(
                self.scene(),
                FxPx::new(w),
                Some(FxPx::new(h)),
                &nexus_text_baked::measure_text::BakedTextMeasure,
            )
            .expect("lays out")
            .boxes
    }

    fn dispatch(&mut self, case: &str, payload: Vec<Value>) {
        common::dispatch(
            &mut self.view,
            &self.device,
            &mut Sink,
            &self.symbols,
            "E",
            case,
            payload,
        );
    }

    fn tap(&mut self, boxes: &[LayoutBox], (x, y): (i32, i32)) {
        let keys: Vec<u32> = Vec::new();
        let locale = IdentityLocale { symbols: &self.symbols, keys: &keys };
        let (x, y) = (FxPx::new(x), FxPx::new(y));
        self.view
            .pointer_scrolled(
                &BaseTokens,
                &self.device,
                &locale,
                &mut Sink,
                boxes,
                "Tap",
                x,
                y,
                &[],
            )
            .expect("tap runs");
    }

    fn field(&self, name: &str) -> Value {
        self.view.runtime.field("S", name).cloned().expect("field")
    }

    /// The boxes of every Tap handler dispatching `case` (with `payload`, when given).
    fn handler_boxes(
        &self,
        boxes: &[LayoutBox],
        case: &str,
        payload: Option<&str>,
    ) -> Vec<LayoutBox> {
        let (e, c) = self.view.runtime.event_case("E", case).expect("case");
        let tap = self.symbols.iter().position(|s| s == "Tap").expect("Tap") as u32;
        let want = payload.map(|p| Value::Str(p.to_string()));
        self.view
            .handlers()
            .iter()
            .filter(|(_, h)| {
                h.trigger == tap
                    && matches!(&h.action, HandlerAction::Dispatch { event, case, payload: p }
                        if *event == e && *case == c && want.as_ref().is_none_or(|w| p.first() == Some(w)))
            })
            .filter_map(|(id, _)| boxes.iter().find(|b| b.node_id == *id).cloned())
            .collect()
    }
}

/// `(x, y, w, h)` of a box.
fn r(b: &LayoutBox) -> (i32, i32, i32, i32) {
    (b.rect.x.as_i32(), b.rect.y.as_i32(), b.rect.width.as_i32(), b.rect.height.as_i32())
}

/// The scene's nodes in the layout engine's numbering: the root is 1, a stack's in-flow
/// children come before its `.overlay()` ones (the walk app-host paints and hit-tests by).
fn numbered(node: &LayoutNode) -> Vec<(usize, &LayoutNode)> {
    fn walk<'a>(node: &'a LayoutNode, next: &mut usize, out: &mut Vec<(usize, &'a LayoutNode)>) {
        *next += 1;
        out.push((*next, node));
        match node {
            LayoutNode::Stack(_, _, children) => {
                let absolute = |c: &&LayoutNode| matches!(c.item().position, Position::Absolute);
                for child in children.iter().filter(|c| !absolute(c)) {
                    walk(child, next, out);
                }
                for child in children.iter().filter(absolute) {
                    walk(child, next, out);
                }
            }
            LayoutNode::Grid(_, _, children) => {
                for child in children {
                    walk(child, next, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(node, &mut 0, &mut out);
    out
}

/// Every Text content in the scene, pre-order (the slot bodies' labels).
fn texts(node: &LayoutNode) -> Vec<String> {
    numbered(node)
        .into_iter()
        .filter_map(|(_, n)| match n {
            LayoutNode::Text(text, _) => Some(String::from(text.content.as_str())),
            _ => None,
        })
        .collect()
}

/// The laid-out box of the Text reading `content`.
fn text_box(scene: &LayoutNode, boxes: &[LayoutBox], content: &str) -> LayoutBox {
    let id = numbered(scene)
        .into_iter()
        .find(|(_, n)| matches!(n, LayoutNode::Text(t, _) if t.content.as_str() == content))
        .map(|(id, _)| id)
        .unwrap_or_else(|| panic!("no text {content:?}"));
    boxes.iter().find(|b| b.node_id == id).cloned().expect("text box")
}

/// The materials painted in the scene, pre-order.
fn materials(node: &LayoutNode) -> Vec<Material> {
    numbered(node)
        .into_iter()
        .filter_map(|(_, n)| match n {
            LayoutNode::Stack(_, visual, _) | LayoutNode::Grid(_, visual, _) => visual.material,
            _ => None,
        })
        .collect()
}

/// The laid-out boxes carrying `material`, pre-order.
fn material_boxes(boxes: &[LayoutBox], material: Material) -> Vec<LayoutBox> {
    boxes.iter().filter(|b| b.visual.material == Some(material)).cloned().collect()
}

// ── the scaffold ────────────────────────────────────────────────────────────────────────────

/// A page using the scaffold with the given props and content body.
fn page(flags: &str, content: &str) -> String {
    format!(
        r#"Page P {{
    WinAppWindow {{ {flags}, menuFullscreen: "Full screen", menuMoveDevice: "Move to another device", menuMinimize: "Minimise", menuClose: "Close" }} {{
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
const OVERLAYS: &str =
    "showSidebar: true, showProps: true, sidebarOverlay: true, propsOverlay: true";
const BARE: &str = r#"            Text("body")"#;
const PANELLED: &str = r#"            Panel {
                Text("body")
            }
            .grow(1)"#;

#[test]
fn the_scaffold_compiles_and_mounts() {
    let nxir = compile(&page(SHOWN, BARE));
    let kit = Kit::mount(&nxir, FixtureEnv::default());
    assert_eq!(texts(kit.scene()), ["nav", "body", "props"]);
}

#[test]
fn the_scaffold_paints_no_surface_of_its_own() {
    // The kit contributes geometry, never paint, to the inline regions: bodies of bare `Text`
    // ⇒ no surface anywhere in the scene, at every tier, while no pane is out of flow.
    let nxir = compile(&page(SHOWN, BARE));
    for tier in ["wide", "regular", "compact"] {
        let kit = Kit::mount(&nxir, env(tier));
        assert_eq!(materials(kit.scene()), [], "{tier}: the kit painted a surface");
    }
}

#[test]
fn a_body_may_write_any_number_of_panels_at_its_own_level() {
    // Zero, one, three — the app decides by writing markup, not by flipping a flag; and each
    // picks its own material (`pane` inside a window, the opaque level a content pane sits on).
    let nxir = compile(&page(SHOWN, PANELLED));
    assert_eq!(materials(Kit::mount(&nxir, FixtureEnv::default()).scene()), [Material::Panel]);
    let three = r#"            Panel { Text("one") }.material(pane)
            Panel { Text("two") }.material(pane)
            Panel { Text("three") }.material(pane)"#;
    let nxir = compile(&page(SHOWN, three));
    let kit = Kit::mount(&nxir, FixtureEnv::default());
    assert_eq!(materials(kit.scene()), [Material::Pane; 3]);
    assert_eq!(texts(kit.scene()), ["nav", "one", "two", "three", "props"]);
}

#[test]
fn a_hidden_region_contributes_nothing() {
    // The sidebar and properties BODIES are still bound by the caller; the scaffold simply
    // never places their placeholders.
    let nxir = compile(&page("showSidebar: false, showProps: false", BARE));
    assert_eq!(texts(Kit::mount(&nxir, FixtureEnv::default()).scene()), ["body"]);
}

#[test]
fn the_regions_sit_8_px_apart_and_a_body_panel_cannot_move_them() {
    // Body inset 0 8 8, regions 8 apart: sidebar 240 · content the rest · properties 240 at
    // 1280×800 — whatever the bodies contain. A panel in the content body fills its region
    // exactly (the pane radius 16 is then concentric with the window's 24).
    let geometry = |content: &str, flags: &str| {
        let nxir = compile(&page(flags, content));
        let kit = Kit::mount(&nxir, FixtureEnv::default());
        let boxes = kit.boxes(1280, 800);
        let nav = r(&text_box(kit.scene(), &boxes, "nav"));
        let props = r(&text_box(kit.scene(), &boxes, "props"));
        let panel = material_boxes(&boxes, Material::Panel).first().map(r);
        (nav, props, panel)
    };
    let (nav, props, _) = geometry(BARE, SHOWN);
    assert_eq!((nav.0, nav.1, nav.2), (8, 0, 240), "the sidebar region");
    assert_eq!((props.0, props.1, props.2), (1032, 0, 240), "the properties region");
    let (nav2, props2, panel) = geometry(PANELLED, SHOWN);
    assert_eq!((nav, props), (nav2, props2), "a body's panel must not move or resize a region");
    assert_eq!(panel, Some((256, 0, 768, 792)), "the content panel fills its region");

    // The app sizes its side regions (the settings design: 284; the files design 248 · 272).
    let (nav, props, panel) = geometry(
        PANELLED,
        "showSidebar: true, showProps: true, sidebarWidth: 248, propsWidth: 272",
    );
    assert_eq!((nav.0, nav.2, props.0, props.2), (8, 248, 1000, 272));
    assert_eq!(panel, Some((264, 0, 728, 792)));
}

#[test]
fn responsive_tiers_move_panes_out_of_flow() {
    // RFC-0084 P7, the design's modes on the platform's tiers: wide keeps both side zones
    // inline (desktop), regular moves the sidebar out (compact), compact both (mobile).
    let nxir = compile(&page(SHOWN, BARE));
    let tier = |t| texts(Kit::mount(&nxir, env(t)).scene());
    assert_eq!(tier("wide"), ["nav", "body", "props"]);
    assert_eq!(tier("regular"), ["body", "props"], "regular: the sidebar leaves the flow");
    assert_eq!(tier("compact"), ["body"], "compact: content alone");
}

#[test]
fn out_of_flow_panes_come_back_on_the_kits_overlay_surface() {
    // The out-of-flow panes return as OVERLAY panes (caller-controlled flags), and there the
    // kit frames them: the opaque overlay surface (it floats above the window's blur), the
    // app's body inside it. Bare bodies ⇒ exactly the kit's surfaces; inline panes stay
    // unframed.
    let nxir = compile(&page(OVERLAYS, BARE));
    let compact = Kit::mount(&nxir, env("compact"));
    assert_eq!(texts(compact.scene()), ["body", "nav", "props"], "both panes overlay");
    assert_eq!(materials(compact.scene()), [Material::Overlay; 2]);
    let regular = Kit::mount(&nxir, env("regular"));
    assert_eq!(texts(regular.scene()), ["body", "props", "nav"], "the sidebar overlays");
    assert_eq!(materials(regular.scene()), [Material::Overlay]);
    let wide = Kit::mount(&nxir, env("wide"));
    assert_eq!(texts(wide.scene()), ["nav", "body", "props"], "wide ignores overlay flags");
    assert_eq!(materials(wide.scene()), []);

    // The app's own panel is not doubled: once, beside the kit's two frames.
    let nxir = compile(&page(OVERLAYS, PANELLED));
    let compact = Kit::mount(&nxir, env("compact"));
    let panes = materials(compact.scene());
    assert_eq!(panes.iter().filter(|m| **m == Material::Panel).count(), 1, "{panes:?}");
}

#[test]
fn overlay_panes_take_the_design_geometry_per_tier() {
    let nxir = compile(&page(OVERLAYS, BARE));
    // regular (the design's compact): top 0, left 8, bottom 8, 256 wide, radius 16.
    let kit = Kit::mount(&nxir, env("regular"));
    let boxes = kit.boxes(1280, 800);
    let panes = material_boxes(&boxes, Material::Overlay);
    assert_eq!(panes.iter().map(r).collect::<Vec<_>>(), [(8, 0, 256, 792)]);
    assert_eq!(panes[0].visual.corner_radius.top_left.as_i32(), 16);
    // compact (the design's mobile): flush with the edge, 300 / 320 wide, the outer corners
    // square and the inner ones 24.
    let kit = Kit::mount(&nxir, env("compact"));
    let boxes = kit.boxes(1280, 800);
    let panes = material_boxes(&boxes, Material::Overlay);
    assert_eq!(panes.iter().map(r).collect::<Vec<_>>(), [(0, 0, 300, 800), (960, 0, 320, 800)]);
    let corners = |b: &LayoutBox| {
        let c = b.visual.corner_radius;
        (c.top_left.as_i32(), c.top_right.as_i32(), c.bottom_right.as_i32(), c.bottom_left.as_i32())
    };
    assert_eq!((corners(&panes[0]), corners(&panes[1])), ((0, 24, 24, 0), (24, 0, 0, 24)));
}

// ── the chrome and the menus ────────────────────────────────────────────────────────────────

const CHROME: &str = r#"Page P {
    Stack {
        WinTopBar { icon: "folder", tool1: "magnifyingglass", tool2: "square.grid.2x2", tool3: "list.bullet", tool4: "", tool1Active: false, tool2Active: false, tool3Active: true, tool4Active: false, tool1Disabled: false, tool2Disabled: false, tool3Disabled: false, tool4Disabled: false, navBack: "arrow.left", navFwd: "arrow.right", navRefresh: "", navBackDisabled: false, navFwdDisabled: true, sideLeft: "sidebar.left", sideRight: "sidebar.right", more: "ellipsis.vertical", moreActive: false, appId: "" }
        Spacer
    }
}"#;

/// Whether the circle a tap handler wraps (its first child) carries the `fill` layer.
fn pinned(kit: &Kit, boxes: &[LayoutBox], case: &str, payload: Option<&str>) -> bool {
    let cell = &kit.handler_boxes(boxes, case, payload)[0];
    let face = boxes.iter().find(|b| b.node_id == cell.node_id + 1).expect("face");
    face.visual.background.is_some()
}

#[test]
fn the_title_chrome_is_52_px_of_circles_around_a_toolbar_centred_on_the_window() {
    let nxir = compile(CHROME);
    let mut kit = Kit::mount(&nxir, env("wide"));
    let boxes = kit.boxes(960, 600);
    assert_eq!(r(&kit.handler_boxes(&boxes, "WinAct", Some("move"))[0]), (0, 0, 960, 52));
    // The window controls: 36 px circles 2 apart, the last 12 px from the window edge.
    let controls: Vec<_> = ["minimize", "zoom", "close"]
        .iter()
        .map(|a| r(&kit.handler_boxes(&boxes, "WinAct", Some(a))[0]))
        .collect();
    assert_eq!(controls, [(836, 8, 36, 36), (874, 8, 36, 36), (912, 8, 36, 36)]);
    // The toolbar: back · forward (dimmed, no tap) | search | grid · list in 32 px cells,
    // centred on the window whatever the zones beside it hold.
    assert!(kit.handler_boxes(&boxes, "WinNav", Some("fwd")).is_empty(), "forward has no target");
    let back = r(&kit.handler_boxes(&boxes, "WinNav", Some("back"))[0]);
    let list = r(&kit.handler_boxes(&boxes, "WinTool", Some("list.bullet"))[0]);
    assert_eq!((back.2, back.3, list.2, list.3), (32, 30, 32, 30));
    assert_eq!(back.0 + list.0 + list.2, 960, "the cluster is symmetric about the centre");
    // Active = the pinned `fill` layer: the list mode, the shown panes (wide: inline flags).
    assert!(pinned(&kit, &boxes, "WinTool", Some("list.bullet")));
    assert!(!pinned(&kit, &boxes, "WinTool", Some("magnifyingglass")));
    assert!(pinned(&kit, &boxes, "WinLeft", None) && pinned(&kit, &boxes, "WinRight", None));
    kit.dispatch("WinLeft", vec![]);
    assert!(!pinned(&kit, &kit.boxes(960, 600), "WinLeft", None), "a hidden sidebar unpins");
    // Below wide the sidebar is an overlay: its toggle follows the overlay flag instead.
    let regular = Kit::mount(&nxir, env("regular"));
    assert!(!pinned(&regular, &regular.boxes(960, 600), "WinLeft", None));
    // The app chip opens the window menu and wears the `fill` layer while it is open.
    let chip =
        |kit: &Kit| kit.handler_boxes(&kit.boxes(960, 600), "WinMenu", Some("app"))[0].clone();
    assert_eq!((r(&chip(&kit)).0, r(&chip(&kit)).3), (12, 30));
    assert!(chip(&kit).visual.background.is_none());
    kit.dispatch("WinMenu", vec![Value::Str("app".into())]);
    assert!(chip(&kit).visual.background.is_some());
}

#[test]
fn the_window_menu_opens_under_the_chip_with_thirteen_tiles() {
    let nxir = compile(&page(SHOWN, BARE));
    let mut kit = Kit::mount(&nxir, FixtureEnv::default());
    assert!(kit.handler_boxes(&kit.boxes(1280, 800), "WinAct", None).is_empty(), "closed");
    kit.dispatch("WinMenu", vec![Value::Str("app".into())]);
    let boxes = kit.boxes(1280, 800);
    let panel = material_boxes(&boxes, Material::Overlay);
    let (x, y, w, _) = r(&panel[0]);
    assert_eq!((x, y, w), (12, 0, 340), "the chip's left edge, 340 wide");
    let acts = kit.handler_boxes(&boxes, "WinAct", None);
    assert_eq!(acts.iter().filter(|b| r(b).3 == 36).count(), 13, "8 + 5 tiles, 36 tall");
    assert_eq!(acts.iter().filter(|b| r(b).3 == 34).count(), 3, "three 34 px rows (+ inert)");
    for zone in ["zone.top-half", "zone.bottom-half", "zone.return"] {
        assert_eq!(kit.handler_boxes(&boxes, "WinAct", Some(zone)).len(), 1, "{zone}");
    }
    assert_eq!(kit.handler_boxes(&boxes, "WinMenu", Some("")).len(), 1, "the backdrop");
}

const APP_MENU: &str = r#"Page P {
    WinAppWindow { showSidebar: true, showProps: true, appMenuOpen: true } {
        contentArea {
            Text("body")
        }
        appMenu {
            WinMenu { width: 240 } {
                items {
                    WinMenuRow { label: "Datei", icon: "", kind: "header", shortcut: "", selected: false }
                    WinMenuItem { id: "new", label: "Neues Fenster", icon: "plus", kind: "", act: false, shortcut: "Strg+N", selected: false }
                    WinMenuDivider { }
                    WinMenuItem { id: "close", label: "Fenster schließen", icon: "xmark", kind: "danger", act: true, shortcut: "", selected: false }
                }
            }
        }
    }
}"#;

#[test]
fn the_app_menu_hangs_under_the_more_button_and_its_rows_dispatch() {
    let nxir = compile(APP_MENU);
    let kit = Kit::mount(&nxir, FixtureEnv::default());
    let boxes = kit.boxes(1280, 800);
    let panel = &material_boxes(&boxes, Material::Overlay)[0];
    let (x, y, w, h) = r(panel);
    assert_eq!((x + w, y, w), (1280 - 145, 0, 240), "its right edge on the ⋮'s");
    assert_eq!(panel.visual.corner_radius.top_left.as_i32(), 16);
    assert_eq!(h, 4 + 26 + 36 + 9 + 36 + 4, "padding 4 · header 26 · rows 36 · divider 9");
    let pick = r(&kit.handler_boxes(&boxes, "WinMenuPick", Some("new"))[0]);
    let act = r(&kit.handler_boxes(&boxes, "WinAct", Some("close"))[0]);
    assert_eq!((pick.3, act.3), (36, 36), "a plain row picks, an act row is a window action");
    assert_eq!(kit.handler_boxes(&boxes, "WinMenu", Some("")).len(), 1, "the backdrop closes");
}

// ── the dialogs ─────────────────────────────────────────────────────────────────────────────

const ALERT: &str = r#"Page P {
    Stack {
        Text("behind")
        if $state.shown {
            WinAlert { id: "a", title: "Löschen?", message: "Nicht rückgängig.", cancelLabel: "Abbrechen", confirmLabel: "Löschen", destructive: true }
        } else {
            Stack { }
        }
    }
}"#;

#[test]
fn the_alert_is_a_300_px_card_that_closes_on_a_backdrop_tap() {
    let nxir = compile(ALERT);
    let mut kit = Kit::mount(&nxir, FixtureEnv::default());
    let boxes = kit.boxes(1280, 800);
    let card = r(&material_boxes(&boxes, Material::Overlay)[0]);
    assert_eq!((card.0, card.2, card.3), (490, 300, 20 + 22 + 4 + 18 + 16 + 1 + 46));
    let cancel = r(&kit.handler_boxes(&boxes, "AlertCancel", Some("a"))[0]);
    let confirm = r(&kit.handler_boxes(&boxes, "AlertConfirm", Some("a"))[0]);
    assert_eq!((cancel.3, confirm.3, cancel.0 + cancel.2 + 1), (46, 46, confirm.0), "edge to edge");
    // A press on the card's text closes nothing (the card absorbs it) …
    let message = r(&text_box(kit.scene(), &boxes, "Nicht rückgängig."));
    kit.tap(&boxes, (message.0 + 4, message.1 + 4));
    assert_eq!((kit.view.take_dismissed(), kit.field("shown")), (None, Value::Bool(true)));
    // … a press on the backdrop cancels, like the design's alert.
    kit.tap(&boxes, (20, 20));
    assert_eq!(kit.view.take_dismissed(), Some(DismissReason::Backdrop));
    assert_eq!(kit.field("shown"), Value::Bool(false));
}

#[test]
fn the_modal_is_440_wide_with_a_titled_header_a_body_and_a_footer() {
    let modal = |footer: bool| {
        format!(
            r#"Page P {{
    Stack {{
        if $state.shown {{
            WinModal {{ id: "m", title: "Umbenennen", noFooter: {}, closeLabel: "Schließen" }} {{
                body {{
                    Text("Neuer Name")
                }}
                footer {{
                    Text("Sichern")
                }}
            }}
        }} else {{
            Stack {{ }}
        }}
    }}
}}"#,
            !footer
        )
    };
    let nxir = compile(&modal(true));
    let mut kit = Kit::mount(&nxir, FixtureEnv::default());
    let boxes = kit.boxes(1280, 800);
    let panel = r(&material_boxes(&boxes, Material::Overlay)[0]);
    // header 16 + 28 + 16 · hairline · body 20 + 20 + 20 · hairline · footer 12 + 20 + 12
    assert_eq!((panel.0, panel.2, panel.3), (420, 440, 60 + 1 + 60 + 1 + 44));
    let close = r(&kit.handler_boxes(&boxes, "ModalClose", Some("m"))[0]);
    assert_eq!((close.0 + close.2, close.2), (panel.0 + panel.2 - 20, 28));
    kit.tap(&boxes, (20, 20));
    assert_eq!(kit.field("shown"), Value::Bool(false), "the backdrop closes a modal");
    let nxir = compile(&modal(false));
    let kit = Kit::mount(&nxir, FixtureEnv::default());
    let boxes = kit.boxes(1280, 800);
    assert_eq!(r(&material_boxes(&boxes, Material::Overlay)[0]).3, 60 + 1 + 60, "no footer");
}

#[test]
fn the_action_sheet_is_two_overlay_cards_that_cancel_on_the_scrim_and_the_cancel_row() {
    let page = r#"Page P {
    Stack {
        if $state.shown {
            WinActionSheet { id: "s", title: "Löschen?", message: "Nicht rückgängig.", cancelLabel: "Abbrechen" } {
                actions {
                    Stack { WinSheetRow { label: "Papierkorb", destructive: false, rule: false } }
                    on Tap -> dispatch(WinNoop)
                    Stack { WinSheetRow { label: "Löschen", destructive: true, rule: true } }
                    on Tap -> dispatch(WinNoop)
                }
            }
        } else {
            Stack { }
        }
    }
}"#;
    let nxir = compile(page);
    let mut kit = Kit::mount(&nxir, FixtureEnv::default());
    let boxes = kit.boxes(1280, 800);
    let cards = material_boxes(&boxes, Material::Overlay);
    let (actions, cancel) = (r(&cards[0]), r(&cards[1]));
    // The head (12 + 18 + 4 + 16 + 12 · hairline), two 54 px rows with a hairline between them;
    // the cancel card is one 54 px row, 8 px under — both 420 wide, centred, 8 from the bottom.
    assert_eq!((actions.2, actions.3), (420, (12 + 18 + 4 + 16 + 12 + 1) + 54 + (1 + 54)));
    assert_eq!((cancel.2, cancel.3, cancel.1 - (actions.1 + actions.3)), (420, 54, 8));
    assert_eq!((actions.0, cancel.1 + cancel.3), (430, 800 - 8));
    // A press on a card's text closes nothing; the cancel row and the scrim cancel.
    let row = r(&text_box(kit.scene(), &boxes, "Papierkorb"));
    kit.tap(&boxes, (row.0 + 2, row.1 + 2));
    assert_eq!((kit.view.take_dismissed(), kit.field("shown")), (None, Value::Bool(true)));
    kit.tap(&boxes, (20, 20));
    assert_eq!(kit.view.take_dismissed(), Some(DismissReason::Backdrop));
    assert_eq!(kit.field("shown"), Value::Bool(false));
}

#[test]
fn the_fab_face_is_a_56_px_accent_circle() {
    let page = r#"Page P {
    Stack {
        Spacer
        Stack { WinFabFace { icon: "plus" } }
        .label("Neu")
        on Tap -> dispatch(WinNoop)
    }
    .direction(row)
    .align(start)
}"#;
    let nxir = compile(page);
    let kit = Kit::mount(&nxir, FixtureEnv::default());
    let boxes = kit.boxes(1280, 800);
    let fab = r(&kit.handler_boxes(&boxes, "WinNoop", None)[0]);
    assert_eq!((fab.2, fab.3), (56, 56));
}

// ── the pane, the rows, the action bar ──────────────────────────────────────────────────────

const PARTS: &str = r#"Page P {
    WinAppWindow { showSidebar: true, showProps: true } {
        sidebarLeft {
            WinPane { title: "", showHeader: false, showFooter: false, bare: true, flush: true, scrolls: false, noDividers: false } {
                body {
                    WinSideHeader { label: "Orte" }
                    WinSideItem { id: "a", label: "Dokumente", sub: "", icon: "folder", active: true, iconSize: 16, hasBadge: false, badge: "" }
                    WinSideItem { id: "b", label: "Verbindungen", sub: "WLAN, Bluetooth", icon: "wifi", active: false, iconSize: 18, hasBadge: true, badge: "2" }
                }
            }
        }
        contentArea {
            WinPane { title: "Inhalt", showHeader: true, showFooter: true, bare: false, flush: false, scrolls: false, noDividers: false } {
                body {
                    Text("row")
                }
                footer {
                    Text("3 Objekte")
                }
            }
        }
        sidebarRight {
            WinPane { title: "Eigenschaften", showHeader: true, showFooter: false, bare: false, flush: false, scrolls: false, noDividers: false } {
                body {
                    WinPropRow { label: "Art", value: "PDF", icon: "", last: false }
                    WinPropRow { label: "Größe", value: "2,4 MB", icon: "", last: true }
                }
            }
        }
        actionBar {
            WinActionBar { } {
                items {
                    WinActionItem { id: "new", label: "Neu", icon: "plus", kind: "primary", active: true }
                    WinActionDivider { }
                    WinActionItem { id: "del", label: "Löschen", icon: "trash", kind: "danger", active: false }
                }
            }
        }
    }
}"#;

#[test]
fn a_pane_is_an_opaque_16_px_panel_with_44_px_strips() {
    let nxir = compile(PARTS);
    let kit = Kit::mount(&nxir, FixtureEnv::default());
    let boxes = kit.boxes(1280, 800);
    // Two panels (content, properties); the bare sidebar sits on the window glass.
    let panes = material_boxes(&boxes, Material::Pane);
    assert_eq!(panes.iter().map(r).collect::<Vec<_>>(), [(256, 0, 768, 792), (1032, 0, 240, 792)]);
    let pane = &panes[0];
    assert_eq!(pane.visual.corner_radius.top_left.as_i32(), 16);
    assert!(pane.visual.inset_highlight.is_some(), "the lit top line");
    assert!(pane.visual.border.top.is_some_and(|b| b.width.as_i32() == 1), "a 1 px separator edge");
    // header 44 (title body 14/600 at padding 16) · hairline · body padded 4 8 · footer 44
    let at = |t: &str| {
        let b = r(&text_box(kit.scene(), &boxes, t));
        (b.0, b.1)
    };
    assert_eq!(at("Inhalt"), (256 + 16, 12));
    assert_eq!(at("row"), (256 + 8, 44 + 1 + 4));
    assert_eq!(at("3 Objekte"), (256 + 12, 792 - 44 + 12));
}

#[test]
fn rows_take_the_design_metrics() {
    let nxir = compile(PARTS);
    let kit = Kit::mount(&nxir, FixtureEnv::default());
    let boxes = kit.boxes(1280, 800);
    // Properties: 32 px label/value rows (footnote, padding 8 12), a separator between.
    let label = |t: &str| r(&text_box(kit.scene(), &boxes, t));
    assert_eq!((label("Art").0, label("Art").1), (1032 + 8 + 12, 49 + 8));
    assert_eq!(label("Größe").1, 49 + 32 + 1 + 8, "one 1 px separator between the rows");
    let value = label("PDF");
    assert_eq!(value.0 + value.2, 1032 + 240 - 8 - 12, "the value is right-aligned");
    // Sidebar: a 30 px overline header, then 36 px rows (46 with a second line).
    let rows: Vec<_> = ["a", "b"]
        .iter()
        .map(|id| kit.handler_boxes(&boxes, "WinMenuPick", Some(id))[0].clone())
        .collect();
    assert_eq!((r(&rows[0]), r(&rows[1])), ((8, 30, 240, 36), (8, 66, 240, 46)));
    assert!(rows[0].visual.background.is_some() && rows[1].visual.background.is_none(), "active");
    assert_eq!(label("ORTE").1, 12, "the overline header is upper-cased by its style");
}

#[test]
fn the_action_bar_floats_12_px_above_the_content_and_marks_the_active_item() {
    let nxir = compile(PARTS);
    let kit = Kit::mount(&nxir, FixtureEnv::default());
    let boxes = kit.boxes(1280, 800);
    let bar = &material_boxes(&boxes, Material::Overlay)[0];
    let (x, y, w, h) = r(bar);
    assert_eq!((y + h, h, 2 * x + w), (792 - 12, 4 + 49 + 4, 256 + 256 + 768), "centred");
    assert_eq!(bar.visual.corner_radius.top_left.as_i32(), 9999, "a capsule");
    let new = r(&kit.handler_boxes(&boxes, "WinAct", Some("new"))[0]);
    assert!(new.3 == 49 && new.2 >= 60, "TabBar items: 49 tall, at least 60 wide");
    assert!(pinned(&kit, &boxes, "WinAct", Some("new")), "the indicator marks the active item");
    assert!(!pinned(&kit, &boxes, "WinAct", Some("del")));
}
