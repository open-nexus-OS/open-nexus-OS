// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The ONE readback authority (RFC-0093 §5, TASK-0324 P6-d). Display truth is read
//! from a dedicated PROBE render target, never from the scanout: a host-side
//! `RESOURCE_COPY_REGION` lifts the region of interest off the FRONT RT into the probe RT, and
//! only the probe RT is ever transferred back to the guest. The scanout is never a transfer
//! source, so a readback can neither disturb a flip nor observe a mid-flip frame. Screen
//! capture (TASK-0068) builds on this primitive — a bigger probe, the same two steps.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU (`gpud: probe sample ok` → `SELFTEST: display nonblack ok`)

use crate::backend::VirtioGpuBackend;
use crate::protocol::{
    VirtioGpuCtxAttachResource, VirtioGpuResourceCreate3d, VirtioGpuResourceDetachBacking,
    VirtioGpuResourceUnref, VirtioGpuSubmit3d, VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE,
    VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING, VIRTIO_GPU_CMD_RESOURCE_CREATE_3D,
    VIRTIO_GPU_CMD_RESOURCE_DETACH_BACKING, VIRTIO_GPU_CMD_RESOURCE_UNREF,
    VIRTIO_GPU_CMD_SUBMIT_3D,
};
use crate::virgl::{
    Submit3d, PIPE_BIND_RENDER_TARGET, PIPE_FORMAT_B8G8R8A8_UNORM, PIPE_TEXTURE_2D,
};
use nexus_gfx::backend::error::GfxError;

/// Probe RT resource id — free in the `0xE0..` display block (E0/E1/E2/E5/E6/E7/E8 taken).
pub(crate) const H_PROBE_TEX: u32 = 0xE9;
/// Probe RT geometry: room for the display-truth strip and the small regions later
/// consumers need. A full-frame capture creates its own probe of its own size.
pub(crate) const PROBE_W: u32 = 256;
pub(crate) const PROBE_H: u32 = 64;
/// A capture's copy destination (RFC-0095) — free in the display block; created per readback
/// at the rectangle's size, backed by the CALLER's VMO, released again (the memory stays theirs).
const H_CAPTURE_TEX: u32 = 0xEC;

