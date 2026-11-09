// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The app's window size applied by the window manager (RFC-0117): the window's
//! minimum (the app's and the WM's, never beyond the work area), the opening size of a freeform
//! window, and the clamp every resize, tile and restore goes through. Pure, host-tested
//! (`tests/window_size.rs`); the runtime calls it from the intent reply, the create and the one
//! frame-apply path.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `tests/window_size.rs`
//!
//! The app's sizes arrive as `u16` from the wire and are untrusted: every result here is bounded
//! by the work area, so no app can make windowd size a window beyond the display.

/// A size in px: `(width, height)`.
pub type Size = (u32, u32);

/// The window's minimum content size: per axis the larger of the app's minimum (`0` = absent)
/// and the window manager's own, but never more than the work area.
#[must_use]
pub fn minimum(app_min: (u16, u16), wm_min: Size, work: Size) -> Size {
    (u32::from(app_min.0).max(wm_min.0).min(work.0), u32::from(app_min.1).max(wm_min.1).min(work.1))
}

/// The opening size of a freeform window: per axis the app's default (`0` = absent → the window
/// manager's default), clamped to `[minimum, work area]`.
#[must_use]
pub fn opening(app_default: (u16, u16), wm_default: Size, min: Size, work: Size) -> Size {
    let pick = |app: u16, wm: u32| if app == 0 { wm } else { u32::from(app) };
    clamp((pick(app_default.0, wm_default.0), pick(app_default.1, wm_default.1)), min, work)
}

/// A size the window manager applies (a resize, a tile zone, a restore): never below the
/// minimum, never beyond the work area. A tile zone smaller than the minimum gives the window
/// its minimum on that axis.
#[must_use]
pub fn clamp(size: Size, min: Size, work: Size) -> Size {
    // `min` is already bounded by the work area (`minimum`), so the order cannot invert.
    (size.0.max(min.0).min(work.0), size.1.max(min.1).min(work.1))
}
