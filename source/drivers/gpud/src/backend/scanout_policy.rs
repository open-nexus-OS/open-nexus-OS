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
}