impl VirtioGpuBackend {
    /// Create the probe RT (once, at GL scanout bring-up). Not a scanout, not a sampler
    /// view: a copy destination with guest backing, nothing else.
    pub(crate) fn gl_probe_init(&mut self) -> Result<(), GfxError> {
        let create = VirtioGpuResourceCreate3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_CREATE_3D),
            resource_id: H_PROBE_TEX,
            target: PIPE_TEXTURE_2D,
            format: PIPE_FORMAT_B8G8R8A8_UNORM,
            bind: PIPE_BIND_RENDER_TARGET,
            width: PROBE_W,
            height: PROBE_H,
            depth: 1,
            array_size: 1,
            last_level: 0,
            nr_samples: 0,
            flags: 0,
            _padding: 0,
        };
        self.ctrl_submit_struct(&create)?;
        let attach = VirtioGpuCtxAttachResource {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE),
            resource_id: H_PROBE_TEX,
            _padding: 0,
        };
        self.ctrl_submit_struct(&attach)?;
        self.gl_swap.probe_backing_va =
            self.virgl_attach_backing(H_PROBE_TEX, (PROBE_W * PROBE_H * 4) as usize)?;
        Ok(())
    }

    /// Lift `w×h` at `(x, y)` of the FRONT RT into the probe and read it back. Returns the
    /// probe backing's guest VA and its row stride in bytes (BGRA); the caller reads it with
    /// volatile loads — the device wrote it. `None` when no GL scanout / probe exists or the
    /// region fits neither the frame nor the probe.
    pub(crate) fn probe_region(
        &mut self,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
    ) -> Option<(usize, usize)> {
        let va = self.gl_swap.probe_backing_va;
        if va == 0 || w == 0 || h == 0 || w > PROBE_W || h > PROBE_H {
            return None;
        }
        if x.checked_add(w)? > self.display_w || y.checked_add(h)? > self.display_h {
            return None;
        }
        self.copy_front_to_guest(H_PROBE_TEX, x, y, w, h, PROBE_W * 4).ok()?;
        Some((va, (PROBE_W * 4) as usize))
    }

    /// The two steps every readback takes (RFC-0093 §5): a host-side copy of `w×h` at `(x, y)`
    /// of the FRONT RT into `res` at its origin — the scanout is only ever a copy SOURCE — then
    /// the transfer of `res` into its guest backing, rows `stride` bytes apart. Both commands
    /// are processed in ring order after the last flip, so the bytes are the shown frame.
    fn copy_front_to_guest(
        &mut self,
        res: u32,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        stride: u32,
    ) -> Result<(), GfxError> {
        let mut s = Submit3d::new();
        s.emit_resource_copy_region(res, 0, 0, self.rt_front_res(), x, y, w, h);
        let bytes = s.as_bytes();
        let hdr = VirtioGpuSubmit3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_SUBMIT_3D),
            size: bytes.len() as u32,
            _padding: 0,
        };
        self.ctrl_submit_header_tail(&hdr, bytes)?;
        self.virgl_transfer_from_host(res, 0, 0, w, h, stride)
    }

    /// `OP_READBACK` on GL (RFC-0095): the rectangle of the shown frame lands in the caller's
    /// VMO `dest`, rows tight. A copy destination of the rectangle's size is created with
    /// `dest` as its backing, filled by the two readback steps and released again — gpud keeps
    /// no memory and no resource per capture. With `READBACK_FREEZE` the whole shown frame is
    /// also copied into the wallpaper texture: the compositor's base layer until windowd's
    /// next `OP_WALLPAPER_DIRTY` (the thaw), and the backdrop the glass blur samples.
    pub(crate) fn gl_readback(
        &mut self,
        dest: u32,
        req: &nexus_display_proto::readback::Readback,
    ) -> Result<(), GfxError> {
        if !req.fits(self.display_w, self.display_h) {
            return Err(GfxError::InvalidArgument);
        }
        let (w, h) = (u32::from(req.w), u32::from(req.h));
        let create = VirtioGpuResourceCreate3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_CREATE_3D),
            resource_id: H_CAPTURE_TEX,
            target: PIPE_TEXTURE_2D,
            format: PIPE_FORMAT_B8G8R8A8_UNORM,
            bind: PIPE_BIND_RENDER_TARGET,
            width: w,
            height: h,
            depth: 1,
            array_size: 1,
            last_level: 0,
            nr_samples: 0,
            flags: 0,
            _padding: 0,
        };
        self.ctrl_submit_struct(&create)?;
        let filled = self.fill_capture(dest, req, w, h);
        // Released whatever happened: the id is reused by the next capture, and the backing
        // is the caller's memory, never gpud's to keep.
        let detach = VirtioGpuResourceDetachBacking {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_DETACH_BACKING),
            resource_id: H_CAPTURE_TEX,
            _padding: 0,
        };
        let _ = self.ctrl_submit_struct(&detach);
        let unref = VirtioGpuResourceUnref {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_UNREF),
            resource_id: H_CAPTURE_TEX,
            _padding: 0,
        };
        let _ = self.ctrl_submit_struct(&unref);
        filled?;
        if req.flags & nexus_display_proto::readback::READBACK_FREEZE != 0 {
            let mut s = Submit3d::new();
            s.emit_resource_copy_region(
                crate::gl_scanout::H_WALLPAPER_TEX,
                0,
                0,
                self.rt_front_res(),
                0,
                0,
                self.display_w,
                self.display_h,
            );
            let bytes = s.as_bytes();
            let hdr = VirtioGpuSubmit3d {
                hdr: self.virgl_hdr(VIRTIO_GPU_CMD_SUBMIT_3D),
                size: bytes.len() as u32,
                _padding: 0,
            };
            self.ctrl_submit_header_tail(&hdr, bytes)?;
            // The glass over the wallpaper re-samples the frozen frame.
            self.blur_cache.wallpaper_epoch = self.blur_cache.wallpaper_epoch.wrapping_add(1);
        }
        Ok(())
    }

    /// Attach `dest` as the capture texture's backing and run the two readback steps.
    fn fill_capture(
        &mut self,
        dest: u32,
        req: &nexus_display_proto::readback::Readback,
        w: u32,
        h: u32,
    ) -> Result<(), GfxError> {
        let hdr = self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING);
        self.attach_backing_runs(hdr, H_CAPTURE_TEX, dest, 0, req.bytes())?;
        let attach = VirtioGpuCtxAttachResource {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE),
            resource_id: H_CAPTURE_TEX,
            _padding: 0,
        };
        self.ctrl_submit_struct(&attach)?;
        self.copy_front_to_guest(H_CAPTURE_TEX, u32::from(req.x), u32::from(req.y), w, h, w * 4)
    }

    /// Display truth (one-shot at the first successful present): the brightest pixel of a
    /// 64×4 strip at the centre of the frame the display shows — read through the probe.
    pub(crate) fn probe_sample(&mut self) -> Option<[u8; 4]> {
        const SAMPLE_W: u32 = 64;
        const SAMPLE_H: u32 = 4;
        let x = self.display_w.saturating_sub(SAMPLE_W) / 2;
        let y = self.display_h.saturating_sub(SAMPLE_H) / 2;
        let (va, stride) = self.probe_region(x, y, SAMPLE_W, SAMPLE_H)?;
        let mut best = [0u8; 4];
        let mut best_sum: u32 = 0;
        for row in 0..SAMPLE_H as usize {
            for col in 0..SAMPLE_W as usize {
                let off = row * stride + col * 4;
                // SAFETY: `va..va+PROBE_W*PROBE_H*4` is the probe's attached guest backing.
                let px = unsafe { core::ptr::read_volatile((va + off) as *const [u8; 4]) };
                let sum = px[0] as u32 + px[1] as u32 + px[2] as u32;
                if sum > best_sum {
                    best_sum = sum;
                    best = px;
                }
            }
        }
        Some(best)
    }
}
