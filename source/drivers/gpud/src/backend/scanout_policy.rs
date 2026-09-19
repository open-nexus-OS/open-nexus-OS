// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The ONE decision "which scanout path may this device get", pure
//! and host-tested (TASK-0324 P0). A GL device (`virtio-gpu-gl`, it offers
//! VIRTIO_GPU_F_VIRGL) is only ever driven through the GL render target: the
//! host's GL display backends blit the scanout texture from row 0 and ignore
//! the SET_SCANOUT y-offset, so the 2D plane-row scanout (rows 1600..2399 of
//! the tall VMO) shows BLACK there while every marker stays true. A driver
//! that cannot do GL on a GL device therefore fails loudly instead of
//! falling back. Non-GL devices (`virtio-gpu-device`) keep the 2D path —
//! it is correct there and it is the only path.
//! OWNERS: @gpu
//! STATUS: Functional
//! TEST_COVERAGE: unit tests below (`test_reject_*`)

/// The scanout path a device gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanoutPath {
    /// GL render target at row 0, presented by the GPU (virgl).
    GlRenderTarget,
    /// 2D resource scanning out the display plane row window (non-GL device).
    PlaneRow2d,
}

/// Why a device cannot be driven at all (the service exits, loudly).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanoutFatal {
    /// GL device, driver compiled without the `virgl` feature.
    GlDeviceWithoutVirglFeature,
    /// GL device, but the GL draw self-test did not pass (no render target).
    GlDrawUnavailable,
}

/// Decide the scanout path. `gl_device` = the device offered
/// VIRTIO_GPU_F_VIRGL; `virgl_draw_ok` = the boot draw self-test verified a
/// GPU draw by readback; `has_virgl_feature` = `cfg!(feature = "virgl")`.
pub const fn scanout_path(
    gl_device: bool,
    virgl_draw_ok: bool,
    has_virgl_feature: bool,
) -> Result<ScanoutPath, ScanoutFatal> {
    if !gl_device {
        return Ok(ScanoutPath::PlaneRow2d);
    }
    if !has_virgl_feature {
        return Err(ScanoutFatal::GlDeviceWithoutVirglFeature);
    }
    if !virgl_draw_ok {
        return Err(ScanoutFatal::GlDrawUnavailable);
    }
    Ok(ScanoutPath::GlRenderTarget)
}

/// When the GL draw capability may be decided, and what the answer means.
///
/// The scanout policy above needs `virgl_draw_ok` as if it were a property of
/// the DEVICE. It is not: it is the outcome of a self-test, and a self-test run
/// before the host window is realized proves nothing either way. Conflating
/// "not proven yet" with "cannot" is what killed gpud roughly one `just start`
/// in four (TASK-0326).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawVerdict {
    /// Nothing to decide: the device is not GL, or the draw is already proven.
    Settled,
    /// Not proven, and not knowable yet — decide when GL is first needed.
    DeferToFirstNeed,
    /// Not proven at the moment GL is needed. The device cannot do it.
    Fatal,
}

/// Decide what an unproven GL draw means at this moment.
///
/// `needed_now` is the whole rule: at start-up it is false (nobody has asked
/// for a scanout yet and the display may not be realized), at the scanout
/// attach it is true (the display is live by construction). There is no clock
/// and no retry budget here on purpose — the moment is an event, not a
/// duration (RFC-0093 §7).
pub const fn draw_verdict(gl_device: bool, draw_ok: bool, needed_now: bool) -> DrawVerdict {
    if !gl_device || draw_ok {
        return DrawVerdict::Settled;
    }
    if needed_now {
        DrawVerdict::Fatal
    } else {
        DrawVerdict::DeferToFirstNeed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gl_device_with_working_virgl_gets_the_render_target() {
        assert_eq!(scanout_path(true, true, true), Ok(ScanoutPath::GlRenderTarget));
    }

    #[test]
    fn non_gl_device_keeps_the_2d_plane_row_path() {
        assert_eq!(scanout_path(false, false, false), Ok(ScanoutPath::PlaneRow2d));
        assert_eq!(scanout_path(false, false, true), Ok(ScanoutPath::PlaneRow2d));
    }

    #[test]
    fn test_reject_virgl_device_without_feature() {
        // The 2026-09-09 black screen: a bundle built without `virgl` on a
        // `virtio-gpu-gl` device. Never a silent 2D fallback again.
        assert_eq!(
            scanout_path(true, false, false),
            Err(ScanoutFatal::GlDeviceWithoutVirglFeature)
        );
    }

    #[test]
    fn test_reject_gl_init_failure_falls_to_2d() {
        // A GL device whose draw self-test failed must not be scanned out 2D.
        assert_eq!(scanout_path(true, false, true), Err(ScanoutFatal::GlDrawUnavailable));
    }

    /// The defect this task fixes: an unproven draw at START-UP used to take the
    /// fatal branch, and on a backend whose window is realized asynchronously
    /// that is the normal state for a moment.
    #[test]
    fn test_reject_fatal_before_display_is_live() {
        assert_eq!(draw_verdict(true, false, false), DrawVerdict::DeferToFirstNeed);
    }

    /// And the half that must NOT soften: once GL is needed, an unproven draw
    /// is the device, and the service dies rather than showing a black screen.
    #[test]
    fn test_reject_soft_failure_when_gl_is_needed() {
        assert_eq!(draw_verdict(true, false, true), DrawVerdict::Fatal);
    }

    #[test]
    fn nothing_to_decide_when_settled() {
        // Proven, at either moment.
        assert_eq!(draw_verdict(true, true, false), DrawVerdict::Settled);
        assert_eq!(draw_verdict(true, true, true), DrawVerdict::Settled);
        // Not a GL device: the 2D path is correct and nothing is deferred.
        assert_eq!(draw_verdict(false, false, false), DrawVerdict::Settled);
        assert_eq!(draw_verdict(false, false, true), DrawVerdict::Settled);
    }

    /// Deferring must never widen the path: a GL device with an unproven draw
    /// still cannot be answered with the 2D plane-row scanout, at any moment.
    #[test]
    fn test_reject_gl_device_on_2d_path_after_deferral() {
        assert_eq!(scanout_path(true, false, true), Err(ScanoutFatal::GlDrawUnavailable));
        assert_ne!(scanout_path(true, false, true), Ok(ScanoutPath::PlaneRow2d));
    }
}
