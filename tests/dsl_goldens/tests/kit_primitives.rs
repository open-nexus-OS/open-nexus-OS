// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! TASK-0353 P7 (wave B): pixel goldens of the window kit's primitives — the title bar (resting
//! and with its states on), the button face, the app chip, the tool cells, the pane, the sidebar
//! rows, the properties rows, the menu and the action bar — each light and dark, through the
//! shared BGRA golden painter (`check_golden_sized`: these are wider than the fixture canvas).
//! The overlays (`overlays.rs`) and the window menu (`tiling_menu.rs`) have their own goldens.
//! The painter draws no glyphs and no backdrop blur, so a case pins geometry, fills, edges and
//! layers; the look with text is judged in the preview (`nx-dsl render`).
//! Regenerate with `UPDATE_GOLDENS=1` (the main session blesses once).

// reason: test harness — a failed compile/mount step must panic loudly.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use dsl_goldens::compile;
use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, Value, View};
use nexus_theme_tokens::{DarkTokens, LightTokens, Tokens};

/// The kit components the cases use (and the parts they pull in), read from the kit's folder.
const PARTS: &[&str] = &[
    "WinTopBar",
    "WinAppChip",
    "WinToolCell",
    "WinButtonFace",
    "WinBarRule",
    "WinPane",
    "WinPaneHead",
    "WinPaneBody",
    "WinPaneFoot",
    "WinPaneTitle",
    "WinPaneRule",
    "WinSideItem",
    "WinSideHeader",
    "WinPropRow",
    "WinMenu",
    "WinMenuItem",
    "WinMenuFace",
    "WinActionBar",
    "WinActionItem",
    "WinActionFace",
    "WinActionSheet",
    "WinFabFace",
];

