// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

mod common;

use nexus_dsl_runtime::{FixtureEnv, IdentityLocale, NoIo, View};

/// Send appends the draft to the transcript and the echo effect answers —
/// both bubbles render in the scene.
#[test]
fn chat_send_appends_and_echoes() {
    let nxir = common::compile("chat");
    let symbols = common::program_symbols(&nxir);
    let tokens = nexus_theme_tokens::BaseTokens;
    let device = FixtureEnv::tablet("landscape");
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    let mut host = NoIo;

    common::dispatch(&mut view, &device, &mut host, &symbols, "ChatEvent", "Send", vec![]);
    let texts = common::scene_texts(&view);
    assert!(
        texts.iter().any(|t| t.starts_with("Du:")),
        "sent bubble missing: {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.starts_with("Echo:")),
        "echo bubble missing: {texts:?}"
    );
}
