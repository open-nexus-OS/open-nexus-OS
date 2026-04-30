// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `windowd` surface/layer/present authority for headless and visible UI proofs.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests, integration coverage in `ui_windowd_host`/`launcher`/`selftest-client`
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

#![cfg_attr(all(nexus_env = "os", target_os = "none"), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

mod buffer;
mod cli;
mod error;
mod frame;
mod geometry;
mod ids;
mod legacy;
mod markers;
mod server;
mod smoke;

pub use buffer::{PixelFormat, SurfaceBuffer, VmoHandle, VmoRights};
pub use cli::{execute, help};
pub use error::{Result, WindowdError};
pub use frame::{Frame, Layer};
pub use geometry::Rect;
pub use ids::{CallerCtx, CallerId, CommitSeq, PresentSeq, SurfaceId, VmoHandleId};
pub use legacy::render_frame;
pub use markers::{
    marker_postflight_ready, present_marker, DISPLAY_BOOTSTRAP_MARKER,
    DISPLAY_FIRST_SCANOUT_MARKER, DISPLAY_MODE_MARKER, LAUNCHER_MARKER, PRESENT_VISIBLE_MARKER,
    READY_MARKER, SELFTEST_DISPLAY_BOOTSTRAP_VISIBLE_MARKER, SELFTEST_LAUNCHER_PRESENT_MARKER,
    SELFTEST_RESIZE_MARKER, SELFTEST_UI_VISIBLE_PRESENT_MARKER,
    SYSTEMUI_FIRST_FRAME_VISIBLE_MARKER, SYSTEMUI_MARKER, VISIBLE_BACKEND_MARKER,
};
pub use server::{
    InputStubStatus, PresentAck, UiProfile, WindowServer, WindowdConfig, VISIBLE_BOOTSTRAP_FORMAT,
    VISIBLE_BOOTSTRAP_HEIGHT, VISIBLE_BOOTSTRAP_HZ, VISIBLE_BOOTSTRAP_WIDTH,
};
pub use smoke::{
    bootstrap_pixel_bgra, run_headless_ui_smoke, run_visible_bootstrap_smoke,
    run_visible_systemui_smoke, validate_visible_bootstrap_capability,
    visible_marker_postflight_ready, visible_systemui_marker_postflight_ready, UiSmokeEvidence,
    VisibleBootstrapEvidence, VisibleBootstrapMode, VisibleDisplayCapability,
    VisibleSystemUiEvidence,
};

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
        assert_eq!(evidence.frame_source.width, 160);
        assert_eq!(evidence.frame_source.height, 100);
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
}
