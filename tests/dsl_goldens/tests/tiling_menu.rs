// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! TASK-0066 P4: pixel goldens of the kit's window menu with its tile glyph rows (`WinAppMenu`
//! + `WinTileGlyph`) — light and dark. Regenerate with `UPDATE_GOLDENS=1`.

// reason: test harness — a failed compile/mount step must panic loudly.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use dsl_goldens::compile;
use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, View};
use nexus_theme_tokens::{DarkTokens, LightTokens, Tokens};

const MENU: &str = include_str!("../../../userspace/apps/window-kit/ui/components/WinAppMenu.nx");
const GLYPH: &str =
    include_str!("../../../userspace/apps/window-kit/ui/components/WinTileGlyph.nx");
const ITEM: &str = include_str!("../../../userspace/apps/window-kit/ui/components/WinMenuItem.nx");

const APP: &str = r#"
Store S {
    x: Int = 0,
}

Event E {
    WinAct(Str),
    WinMenuPick(Str),
    WinNoop,
}

reduce E {
    WinAct(a) => state.x = state.x,
    WinMenuPick(id) => state.x = state.x,
    WinNoop => state.x = state.x,
}

Page P {
    Stack {
        WinAppMenu {
            fullscreen: "Full screen",
            moveDevice: "Move to another device",
            minimize: "Minimise",
            close: "Close",
        }
    }
    .padding(4)
}
"#;

fn mount<'p>(nxir: &'p [u8], tokens: &dyn Tokens) -> View<'p> {
    let symbols = nexus_dsl_runtime::Runtime::mount(nxir).expect("pre-mount").symbols().to_vec();
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    View::mount(nxir, tokens, &FixtureEnv::desktop(), &locale).expect("mounts")
}

#[test]
fn window_menu_with_tile_glyphs_matches_its_goldens_light_and_dark() {
    let nxir = compile(&format!("{GLYPH}\n{ITEM}\n{MENU}\n{APP}"));
    for (theme, tokens) in [("light", &LightTokens as &dyn Tokens), ("dark", &DarkTokens)] {
        let view = mount(&nxir, tokens);
        ui_v10_goldens::check_golden(&format!("kit_window_menu_{theme}"), view.scene()).unwrap();
    }
}
