// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#[path = "../src/runtime_mode.rs"]
mod runtime_mode;

use runtime_mode::{parse_runtime_mode, parse_runtime_profile, RuntimeMode, RuntimeProfile};

#[test]
fn runtime_mode_parser_accepts_all_supported_tokens() {
    assert_eq!(parse_runtime_mode(b"proof"), Some(RuntimeMode::Proof));
    assert_eq!(
        parse_runtime_mode(b"interactive-minimal\n"),
        Some(RuntimeMode::InteractiveMinimal)
    );
    assert_eq!(
        parse_runtime_mode(b" interactive-full\r"),
        Some(RuntimeMode::InteractiveFull)
    );
}

#[test]
fn runtime_mode_parser_rejects_unknown_tokens() {
    assert_eq!(parse_runtime_mode(b""), None);
    assert_eq!(parse_runtime_mode(b"interactive"), None);
    assert_eq!(parse_runtime_mode(b"proof-ish"), None);
}

#[test]
fn runtime_profile_parser_accepts_supported_tokens() {
    assert_eq!(parse_runtime_profile(b"full"), Some(RuntimeProfile::Full));
    assert_eq!(parse_runtime_profile(b"bringup"), Some(RuntimeProfile::Bringup));
    assert_eq!(parse_runtime_profile(b"quick"), Some(RuntimeProfile::Quick));
    assert_eq!(parse_runtime_profile(b"ota"), Some(RuntimeProfile::Ota));
    assert_eq!(parse_runtime_profile(b"net"), Some(RuntimeProfile::Net));
    assert_eq!(parse_runtime_profile(b"none"), Some(RuntimeProfile::None));
}
