// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Virgl 3D bring-up + command stream: the context, render-target creation, backing attach and
//! transfers, and the probe's readback proofs of the 3D path — the wire format, the draw state,
//! and draws with the library's pipelines (`virgl_pipelines`). Compiled only for the virgl OS
//! build; the 2D path never pulls in this code.

#![cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]

use super::transport::{align_page, read_reg, write_reg};
use super::VirtioGpuBackend;
use crate::error::GpuDriverError;
use crate::protocol;
use nexus_gfx::backend::error::GfxError;

/// The probe's draw targets are this square.
const PROBE_SIDE: u32 = 64;

/// The BGRA bytes at (`x`, `y`) of a `width`-pixel-wide render target's guest backing at `va`,
/// after its transfer from the host.
pub(crate) fn rt_pixel(va: usize, width: u32, x: u32, y: u32) -> [u8; 4] {
    let at = (y as usize * width as usize + x as usize) * 4;
    // SAFETY: the callers pass a backing of `width` × height pixels mapped by
    // `virgl_attach_backing` and a pixel inside it; the transfer has completed (synchronous).
    unsafe {
        let p = (va + at) as *const u8;
        [
            p.read_volatile(),
            p.add(1).read_volatile(),
            p.add(2).read_volatile(),
            p.add(3).read_volatile(),
        ]
    }
}

fn probe_pixel(va: usize, x: u32, y: u32) -> [u8; 4] {
    rt_pixel(va, PROBE_SIDE, x, y)
}

impl VirtioGpuBackend {
    /// Create a virgl rendering context for GPU shader dispatch.
    /// Must be called after probe_os() (ctrlq is set up).
    pub(crate) fn create_virgl_context(&mut self) -> Result<(), GpuDriverError> {
        use crate::protocol::{
            VirtioGpuCtrlHdr, VirtioGpuCtxCreate, VIRTIO_GPU_CAPSET_VIRGL2,
            VIRTIO_GPU_CMD_CTX_CREATE,
        };
        let mut name = [0u8; 64];
        let label = b"gpud-virgl-ctx";
        let len = label.len().min(64);
        name[..len].copy_from_slice(&label[..len]);

        // The guest chooses the context id; subsequent 3D commands carry it in
        // their header's ctx_id field. We use a single context (id 1).
        const VIRGL_CTX_ID: u32 = 1;
        let cmd = VirtioGpuCtxCreate {
            hdr: VirtioGpuCtrlHdr {
                type_: VIRTIO_GPU_CMD_CTX_CREATE,
                flags: 0,
                fence_id: 0,
                ctx_id: VIRGL_CTX_ID,
                _padding: 0,
            },
            nlen: len as u32,
            context_init: VIRTIO_GPU_CAPSET_VIRGL2,
            debug_name: name,
        };

        // `ctrl_submit_struct` writes the command, notifies the device, and
        // validates the response header (RESP_OK_NODATA → Ok, else Err).
        self.ctrl_submit_struct(&cmd).map_err(|_| GpuDriverError::CommandRejected)?;
        self.virgl_ctx_id = VIRGL_CTX_ID;
        Ok(())
    }

    /// End-to-end validation of the `SUBMIT_3D` path: emit a minimal NOP stream
    /// and confirm virglrenderer accepts it. This proves the 3D wire format and
    /// context routing work before the full blur pipeline is built; it does not
    /// touch the blur path (blur stays on the CPU separable gaussian until the
    /// GPU shader lands).
    pub(crate) fn submit3d_selftest(&mut self) -> Result<(), GfxError> {
        use crate::protocol::{VirtioGpuCtrlHdr, VirtioGpuSubmit3d, VIRTIO_GPU_CMD_SUBMIT_3D};
        let mut stream = crate::virgl::Submit3d::new();
        stream.emit_nop();
        let bytes = stream.as_bytes();
        let hdr = VirtioGpuSubmit3d {
            hdr: VirtioGpuCtrlHdr {
                type_: VIRTIO_GPU_CMD_SUBMIT_3D,
                flags: 0,
                fence_id: 0,
                ctx_id: self.virgl_ctx_id,
                _padding: 0,
            },
            size: bytes.len() as u32,
            _padding: 0,
        };
        self.ctrl_submit_header_tail(&hdr, bytes)
    }

