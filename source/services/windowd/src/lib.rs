// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `windowd` surface/layer/present/input authority for headless, visible, and v2a UI proofs.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests plus integration coverage in `ui_windowd_host`, `ui_v2a_host`, `launcher`, and `selftest-client`
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

#![cfg_attr(all(nexus_env = "os", target_os = "none"), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

mod assets;
mod buffer;
mod cli;
#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
mod compositor;
mod display_backend;
mod error;
#[cfg(any(test, all(feature = "os-lite", nexus_env = "os", target_os = "none")))]
mod fixed_sdf;
mod frame;
mod geometry;
mod ids;
mod layout_panel;
mod legacy;
#[cfg(any(test, target_os = "none"))]
mod live_runtime;
mod markers;
mod proof_panel_spec;
#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
mod render_assets;
#[cfg(any(test, target_os = "none"))]
mod resource_pool;
#[cfg(any(test, target_os = "none"))]
mod scene_graph;
mod server;
mod smoke;
#[cfg(any(test, target_os = "none"))]
mod systemui_shell;
mod telemetry;
mod visible_state;

pub use assets::{
    proof_text_asset, ProofTextAsset, CURSOR_HOTSPOT_X, CURSOR_HOTSPOT_Y, CURSOR_LEFT_PTR_BGRA,
    CURSOR_LEFT_PTR_HEIGHT, CURSOR_LEFT_PTR_SVG, CURSOR_LEFT_PTR_WIDTH,
};
pub use buffer::{PixelFormat, SurfaceBuffer, VmoHandle, VmoRights};
pub use cli::{execute, help};
pub use display_backend::{
    bootstrap_display_handoff, live_visible_state_handoff, DisplayFrameSource,
    DisplayPresentHandoff,
};
pub use error::{Result, WindowdError};
pub use frame::{Frame, Layer};
pub use geometry::Rect;
pub use ids::{
    CallerCtx, CallerId, CommitSeq, FenceId, FrameIndex, InputSeq, PresentSeq, SurfaceId,
    VmoHandleId,
};
pub use layout_panel::{
    build_combined_tree, build_filter_panel_tree, build_proof_panel_tree, compute_proof_layout,
    filter_scrollbar_strip_x, filter_scrollbar_thumb_bounds, filter_scrollbar_track_x,
    ProofTextMeasure, FILTER_LIST_PADDING, FILTER_SCROLLBAR_GUTTER, FILTER_SCROLLBAR_MIN_THUMB,
    FILTER_SCROLLBAR_WIDTH,
};
pub use legacy::render_frame;
pub use markers::{
    damage_rects_marker,
    focus_marker,
    marker_postflight_ready,
    present_marker,
    CLICK_LATENCY_OK_MARKER,
    // TASK-0059 markers
    CLIPPING_ON_MARKER,
    COMPOSE_READY_MARKER,
    CURSOR_MOVE_VISIBLE_MARKER,
    CURSOR_SVG_LOADED_MARKER,
    DISPLAY_BOOTSTRAP_MARKER,
    DISPLAY_FIRST_SCANOUT_MARKER,
    DISPLAY_MODE_MARKER,
    EFFECTS_ON_MARKER,
    EFFECT_BLUR_OK_MARKER,
    FAIL_COMPOSE_EVIDENCE_MARKER,
    FAIL_PRESENT_STALL_MARKER,
    FILTER_LIST_OK_MARKER,
    FOCUS_VISIBLE_MARKER,
    FULL_WINDOW_VISIBLE_MARKER,
    HOVER_VISIBLE_MARKER,
    ICON_TARGET_VISIBLE_MARKER,
    IDLE_FASTPATH_OK_MARKER,
    INPUT_ON_MARKER,
    INPUT_VISIBLE_ON_MARKER,
    INTERACTIVE_CLICK_TARGET_READY_MARKER,
    INTERACTIVE_FULL_MARKERS_MARKER,
    INTERACTIVE_KEYBOARD_TARGET_READY_MARKER,
    INTERACTIVE_SCENE_READY_MARKER,
    KEYBOARD_LATENCY_OK_MARKER,
    KEYBOARD_VISIBLE_MARKER,
    LAUNCHER_CLICK_OK_MARKER,
    LAUNCHER_CLICK_VISIBLE_OK_MARKER,
    LAUNCHER_MARKER,
    LAYOUT_ENGINE_ON_MARKER,
    LIVE_SCROLL_OK_MARKER,
    NO_DAMAGE_SKIP_OK_MARKER,
    POINTER_COALESCE_OK_MARKER,
    PRESENT_COALESCED_MARKER,
    PRESENT_FASTPATH_MARKER,
    PRESENT_QUEUED_MARKER,
    PRESENT_SCHEDULER_ON_MARKER,
    PRESENT_VISIBLE_MARKER,
    READY_MARKER,
    SCROLL_ON_MARKER,
    SELFTEST_DISPLAY_BOOTSTRAP_VISIBLE_MARKER,
    SELFTEST_LAUNCHER_PRESENT_MARKER,
    SELFTEST_RESIZE_MARKER,
    SELFTEST_UI_V2B_ASSETS_OK_MARKER,
    SELFTEST_UI_V2_INPUT_OK_MARKER,
    SELFTEST_UI_V2_PRESENT_OK_MARKER,
    SELFTEST_UI_V3_EFFECT_OK_MARKER,
    SELFTEST_UI_V3_FILTER_OK_MARKER,
    SELFTEST_UI_V3_IME_OK_MARKER,
    SELFTEST_UI_V3_SCROLL_OK_MARKER,
    SELFTEST_UI_VISIBLE_INPUT_OK_MARKER,
    SELFTEST_UI_VISIBLE_PRESENT_MARKER,
    SELFTEST_UI_VISIBLE_WHEEL_OK_MARKER,
    SYSTEMUI_FIRST_FRAME_VISIBLE_MARKER,
    SYSTEMUI_MARKER,
    TEXT_INPUT_ON_MARKER,
    TEXT_TARGET_VISIBLE_MARKER,
    TEXT_WRAPPING_ON_MARKER,
    VISIBLE_BACKEND_MARKER,
    WALLPAPER_VISIBLE_MARKER,
    WHEEL_VISIBLE_MARKER,
};
pub use proof_panel_spec::{
    filter_words, ProofTextSpec, ALL_TEXT_SPECS, FILTER_WORDS, TOKEN_CARD_ACTIVE_BG, TOKEN_CARD_BG,
    TOKEN_CARD_BORDER, TOKEN_CLICK, TOKEN_GLASS_EDGE, TOKEN_GLASS_TINT, TOKEN_HOVER, TOKEN_ICON_BG,
    TOKEN_ICON_FG, TOKEN_KEYBOARD, TOKEN_PANEL_BG, TOKEN_PANEL_BORDER, TOKEN_SCROLL,
};
pub use server::{
    BackBufferLease, InputDelivery, InputEventKind, InputStubStatus, PointerPosition, PresentAck,
    PresentFenceStatus, PresentFrameAck, ScheduledPresentAck, TouchInputPhase, UiProfile,
    WindowServer, WindowdConfig, VISIBLE_BOOTSTRAP_FORMAT, VISIBLE_BOOTSTRAP_HEIGHT,
    VISIBLE_BOOTSTRAP_HZ, VISIBLE_BOOTSTRAP_WIDTH, VISIBLE_CURSOR_BGRA, VISIBLE_FOCUS_BGRA,
    VISIBLE_HOVER_BGRA,
};
pub use smoke::{
    bootstrap_pixel_bgra, run_headless_ui_smoke, run_ui_v2a_smoke, run_visible_bootstrap_smoke,
    run_visible_input_smoke, run_visible_systemui_smoke, v2a_marker_postflight_ready,
    validate_visible_bootstrap_capability, visible_input_marker_postflight_ready,
    visible_marker_postflight_ready, visible_systemui_marker_postflight_ready, UiSmokeEvidence,
    UiV2aEvidence, UiVisibleInputEvidence, VisibleBootstrapEvidence, VisibleBootstrapMode,
    VisibleDisplayCapability, VisibleSystemUiEvidence, VISIBLE_INPUT_CLICK_BGRA,
    VISIBLE_INPUT_KEYBOARD_BGRA,
};
pub use telemetry::{WindowdDisplayTelemetry, WindowdDisplayTelemetryReport};
pub use visible_state::{
    compose_live_visible_frame, copy_live_visible_row, VISIBLE_INPUT_WHEEL_ACTIVE_BGRA,
    VISIBLE_INPUT_WHEEL_IDLE_BGRA,
};

