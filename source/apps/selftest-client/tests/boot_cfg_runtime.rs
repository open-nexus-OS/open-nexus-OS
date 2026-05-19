// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Host tests for `selftest-client` runtime mode/profile parsing and visible-input proof predicates.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: runtime mode/profile parsing plus visible-input proof witness checks.
//!
//! TEST_SCOPE:
//!   - runtime mode/profile parsing from boot-config bytes
//!   - service-owned display bootstrap readiness predicates
//!   - visible-input proof witness accumulation for transient hold/wheel states
//!
//! TEST_SCENARIOS:
//!   - `runtime_mode_parser_*`: accepted and rejected runtime mode/profile tokens
//!   - `display_observer_requires_service_owned_display_evidence()`: bootstrap readiness floor
//!   - `proof_visible_input_*()`: full live chain and transient witness accumulation
//!
//! DEPENDENCIES:
//!   - `display_observer.rs`: observer-side proof predicates
//!   - `runtime_mode.rs`: runtime mode/profile parsing
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

#[path = "../src/os_lite/display_observer.rs"]
mod display_observer;
#[path = "../src/runtime_mode.rs"]
mod runtime_mode;

use input_live_protocol::VisibleState;
use runtime_mode::{parse_runtime_mode, parse_runtime_profile, RuntimeMode, RuntimeProfile};
use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let source_apps = match base.parent() {
        Some(path) => path,
        None => panic!("source/apps"),
    };
    let source = match source_apps.parent() {
        Some(path) => path,
        None => panic!("source"),
    };
    let root = match source.parent() {
        Some(path) => path,
        None => panic!("repo root"),
    };
    root.to_path_buf()
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
        wheel_up_visible: true,
        wheel_down_visible: false,
        pointer_route_live: true,
        keyboard_route_live: true,
        cursor_svg_visible: true,
        text_target_visible: true,
        icon_target_visible: true,
        wallpaper_visible: true,
        cursor_overlay_visible: true,
        cursor_x: 8,
        cursor_y: 40,
        ..VisibleState::default()
    };
    assert!(display_observer::proof_visible_input_ready(state));
    assert!(display_observer::proof_v2b_assets_ready(state));
    assert!(display_observer::interactive_scene_ready(state));

    let mut missing_keyboard = state;
    missing_keyboard.keyboard_visible = false;
    assert!(!display_observer::proof_visible_input_ready(missing_keyboard));

    let mut missing_cursor_asset = state;
    missing_cursor_asset.cursor_svg_visible = false;
    assert!(!display_observer::proof_v2b_assets_ready(missing_cursor_asset));

    let mut missing_wheel = state;
    missing_wheel.wheel_up_visible = false;
    assert!(!display_observer::proof_visible_input_ready(missing_wheel));
}

#[test]
fn proof_visible_input_witness_latches_transient_hold_and_wheel_bits() {
    let base = VisibleState {
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
        launcher_click_visible: false,
        keyboard_visible: false,
        wheel_up_visible: false,
        wheel_down_visible: false,
        pointer_route_live: true,
        keyboard_route_live: true,
        cursor_svg_visible: true,
        text_target_visible: true,
        icon_target_visible: true,
        wallpaper_visible: true,
        cursor_overlay_visible: true,
        cursor_x: 8,
        cursor_y: 40,
        ..VisibleState::default()
    };
    let click_state = VisibleState { launcher_click_visible: true, ..base };
    let keyboard_state = VisibleState { keyboard_visible: true, ..base };
    let wheel_state = VisibleState { wheel_up_visible: true, ..base };
    let mut witness = display_observer::ProofVisibleInputWitness::new();

    witness.observe(click_state);
    assert!(!witness.ready(), "click alone must not satisfy the proof");

    witness.observe(keyboard_state);
    assert!(
        !witness.ready(),
        "click and keyboard still need a visible wheel pulse to satisfy the proof"
    );

    witness.observe(wheel_state);
    assert!(
        witness.ready(),
        "sequential transient click/key/wheel samples must accumulate into one proof witness"
    );
    assert!(display_observer::proof_visible_input_ready(witness.observed_state()));
}

#[test]
fn emit_missing_visible_input_bits_accepts_partial_state_on_host() {
    display_observer::emit_missing_visible_input_bits(VisibleState::default());
}

#[test]
fn test_reject_observer_display_authority() {
    let display_path =
        repo_root().join("source/apps/selftest-client/src/os_lite/display_bootstrap_observer.rs");
    let display = fs::read_to_string(&display_path)
        .unwrap_or_else(|err| panic!("read {}: {err}", display_path.display()));

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
