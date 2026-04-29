// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `windowd` headless surface/layer/present authority for TASK-0055.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 3 unit tests, integration coverage in `ui_windowd_host`/`launcher`/`selftest-client`
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
    marker_postflight_ready, present_marker, LAUNCHER_MARKER, READY_MARKER,
    SELFTEST_LAUNCHER_PRESENT_MARKER, SELFTEST_RESIZE_MARKER, SYSTEMUI_MARKER,
};
pub use server::{InputStubStatus, PresentAck, UiProfile, WindowServer, WindowdConfig};
pub use smoke::{run_headless_ui_smoke, UiSmokeEvidence};

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
}