    /// A virtio-gpu control header carrying our virgl context id (3D commands
    /// are context-scoped, unlike the 2D path which uses ctx_id 0).
    pub(crate) fn virgl_hdr(&self, type_: u32) -> protocol::VirtioGpuCtrlHdr {
        protocol::VirtioGpuCtrlHdr {
            type_,
            flags: 0,
            fence_id: 0,
            ctx_id: self.virgl_ctx_id,
            _padding: 0,
        }
    }

    /// Create a 3D render-target texture and attach it to the virgl context.
    /// Bound as both RENDER_TARGET (draw destination) and SAMPLER_VIEW (so the
    /// blur shader can later read a source texture of the same shape).
    pub(crate) fn virgl_create_rt(&mut self, res_id: u32, w: u32, h: u32) -> Result<(), GfxError> {
        use crate::protocol::{
            VirtioGpuCtxAttachResource, VirtioGpuResourceCreate3d,
            VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE, VIRTIO_GPU_CMD_RESOURCE_CREATE_3D,
        };
        use crate::virgl::{
            PIPE_BIND_RENDER_TARGET, PIPE_BIND_SAMPLER_VIEW, PIPE_FORMAT_B8G8R8A8_UNORM,
            PIPE_TEXTURE_2D,
        };
        let create = VirtioGpuResourceCreate3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_CREATE_3D),
            resource_id: res_id,
            target: PIPE_TEXTURE_2D,
            format: PIPE_FORMAT_B8G8R8A8_UNORM,
            bind: PIPE_BIND_RENDER_TARGET | PIPE_BIND_SAMPLER_VIEW,
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
        let attach = VirtioGpuCtxAttachResource {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE),
            resource_id: res_id,
            _padding: 0,
        };
        self.ctrl_submit_struct(&attach)?;
        Ok(())
    }

    /// Increment A: create a render target, wrap it as a surface, bind it as
    /// the framebuffer, and clear it. Validates the draw-state pipeline
    /// (resource → surface → framebuffer → clear) end-to-end against
    /// virglrenderer before shaders/draw are introduced. Does not touch the
    /// blur path.
    pub(crate) fn virgl_rt_clear_test(&mut self) -> Result<(), GfxError> {
        use crate::protocol::{VirtioGpuCtrlHdr, VirtioGpuSubmit3d, VIRTIO_GPU_CMD_SUBMIT_3D};
        use crate::virgl::{Submit3d, PIPE_CLEAR_COLOR0, PIPE_FORMAT_B8G8R8A8_UNORM};
        const RT_RES_ID: u32 = 0xF0;
        const SURFACE_HANDLE: u32 = 1;

        self.virgl_create_rt(RT_RES_ID, 64, 64)?;

        let mut s = Submit3d::new();
        s.emit_create_surface(SURFACE_HANDLE, RT_RES_ID, PIPE_FORMAT_B8G8R8A8_UNORM);
        s.emit_set_framebuffer_state(0, &[SURFACE_HANDLE]);
        s.emit_clear(PIPE_CLEAR_COLOR0, [1.0, 0.0, 0.0, 1.0], 1.0, 0);
        let bytes = s.as_bytes();
        let hdr = VirtioGpuSubmit3d {
            hdr: VirtioGpuCtrlHdr {
                type_: VIRTIO_GPU_CMD_SUBMIT_3D,
                flags: 0,
                fence_id: 0,
                ctx_id: self.virgl_ctx_id,
                _padding: 0,
            },
            size: bytes.len() as u32,
            _padding: 0,
        };
        self.ctrl_submit_header_tail(&hdr, bytes)
    }

    /// Allocate a guest VMO, map it into the virgl backing VA region, and
    /// attach it as the backing store of `res_id` (then attach the resource to
    /// the virgl context). Returns the backing VA for CPU access after
    /// TRANSFER_FROM_HOST_3D.
    pub(crate) fn virgl_attach_backing(
        &mut self,
        res_id: u32,
        byte_len: usize,
    ) -> Result<usize, GfxError> {
        use crate::protocol::{
            VirtioGpuCtxAttachResource, VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE,
            VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING,
        };
        // RFC-0085: one whole-range `vm_map` at a kernel-chosen va (the old
        // 12-slot backing arena is gone). RFC-0098 C4 (TASK-0286 P4a): an
        // anonymous object made for the GPU (inside its DMA reach, TASK-0246 P1);
        // the device reads it through its runs.
        let backing_len = align_page(byte_len);
        let vmo = nexus_abi::vmo_create_for(self.device, backing_len)
            .map_err(|_| GfxError::ResourceExhausted)?;
        let flags = nexus_abi::page_flags::VALID
            | nexus_abi::page_flags::USER
            | nexus_abi::page_flags::READ
            | nexus_abi::page_flags::WRITE;
        let backing_va =
            nexus_abi::vm_map(vmo, 0, backing_len, flags).map_err(|_| GfxError::MmioFault)?;
        unsafe { core::ptr::write_bytes(backing_va as *mut u8, 0, backing_len) };
        let hdr = self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING);
        self.attach_backing_runs(hdr, res_id, vmo, 0, byte_len)?;
        let ctx_attach = VirtioGpuCtxAttachResource {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE),
            resource_id: res_id,
            _padding: 0,
        };
        self.ctrl_submit_struct(&ctx_attach)?;
        Ok(backing_va)
    }

    /// Issue TRANSFER_FROM_HOST_3D for a full-width box of `res_id` and wait
    /// for completion — host GPU contents land in the resource's guest backing.
    pub(crate) fn virgl_transfer_from_host(
        &mut self,
        res_id: u32,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        stride: u32,
    ) -> Result<(), GfxError> {
        use crate::protocol::{
            VirtioGpuBox, VirtioGpuTransferHost3d, VIRTIO_GPU_CMD_TRANSFER_FROM_HOST_3D,
        };
        let cmd = VirtioGpuTransferHost3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_TRANSFER_FROM_HOST_3D),
            box_: VirtioGpuBox { x, y, z: 0, w, h, d: 1 },
            offset: (y as u64) * (stride as u64) + (x as u64) * 4,
            resource_id: res_id,
            level: 0,
            stride,
            layer_stride: 0,
        };
        self.ctrl_submit_struct(&cmd)
    }

    /// A 64×64 render target with a guest backing, cleared blue, the library's gradient pipeline
    /// drawn over all of it from `top` to `bottom`, read back: the backing's address. The probe's
    /// draw proofs — the gradient program is our toolchain's, so a pixel it wrote proves the
    /// library's text parses, binds and runs.
    fn gradient_probe(
        &mut self,
        rt: u32,
        surface: u32,
        top: [f32; 4],
        bottom: [f32; 4],
    ) -> Result<usize, GfxError> {
        use crate::virgl::{Submit3d, PIPE_CLEAR_COLOR0, PIPE_FORMAT_B8G8R8A8_UNORM};
        use nexus_gfx::backend::gpu::quads::Quad;
        if !self.pipelines.ready {
            return Err(GfxError::DeviceNotFound);
        }
        self.virgl_create_rt(rt, PROBE_SIDE, PROBE_SIDE)?;
        let backing_va = self.virgl_attach_backing(rt, (PROBE_SIDE * PROBE_SIDE * 4) as usize)?;
        let side = PROBE_SIDE as f32;
        let mut s = Submit3d::new();
        s.emit_create_surface(surface, rt, PIPE_FORMAT_B8G8R8A8_UNORM);
        s.emit_set_framebuffer_state(0, &[surface]);
        s.emit_set_viewport_box(0.0, 0.0, side, side);
        s.emit_clear(PIPE_CLEAR_COLOR0, [0.0, 0.0, 1.0, 1.0], 1.0, 0); // blue base
        let q = Quad::gradient(0.0, 0.0, side, side, top, bottom, 0.0);
        self.emit_draw(&mut s, self.pipelines.gradient, surface, &q, 0);
        self.submit3d(&s)?;
        self.virgl_transfer_from_host(rt, 0, 0, PROBE_SIDE, PROBE_SIDE, PROBE_SIDE * 4)?;
        Ok(backing_va)
    }

    /// Full-pipeline draw proof: the gradient pipeline red from top to bottom over the blue clear;
    /// the centre pixel (BGRA bytes) says how far the pipeline got — red: it drew; blue: the
    /// clear alone. This verifies the entire 3D path on the device, no display needed.
    pub(crate) fn virgl_draw_selftest(&mut self) -> Result<[u8; 4], GfxError> {
        const RT_RES: u32 = 0xF4;
        const H_SURF: u32 = 0x24;
        let red = [1.0, 0.0, 0.0, 1.0];
        let va = self.gradient_probe(RT_RES, H_SURF, red, red)?;
        Ok(probe_pixel(va, PROBE_SIDE / 2, PROBE_SIDE / 2))
    }

    /// Gradient proof: the gradient pipeline from red at the top to blue at the bottom. True when
    /// a row near the top reads red and one near the bottom blue — the colour interpolated across
    /// the rectangle, and `uv` running down it as the programs assume (clip y −1 is the top).
    pub(crate) fn virgl_gradient_selftest(&mut self) -> Result<bool, GfxError> {
        const RT_RES: u32 = 0xF6;
        const H_SURF: u32 = 0x54;
        let va = self.gradient_probe(RT_RES, H_SURF, [1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0])?;
        let (top, bottom) =
            (probe_pixel(va, PROBE_SIDE / 2, 4), probe_pixel(va, PROBE_SIDE / 2, PROBE_SIDE - 5));
        // BGRA: red is byte 2, blue byte 0.
        Ok(top[2] > top[0].saturating_add(128) && bottom[0] > bottom[2].saturating_add(128))
    }

    /// Issue TRANSFER_TO_HOST_3D for a box of `res_id` (guest backing → host
    /// GL texture) and wait for completion.
    pub(crate) fn virgl_transfer_to_host(
        &mut self,
        res_id: u32,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        stride: u32,
    ) -> Result<(), GfxError> {
        use crate::protocol::{
            VirtioGpuBox, VirtioGpuTransferHost3d, VIRTIO_GPU_CMD_TRANSFER_TO_HOST_3D,
        };
        let cmd = VirtioGpuTransferHost3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_TRANSFER_TO_HOST_3D),
            box_: VirtioGpuBox { x, y, z: 0, w, h, d: 1 },
            offset: (y as u64) * (stride as u64) + (x as u64) * 4,
            resource_id: res_id,
            level: 0,
            stride,
            layer_stride: 0,
        };
        self.ctrl_submit_struct(&cmd)
    }

    /// Read the device feature bits and acknowledge the subset we support for
    /// virgl 3D. Must run after ACKNOWLEDGE|DRIVER and before FEATURES_OK.
    ///
    /// Sets `self.virgl_capable` iff the device offered `VIRTIO_GPU_F_VIRGL`.
    /// We ack VIRGL (3D), CONTEXT_INIT (so CTX_CREATE may select the VIRGL2
    /// capset), and VERSION_1 (modern virtio — the queue is already driven via
    /// the split DESC/DRIVER/DEVICE registers, so VERSION_1 is the correct mode).
    pub(crate) fn negotiate_features_virgl(&mut self) {
        // Low feature word (bits 0..31).
        write_reg(self.mmio_base, protocol::VIRTIO_MMIO_DEVICE_FEATURES_SEL, 0);
        let dev_lo = read_reg(self.mmio_base, protocol::VIRTIO_MMIO_DEVICE_FEATURES);
        // High feature word (bits 32..63), where VERSION_1 lives.
        write_reg(self.mmio_base, protocol::VIRTIO_MMIO_DEVICE_FEATURES_SEL, 1);
        let dev_hi = read_reg(self.mmio_base, protocol::VIRTIO_MMIO_DEVICE_FEATURES);

        let want_lo = protocol::VIRTIO_GPU_F_VIRGL | protocol::VIRTIO_GPU_F_CONTEXT_INIT;
        let drv_lo = dev_lo & want_lo;
        let drv_hi = dev_hi & protocol::VIRTIO_F_VERSION_1_HI;

        self.virgl_capable = (dev_lo & protocol::VIRTIO_GPU_F_VIRGL) != 0;

        write_reg(self.mmio_base, protocol::VIRTIO_MMIO_DRIVER_FEATURES_SEL, 0);
        write_reg(self.mmio_base, protocol::VIRTIO_MMIO_DRIVER_FEATURES, drv_lo);
        write_reg(self.mmio_base, protocol::VIRTIO_MMIO_DRIVER_FEATURES_SEL, 1);
        write_reg(self.mmio_base, protocol::VIRTIO_MMIO_DRIVER_FEATURES, drv_hi);
    }
}
