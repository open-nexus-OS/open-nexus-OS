// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Host contract tests for `fbdevd` service-owned RAMFB bootstrap and visible-state observation.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `cargo test -p fbdevd -- --nocapture`
//!
//! TEST_SCOPE:
//!   - RAMFB metadata and DMA reject paths
//!   - service-owned framebuffer handoff validation
//!   - observer-side visible state transport and markers
//!
//! TEST_SCENARIOS:
//!   - `test_reject_*`: malformed RAMFB / DMA metadata must fail closed
//!   - `visible_state_*`: observer transport keeps service-owned scanout evidence intact
//!   - `scanout_*`: framebuffer mapping and frame encoding stay bounded
//!
//! DEPENDENCIES:
//!   - `fbdevd::backend::ramfb`: RAMFB file and DMA validation
//!   - `fbdevd::protocol`: visible-state wire helpers
//!   - `fbdevd::service`: service-owned scanout glue
//!
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

use fbdevd::{
    dma_transfer_complete, encode_ramfb_config, encode_ramfb_dma_request, live_dirty_rows,
    require_fw_cfg_signature, validate_dma_capability, validate_framebuffer_cap,
    validate_ramfb_file, DirtyRows, DisplayReactor, DisplayScanout, FbdevService, FbdevdError,
    TickBudget, FLUSH_OK_MARKER, MAP_OK_MARKER, RAMFB_CONFIGURED_MARKER, READY_MARKER,
};
use input_live_protocol::VisibleState;
use pointer_state::{PointerPosition, PointerSpace, PointerTransform};
use std::vec::Vec;
use windowd::{
    bootstrap_display_handoff, live_visible_state_handoff, DisplayFrameSource, Frame,
    VisibleBootstrapMode, VisibleDisplayCapability,
};

fn must_ok<T, E: core::fmt::Debug>(result: Result<T, E>, context: &str) -> T {
    match result {
        Ok(value) => value,
        Err(err) => panic!("{context}: {err:?}"),
    }
}

fn must_some<T>(value: Option<T>, context: &str) -> T {
    match value {
        Some(value) => value,
        None => panic!("{context}"),
    }
}

#[test]
fn test_reject_invalid_ramfb_fw_cfg() {
    assert_eq!(require_fw_cfg_signature(false), Err(FbdevdError::InvalidRamfbFwCfg));
}

#[test]
fn test_reject_ramfb_file_too_small() {
    assert_eq!(validate_ramfb_file(0x24, 27), Err(FbdevdError::RamfbFileTooSmall));
}

#[test]
fn test_reject_invalid_dma_capability() {
    assert_eq!(validate_dma_capability(0, 0x1000, 4096), Err(FbdevdError::DmaCapInvalid));
    assert_eq!(validate_dma_capability(1, 0x1000, 1024), Err(FbdevdError::DmaCapInvalid));
}

#[test]
fn test_reject_invalid_framebuffer_cap() {
    let mode = must_ok(VisibleBootstrapMode::fixed(), "fixed mode");
    let invalid = VisibleDisplayCapability {
        byte_len: must_ok(mode.byte_len(), "mode bytes") - 1,
        mapped: true,
        writable: true,
    };

    assert_eq!(validate_framebuffer_cap(mode, invalid), Err(FbdevdError::InvalidFramebufferCap));
}

#[test]
fn test_reject_present_without_frame() {
    let mode = must_ok(VisibleBootstrapMode::fixed(), "fixed mode");
    let handoff = windowd::DisplayPresentHandoff {
        mode,
        source: DisplayFrameSource::Materialized(Frame {
            width: mode.width,
            height: mode.height,
            stride: mode.stride,
            pixels: Vec::new(),
        }),
        damage_rects: 1,
        backend_visible: true,
        systemui_first_frame_visible: true,
        scanout_ready: true,
        cursor_bitmap: None,
        cursor_width: 0,
        cursor_height: 0,
    };
    let mut scanout = DisplayScanout::new();
    scanout.configure();

    assert_eq!(scanout.present(1, &handoff), Err(FbdevdError::PresentWithoutFrame));
}

#[test]
fn test_reject_flush_without_configured_backend() {
    let handoff = must_ok(bootstrap_display_handoff(), "bootstrap handoff");
    let mut scanout = DisplayScanout::new();

    assert_eq!(scanout.present(1, &handoff), Err(FbdevdError::FlushWithoutConfiguredBackend));
}

#[test]
fn test_reject_stale_scanout_generation() {
    let handoff = must_ok(bootstrap_display_handoff(), "bootstrap handoff");
    let mut scanout = DisplayScanout::new();
    scanout.configure();

    assert_eq!(scanout.present(1, &handoff), Ok(1));
    assert_eq!(scanout.present(1, &handoff), Err(FbdevdError::StaleScanoutGeneration));
}