#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
pub use compositor::service_main_loop;

#[cfg(not(all(nexus_env = "os", target_os = "none")))]
pub use cli::run;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_string_present() {
        assert!(execute(&["--help"])[0].contains("windowd"));
    }

    #[test]
    fn smoke_markers_require_real_present() {
        let lines = execute(&[]);
        assert_eq!(lines[0], READY_MARKER);
        assert!(lines.iter().any(|line| line == "windowd: present ok (seq=1 dmg=1)"));
        assert!(lines.contains(&String::from(SELFTEST_RESIZE_MARKER)));
    }

    #[test]
    fn marker_postflight_rejects_missing_present() {
        assert_eq!(marker_postflight_ready(None), Err(WindowdError::MarkerBeforePresentState));
    }

    #[test]
    fn visible_bootstrap_smoke_requires_present_before_marker() {
        let evidence = run_visible_bootstrap_smoke().expect("visible bootstrap smoke");
        assert!(evidence.ready);
        assert_eq!(evidence.mode.width, VISIBLE_BOOTSTRAP_WIDTH);
        assert_eq!(evidence.mode.height, VISIBLE_BOOTSTRAP_HEIGHT);
        assert_eq!(evidence.first_present.seq.raw(), 1);
        assert_eq!(evidence.seed_surface.width, 64);
        assert_eq!(evidence.seed_surface.height, 48);
        assert!(visible_marker_postflight_ready(Some(evidence)).is_ok());
        assert_eq!(
            visible_marker_postflight_ready(None),
            Err(WindowdError::MarkerBeforePresentState)
        );
    }

    #[test]
    fn visible_systemui_smoke_requires_present_before_marker() {
        let evidence = run_visible_systemui_smoke().expect("visible systemui smoke");
        assert!(evidence.ready);
        assert!(evidence.backend_visible);
        assert!(evidence.systemui_first_frame);
        assert_eq!(evidence.first_present.seq.raw(), 1);
        assert_eq!(evidence.frame_source.width, VISIBLE_BOOTSTRAP_WIDTH);
        assert_eq!(evidence.frame_source.height, VISIBLE_BOOTSTRAP_HEIGHT);
        assert!(visible_systemui_marker_postflight_ready(Some(evidence)).is_ok());
        assert_eq!(
            visible_systemui_marker_postflight_ready(None),
            Err(WindowdError::MarkerBeforePresentState)
        );
    }

    #[test]
    fn visible_bootstrap_rejects_invalid_mode_and_capability() {
        let mode = VisibleBootstrapMode::fixed().expect("fixed mode");
        assert_eq!(
            VisibleBootstrapMode { width: 1024, ..mode }.validate(),
            Err(WindowdError::InvalidDimensions)
        );
        assert_eq!(
            VisibleBootstrapMode { stride: mode.stride - 4, ..mode }.validate(),
            Err(WindowdError::InvalidStride)
        );
        assert_eq!(
            VisibleBootstrapMode { format: PixelFormat::Unsupported(1), ..mode }.validate(),
            Err(WindowdError::UnsupportedFormat)
        );
        assert_eq!(
            validate_visible_bootstrap_capability(
                mode,
                VisibleDisplayCapability {
                    byte_len: mode.byte_len().expect("mode bytes") - 1,
                    mapped: true,
                    writable: true,
                },
            ),
            Err(WindowdError::InvalidDisplayCapability)
        );
    }

    #[test]
    fn visible_input_smoke_requires_routed_visible_state() {
        let evidence = run_visible_input_smoke().expect("visible input smoke");
        assert!(evidence.input_visible_on);
        assert!(evidence.full_window_visible);
        assert!(evidence.cursor_move_visible);
        assert!(evidence.hover_visible);
        assert!(evidence.focus_visible);
        assert!(evidence.launcher_click_visible);
        assert!(evidence.keyboard_visible);
        assert_eq!(evidence.focused_surface.raw(), 1);
        assert!(visible_input_marker_postflight_ready(Some(evidence)).is_ok());
        assert_eq!(
            visible_input_marker_postflight_ready(None),
            Err(WindowdError::MarkerBeforePresentState)
        );
    }
}
