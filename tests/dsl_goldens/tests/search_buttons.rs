// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! TASK-0067B: pixel goldens of the shell search's round category buttons — normal,
//! selected (the inner accent disc) and inert (dimmed) — light and dark. Regenerate with
//! `UPDATE_GOLDENS=1`.

// reason: test harness — a failed compile/mount step must panic loudly.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use dsl_goldens::compile;
use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, View};
use nexus_theme_tokens::{DarkTokens, LightTokens, Tokens};

const BUTTON: &str =
    include_str!("../../../userspace/apps/desktop-shell/ui/components/search/SearchModeButton.nx");

const APP: &str = r#"
Store S {
    x: Int = 0,
}

Page P {
    Stack {
        SearchModeButton { icon: "square.grid", label: "Apps", selected: false, inert: false }
        SearchModeButton { icon: "doc", label: "Files", selected: false, inert: true }
        SearchModeButton { icon: "doc.on.clipboard", label: "Clipboard", selected: true, inert: false }
    }
    .direction(row)
    .gap(2)
    .padding(2)
}
"#;

fn mount<'p>(nxir: &'p [u8], tokens: &dyn Tokens) -> View<'p> {
    let symbols = nexus_dsl_runtime::Runtime::mount(nxir).expect("pre-mount").symbols().to_vec();
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    View::mount(nxir, tokens, &FixtureEnv::desktop(), &locale).expect("mounts")
}

#[test]
fn search_buttons_match_their_goldens_light_and_dark() {
    let nxir = compile(&format!("{BUTTON}\n{APP}"));
    for (theme, tokens) in [("light", &LightTokens as &dyn Tokens), ("dark", &DarkTokens)] {
        let view = mount(&nxir, tokens);
        ui_v10_goldens::check_golden(&format!("shell_search_buttons_{theme}"), view.scene())
            .unwrap();
    }
}