#[test]
fn bootstrap_service_is_observer_ready_after_first_present() {
    let bootstrap = must_ok(bootstrap_display_handoff(), "bootstrap handoff");
    let service = must_ok(FbdevService::enabled(&bootstrap), "enabled service");

    assert!(service.display_enabled());
    assert!(service.observer_ready());
    assert!(service.visible_state().backend_visible);
    assert!(service.visible_state().display_scanout_ready);
    assert!(service.visible_state().systemui_first_frame_visible);
}

#[test]
fn live_present_contract_keeps_display_bits_while_merging_input_state() {
    let bootstrap = must_ok(bootstrap_display_handoff(), "bootstrap handoff");
    let mut service = must_ok(FbdevService::enabled(&bootstrap), "enabled service");
    service.merge_input_state(VisibleState {
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
        ..VisibleState::default()
    });

    let merged = service.visible_state();
    assert!(merged.backend_visible);
    assert!(merged.display_scanout_ready);
    assert!(merged.systemui_first_frame_visible);
    assert!(merged.scene_ready);
    assert!(merged.keyboard_visible);

    let handoff = must_ok(live_visible_state_handoff(service.render_state()), "live handoff");
    assert_eq!(handoff.mode.width, windowd::VISIBLE_BOOTSTRAP_WIDTH);
    assert_eq!(handoff.mode.height, windowd::VISIBLE_BOOTSTRAP_HEIGHT);
    assert!(must_ok(handoff.byte_len(), "live byte len") > 0);
}

#[test]
fn observer_state_latches_transient_visible_input_bits_without_sticking_render_state() {
    let bootstrap = must_ok(bootstrap_display_handoff(), "bootstrap handoff");
    let mut service = must_ok(FbdevService::enabled(&bootstrap), "enabled service");

    service.merge_input_state(VisibleState {
        input_visible_on: true,
        cursor_move_visible: true,
        hover_visible: true,
        focus_visible: true,
        launcher_click_visible: true,
        keyboard_visible: true,
        wheel_up_visible: true,
        pointer_route_live: true,
        keyboard_route_live: true,
        cursor_x: 8,
        cursor_y: 40,
        ..VisibleState::default()
    });
    service.merge_input_state(VisibleState {
        input_visible_on: false,
        cursor_move_visible: false,
        hover_visible: false,
        focus_visible: false,
        launcher_click_visible: false,
        keyboard_visible: false,
        wheel_up_visible: false,
        wheel_down_visible: false,
        pointer_route_live: true,
        keyboard_route_live: true,
        cursor_x: 8,
        cursor_y: 40,
        ..VisibleState::default()
    });

    let observer = service.visible_state();
    assert!(observer.launcher_click_visible);
    assert!(observer.keyboard_visible);
    assert!(observer.wheel_up_visible);

    let render = service.render_state();
    assert!(!render.launcher_click_visible);
    assert!(!render.keyboard_visible);
    assert!(!render.wheel_up_visible);
}

#[test]
fn observer_state_latches_displayserver_asset_evidence() {
    let bootstrap = must_ok(bootstrap_display_handoff(), "bootstrap handoff");
    let mut service = must_ok(FbdevService::enabled(&bootstrap), "enabled service");

    service.merge_input_state(VisibleState {
        cursor_svg_visible: true,
        text_target_visible: true,
        icon_target_visible: true,
        wallpaper_visible: true,
        cursor_overlay_visible: true,
        ..VisibleState::default()
    });

    let observer = service.visible_state();
    assert!(observer.cursor_svg_visible);
    assert!(observer.text_target_visible);
    assert!(observer.icon_target_visible);
    assert!(observer.wallpaper_visible);
    assert!(observer.cursor_overlay_visible);
}

#[test]
fn telemetry_reports_windowd_and_fbdevd_fps_lines() {
    let bootstrap = must_ok(bootstrap_display_handoff(), "bootstrap handoff");
    let mut service = must_ok(FbdevService::enabled(&bootstrap), "enabled service");
    service.merge_input_state(VisibleState {
        scene_ready: true,
        cursor_x: 8,
        cursor_y: 40,
        ..VisibleState::default()
    });
    let handoff = must_ok(live_visible_state_handoff(service.render_state()), "live handoff");
    assert_eq!(service.present(&handoff), Ok(2));

    assert!(service.telemetry_if_due(1).is_none());
    let (windowd_line, fbdevd_line) =
        must_some(service.telemetry_if_due(1_000_000_001), "fps lines");
    assert!(windowd_line.starts_with("fps: windowd compose_hz="));
    assert!(fbdevd_line.starts_with("fps: fbdevd flush_hz="));
}

