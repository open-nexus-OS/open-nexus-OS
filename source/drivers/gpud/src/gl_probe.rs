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
    VirtioGpuCtxAttachResource, VirtioGpuResourceCreate3d, VirtioGpuSubmit3d,
    VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE, VIRTIO_GPU_CMD_RESOURCE_CREATE_3D,
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
        // Step 1: host-side copy front RT → probe RT (the scanout is only ever a copy SOURCE).
        let mut s = Submit3d::new();
        s.emit_resource_copy_region(H_PROBE_TEX, 0, 0, self.rt_front_res(), x, y, w, h);
        let bytes = s.as_bytes();
        let hdr = VirtioGpuSubmit3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_SUBMIT_3D),
            size: bytes.len() as u32,
            _padding: 0,
        };
        self.ctrl_submit_header_tail(&hdr, bytes).ok()?;
        // Step 2: transfer the PROBE back to the guest — never the scanout.
        self.virgl_transfer_from_host(H_PROBE_TEX, 0, 0, w, h, PROBE_W * 4).ok()?;
        Some((va, (PROBE_W * 4) as usize))
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