fn kit() -> String {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../userspace/apps/window-kit/ui/components");
    PARTS
        .iter()
        .map(|name| {
            std::fs::read_to_string(dir.join(format!("{name}.nx"))).expect("a kit component")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

const APP: &str = r#"
Store S {
    which: Str = "",
    menu: Str = "",
    leftOpen: Int = 0,
    rightOpen: Int = 0,
    paneL: Int = 0,
    paneR: Int = 0,
    x: Int = 0,
}

Event E {
    Show(Str),
    WinNoop,
    WinMenu(Str),
    WinAct(Str),
    WinMenuPick(Str),
    WinLeft,
    WinRight,
    WinTool(Str),
    WinNav(Str),
    SheetCancel(Str),
}

reduce E {
    Show(w) => state.which = w,
    WinNoop => state.x = state.x,
    WinMenu(m) => state.menu = m,
    WinAct(a) => state.x = state.x,
    WinMenuPick(id) => state.x = state.x,
    WinLeft => state.leftOpen = 1,
    WinRight => state.rightOpen = 1,
    WinTool(t) => state.x = state.x,
    WinNav(d) => state.x = state.x,
    SheetCancel(id) => state.x = state.x,
}

Page P {
    Stack {
        if $state.which == "top_bar" {
            WinTopBar {
                icon: "folder",
                tool1: "magnifyingglass",
                tool2: "square.grid.2x2",
                tool3: "list.bullet",
                tool1Active: $state.rightOpen == 1,
                tool2Active: false,
                tool3Active: false,
                tool4: "",
                tool4Active: false,
                tool1Disabled: false,
                tool2Disabled: false,
                tool3Disabled: false,
                tool4Disabled: false,
                navBack: "arrow.left",
                navFwd: "arrow.right",
                navRefresh: "",
                navBackDisabled: false,
                navFwdDisabled: true,
                sideLeft: "sidebar.left",
                sideRight: "sidebar.right",
                more: "ellipsis.vertical",
                moreActive: $state.rightOpen == 1,
                appId: "",
            }
        } else if $state.which == "button_face" {
            Stack {
                WinButtonFace { symbol: "gearshape", size: 14, active: false, danger: false, disabled: false, large: false }
                WinButtonFace { symbol: "gearshape", size: 14, active: true, danger: false, disabled: false, large: false }
                WinButtonFace { symbol: "trash", size: 14, active: false, danger: true, disabled: false, large: false }
                WinButtonFace { symbol: "gearshape", size: 14, active: false, danger: false, disabled: true, large: false }
                WinButtonFace { symbol: "minus", size: 14, active: false, danger: false, disabled: false, large: true }
                WinButtonFace { symbol: "xmark", size: 14, active: false, danger: true, disabled: false, large: true }
            }
            .direction(row)
            .align(center)
            .gap(8)
        } else if $state.which == "app_chip" {
            WinAppChip { icon: "folder", appId: "" }
        } else if $state.which == "tool_cells" {
            Stack {
                WinNavCell { symbol: "arrow.left", action: "back", disabled: false }
                WinNavCell { symbol: "arrow.right", action: "fwd", disabled: true }
                WinBarRule { show: true, pad: 4 }
                WinToolCell { symbol: "magnifyingglass", active: true }
                WinToolCell { symbol: "list.bullet", active: false }
            }
            .direction(row)
            .align(center)
        } else if $state.which == "pane_panel" {
            Stack {
                WinPane { title: "Documents", showHeader: true, showFooter: true, bare: false, flush: false, scrolls: false, noDividers: false } {
                    actions {
                        WinButtonFace { symbol: "plus", size: 14, active: false, danger: false, disabled: false, large: false }
                    }
                    body {
                        Text("Body of the pane").textStyle(footnote).fg(label)
                    }
                    footer {
                        Text("3 items").textStyle(footnote).fg(labelSecondary)
                    }
                }
            }
            .height(150)
        } else if $state.which == "pane_bare" {
            Stack {
                WinPane { title: "", showHeader: false, showFooter: true, bare: true, flush: false, scrolls: false, noDividers: false } {
                    body {
                        Text("Body of the pane").textStyle(footnote).fg(label)
                    }
                    footer {
                        Text("Account").textStyle(footnote).fg(labelSecondary)
                    }
                }
            }
            .height(150)
        } else if $state.which == "side" {
            Stack {
                WinSideHeader { label: "Places" }
                WinSideItem { id: "home", label: "Home", sub: "", icon: "house", active: true, iconSize: 16, hasBadge: false, badge: "" }
                WinSideItem { id: "docs", label: "Documents", sub: "", icon: "doc", active: false, iconSize: 16, hasBadge: true, badge: "3" }
                WinSideItem { id: "pics", label: "Pictures", sub: "", icon: "photo", active: false, iconSize: 16, hasBadge: false, badge: "" }
            }
            .gap(2)
            .paddingX(8)
        } else if $state.which == "prop_rows" {
            Stack {
                WinPropRow { label: "Kind", value: "Folder", icon: "", last: false }
                WinPropRow { label: "Size", value: "12 MB", icon: "", last: false }
                WinPropRow { label: "Modified", value: "Today", icon: "", last: true }
            }
        } else if $state.which == "menu" {
            WinMenu { width: 220 } {
                items {
                    WinMenuRow { label: "File", icon: "", kind: "header", shortcut: "", selected: false }
                    WinMenuItem { id: "new", label: "New window", icon: "plus", kind: "", act: false, shortcut: "Ctrl+N", selected: false }
                    WinMenuItem { id: "grid", label: "As grid", icon: "square.grid.2x2", kind: "check", act: false, shortcut: "", selected: false }
                    WinMenuItem { id: "view", label: "View", icon: "eye", kind: "sub", act: false, shortcut: "", selected: true }
                    WinMenuRow { label: "Export", icon: "arrow.up", kind: "disabled", shortcut: "", selected: false }
                    WinMenuDivider { }
                    WinMenuItem { id: "trash", label: "Delete", icon: "trash", kind: "danger", act: false, shortcut: "", selected: false }
                }
            }
        } else if $state.which == "action_bar" {
            Stack {
                WinActionBar { } {
                    items {
                        Stack { WinActionFace { label: "New", icon: "plus", kind: "", active: true } }
                        on Tap -> dispatch(WinNoop)
                        WinActionItem { id: "share", label: "Share", icon: "square.and.arrow.up", kind: "", active: false }
                        WinActionDivider { }
                        WinActionItem { id: "delete", label: "Delete", icon: "trash", kind: "danger", active: false }
                    }
                }
            }
            .direction(row)
        } else if $state.which == "action_sheet" {
            WinActionSheet { id: "s", title: "Delete 3 items?", message: "This cannot be undone.", cancelLabel: "Cancel" } {
                actions {
                    Stack { WinSheetRow { label: "Move to bin", destructive: false, rule: false } }
                    on Tap -> dispatch(WinNoop)
                    Stack { WinSheetRow { label: "Delete now", destructive: true, rule: true } }
                    on Tap -> dispatch(WinNoop)
                }
            }
        } else if $state.which == "fab" {
            Stack { WinFabFace { icon: "plus" } }
            .label("New")
            on Tap -> dispatch(WinNoop)
        } else {
            Stack { }
        }
    }
    .padding(8)
}
"#;

fn mount<'p>(
    nxir: &'p [u8],
    tokens: &dyn Tokens,
    which: &str,
    states: &[(&str, &str)],
) -> View<'p> {
    let symbols = nexus_dsl_runtime::Runtime::mount(nxir).expect("pre-mount").symbols().to_vec();
    let keys: Vec<u32> = Vec::new();
    let env = FixtureEnv::desktop();
    let mut view = {
        let locale = IdentityLocale { symbols: &symbols, keys: &keys };
        View::mount(nxir, tokens, &env, &locale).expect("mounts")
    };
    let mut send = |case: &str, payload: Vec<Value>| {
        let (e, c) = view.runtime.event_case("E", case).expect("event case");
        let locale = IdentityLocale { symbols: &symbols, keys: &keys };
        view.dispatch(tokens, &env, &locale, &mut nexus_dsl_runtime::NoIo, e, c, payload)
            .expect("dispatch");
    };
    for (case, arg) in states {
        send(case, if arg.is_empty() { vec![] } else { vec![Value::Str((*arg).to_string())] });
    }
    send("Show", vec![Value::Str(which.to_string())]);
    view
}

/// One case: its golden stem, the state events to dispatch first, and the canvas it is drawn on.
struct Case {
    name: &'static str,
    which: &'static str,
    states: &'static [(&'static str, &'static str)],
    size: (i32, i32),
}

const CASES: &[Case] = &[
    Case { name: "top_bar_inactive", which: "top_bar", states: &[], size: (680, 68) },
    Case {
        name: "top_bar_active",
        which: "top_bar",
        states: &[("WinMenu", "app"), ("WinLeft", ""), ("WinRight", "")],
        size: (680, 68),
    },
    Case { name: "button_face", which: "button_face", states: &[], size: (280, 60) },
    Case { name: "app_chip_closed", which: "app_chip", states: &[], size: (96, 46) },
    Case {
        name: "app_chip_open",
        which: "app_chip",
        states: &[("WinMenu", "app")],
        size: (96, 46),
    },
    Case { name: "tool_cells", which: "tool_cells", states: &[], size: (220, 46) },
    Case { name: "pane_panel", which: "pane_panel", states: &[], size: (220, 170) },
    Case { name: "pane_bare", which: "pane_bare", states: &[], size: (220, 170) },
    Case { name: "side", which: "side", states: &[], size: (200, 180) },
    Case { name: "prop_rows", which: "prop_rows", states: &[], size: (220, 130) },
    Case { name: "menu", which: "menu", states: &[], size: (260, 300) },
    Case { name: "action_bar", which: "action_bar", states: &[], size: (300, 90) },
    Case { name: "action_sheet", which: "action_sheet", states: &[], size: (300, 300) },
    Case { name: "fab", which: "fab", states: &[], size: (90, 90) },
];

#[test]
fn kit_primitives_match_their_goldens_light_and_dark() {
    let nxir = compile(&format!("{}\n{APP}", kit()));
    let mut failed = Vec::new();
    for (theme, tokens) in [("light", &LightTokens as &dyn Tokens), ("dark", &DarkTokens)] {
        for case in CASES {
            let view = mount(&nxir, tokens, case.which, case.states);
            let name = format!("kit_{}_{theme}", case.name);
            let (w, h) = case.size;
            if ui_v10_goldens::check_golden_sized(&name, view.scene(), w, h).is_err() {
                failed.push(name);
            }
        }
    }
    assert!(failed.is_empty(), "golden mismatches: {failed:?}");
}