#[test]
fn display_reactor_presents_on_cadence_without_overrunning_tick_budget() {
    let mut reactor = DisplayReactor::new(60);
    let mut budget = TickBudget::new(1);

    assert!(reactor.should_present(1, &mut budget));
    assert_eq!(budget.remaining(), 0);
    assert!(!reactor.should_present(20_000_000, &mut budget));

    let mut budget = TickBudget::new(1);
    assert!(!reactor.should_present(16_000_000, &mut budget));

    let mut budget = TickBudget::new(1);
    assert!(reactor.should_present(17_000_000, &mut budget));
}

#[test]
fn live_cursor_only_changes_dirty_the_cursor_rows_instead_of_full_frame() {
    let mode = must_ok(VisibleBootstrapMode::fixed(), "fixed mode");
    let transform = must_ok(
        PointerTransform::new(
            must_ok(PointerSpace::new(mode.width, mode.height), "display"),
            must_ok(PointerSpace::new(64, 48), "route"),
        ),
        "transform",
    );
    let previous_cursor = transform.route_to_display(PointerPosition::new(8, 12));
    let next_cursor = transform.route_to_display(PointerPosition::new(10, 14));
    let extent = transform.display_extent_from_route();
    let previous = VisibleState {
        scene_ready: true,
        backend_visible: true,
        display_scanout_ready: true,
        systemui_first_frame_visible: true,
        cursor_x: previous_cursor.x,
        cursor_y: previous_cursor.y,
        ..VisibleState::default()
    };
    let next = VisibleState { cursor_x: next_cursor.x, cursor_y: next_cursor.y, ..previous };

    assert_eq!(
        live_dirty_rows(previous, next, mode),
        DirtyRows::Range {
            start_y: previous_cursor.y as u32,
            end_y: (next_cursor.y as u32 + extent.height).min(mode.height)
        }
    );
    assert_eq!(live_dirty_rows(previous, previous, mode), DirtyRows::None);
    assert_eq!(
        live_dirty_rows(previous, VisibleState { keyboard_visible: true, ..next }, mode),
        DirtyRows::Full
    );
}

#[test]
fn partial_live_present_accounts_only_dirty_bytes() {
    let bootstrap = must_ok(bootstrap_display_handoff(), "bootstrap handoff");
    let mut service = must_ok(FbdevService::enabled(&bootstrap), "enabled service");
    assert!(service.telemetry_if_due(1).is_none());
    assert!(service.telemetry_if_due(1_000_000_001).is_some());

    assert_eq!(service.present_live_bytes(bootstrap.mode.stride as usize * 4), Ok(2));
    let (_, fbdevd_line) = must_some(service.telemetry_if_due(2_000_000_002), "fps lines");

    assert!(fbdevd_line.contains("bytes=20480"));
}

#[test]
fn dma_descriptor_encoding_matches_fw_cfg_contract() {
    let request = encode_ramfb_dma_request(0x41, 0x0123_4567_89ab_cdef);

    assert_eq!(&request[0..4], &((0x41u32 << 16) | (1 << 3) | (1 << 4)).to_be_bytes());
    assert_eq!(&request[4..8], &28u32.to_be_bytes());
    assert_eq!(&request[8..16], &0x0123_4567_89ab_cdefu64.to_be_bytes());
}

#[test]
fn dma_control_error_maps_to_stable_failure_gate() {
    assert_eq!(dma_transfer_complete(0), Ok(true));
    assert_eq!(dma_transfer_complete(1 << 3), Ok(false));
    assert_eq!(dma_transfer_complete(1), Err(FbdevdError::DmaDeviceError));
}

#[test]
fn ramfb_config_and_markers_match_service_owned_contract() {
    let mode = must_ok(VisibleBootstrapMode::fixed(), "fixed mode");
    let config = encode_ramfb_config(0x1234_5000, mode);

    assert_eq!(&config[0..8], &0x1234_5000u64.to_be_bytes());
    assert_eq!(READY_MARKER, "fbdevd: ready");
    assert_eq!(MAP_OK_MARKER, "fbdevd: map ok");
    assert_eq!(RAMFB_CONFIGURED_MARKER, "fbdevd: ramfb configured");
    assert_eq!(FLUSH_OK_MARKER, "fbdevd: flush ok");
    assert_eq!(FbdevdError::DmaMapPage.label(), "fbdevd: fail dma-map-page");
    assert_eq!(FbdevdError::DmaTimeout.label(), "fbdevd: fail dma-timeout");
}
