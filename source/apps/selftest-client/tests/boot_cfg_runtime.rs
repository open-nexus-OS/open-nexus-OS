// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#[path = "../src/os_lite/display_observer.rs"]
mod display_observer;
#[path = "../src/runtime_mode.rs"]
mod runtime_mode;

use input_live_protocol::VisibleState;
use runtime_mode::{parse_runtime_mode, parse_runtime_profile, RuntimeMode, RuntimeProfile};
use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("source/apps")
        .parent()
        .expect("source")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

#[test]
fn runtime_mode_parser_accepts_all_supported_tokens() {
    assert_eq!(parse_runtime_mode(b"proof"), Some(RuntimeMode::Proof));
    assert_eq!(parse_runtime_mode(b"interactive-minimal\n"), Some(RuntimeMode::InteractiveMinimal));
    assert_eq!(parse_runtime_mode(b" interactive-full\r"), Some(RuntimeMode::InteractiveFull));
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

#[test]
fn display_observer_requires_service_owned_display_evidence() {
    let mut state = VisibleState::default();
    assert!(!display_observer::display_bootstrap_ready(state));

    state.backend_visible = true;
    state.display_scanout_ready = true;
    state.systemui_first_frame_visible = true;
    assert!(display_observer::display_bootstrap_ready(state));
}

#[test]
fn proof_visible_input_ready_requires_full_live_chain() {
    let state = VisibleState {
        virtio_raw_seen: true,
        hid_normalized_seen: true,
        backend_visible: true,
        display_scanout_ready: true,
        systemui_first_frame_visible: true,
        scene_ready: true,
        full_window_visible: true,
        click_target_visible: true,
        keyboard_target_visible: true,
        input_visible_on: true,
        cursor_move_visible: true,
        hover_visible: true,
        focus_visible: true,
        launcher_click_visible: true,
        keyboard_visible: true,
        pointer_route_live: true,
        keyboard_route_live: true,
        cursor_x: 8,
        cursor_y: 40,
    };
    assert!(display_observer::proof_visible_input_ready(state));
    assert!(display_observer::interactive_scene_ready(state));

    let mut missing_keyboard = state;
    missing_keyboard.keyboard_visible = false;
    assert!(!display_observer::proof_visible_input_ready(missing_keyboard));
}

#[test]
fn test_reject_observer_display_authority() {
    let display = fs::read_to_string(
        repo_root().join("source/apps/selftest-client/src/os_lite/display_bootstrap_observer.rs"),
    )
    .expect("read display bootstrap");

    assert!(
        display.contains("route_with_retry(\"fbdevd\")")
            && display.contains("fetch_live_visible_state")
            && display.contains("interactive_live_tick"),
        "observer-only bootstrap must read service-owned visible state from fbdevd"
    );
    assert!(
        !display.contains("vmo_write(")
            && !display.contains("configure_ramfb(")
            && !display.contains("write_handoff("),
        "observer-only bootstrap must reject final scanout authority"
    );
}
