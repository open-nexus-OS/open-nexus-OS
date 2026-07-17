// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Virgl 3D bring-up + command stream: render-target creation, backing
//! attach/transfer, the draw/gradient/shader self-tests that validate the GPU
//! pipeline by readback, and the gaussian backdrop-blur submit. Compiled only
//! for the virgl OS build; the 2D path never pulls in this code.

#![cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]

use super::raster::blur_backdrop_separable_vmo;
use super::transport::{
    align_page, read_reg, write_reg, GPU_VIRGL_BACKING_BASE_VA, GPU_VIRGL_BACKING_STRIDE,
};
use super::VirtioGpuBackend;
use crate::error::GpuDriverError;
use crate::markers::{
    GPUD_VIRGL_BLUR_GPU_ON, GPUD_VIRGL_BLUR_PARITY_OFF, GPUD_VIRGL_BLUR_PARITY_OK,
};
use crate::protocol;
use nexus_gfx::backend::error::GfxError;

impl VirtioGpuBackend {
    /// Create a virgl rendering context for GPU shader dispatch.
    /// Must be called after probe_os() (ctrlq is set up).
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
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
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
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
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
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
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
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
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
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
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) fn virgl_attach_backing(
        &mut self,
        res_id: u32,
        byte_len: usize,
    ) -> Result<usize, GfxError> {
        use crate::protocol::{
            VirtioGpuCtxAttachResource, VirtioGpuMemEntry, VirtioGpuResourceAttachBacking,
            VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE, VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING,
        };
        let slot = self.virgl_backing_count;
        if slot >= 8 {
            return Err(GfxError::ResourceExhausted);
        }
        let backing_va = GPU_VIRGL_BACKING_BASE_VA + slot * GPU_VIRGL_BACKING_STRIDE;
        let backing_len = align_page(byte_len);
        let vmo = nexus_abi::vmo_create(backing_len).map_err(|_| GfxError::ResourceExhausted)?;
        let flags = nexus_abi::page_flags::VALID
            | nexus_abi::page_flags::USER
            | nexus_abi::page_flags::READ
            | nexus_abi::page_flags::WRITE;
        for offset in (0..backing_len).step_by(4096) {
            nexus_abi::vmo_map_page(vmo, backing_va + offset, offset, flags)
                .map_err(|_| GfxError::MmioFault)?;
        }
        unsafe { core::ptr::write_bytes(backing_va as *mut u8, 0, backing_len) };
        let mut info = nexus_abi::CapQuery { kind_tag: 0, reserved: 0, base: 0, len: 0 };
        nexus_abi::cap_query(vmo, &mut info).map_err(|_| GfxError::MmioFault)?;
        self.virgl_backing_count = slot + 1;

        let attach = VirtioGpuResourceAttachBacking {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING),
            resource_id: res_id,
            nr_entries: 1,
        };
        let entry = VirtioGpuMemEntry { addr: info.base, length: byte_len as u32, _padding: 0 };
        self.ctrl_submit_pair(&attach, &entry)?;
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
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
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

    /// Full-pipeline GPU draw proof: state objects + fullscreen-triangle vertex
    /// buffer + the (already created) passthrough VS / solid-red FS + DRAW_VBO
    /// into a 64×64 render target, then TRANSFER_FROM_HOST_3D readback and a
    /// CPU pixel check. Returns the first pixel's BGRA bytes.
    ///
    /// This verifies the entire 3D path end-to-end on-device — no display
    /// needed: if the pixels read back red, the GPU really rasterized our draw.
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) fn virgl_draw_selftest(&mut self) -> Result<[u8; 4], GfxError> {
        use crate::protocol::{VirtioGpuSubmit3d, VIRTIO_GPU_CMD_SUBMIT_3D};
        use crate::virgl::{
            Submit3d, PIPE_BIND_VERTEX_BUFFER, PIPE_BUFFER, PIPE_CLEAR_COLOR0,
            PIPE_FORMAT_B8G8R8A8_UNORM, PIPE_FORMAT_R32G32B32A32_FLOAT, PIPE_FORMAT_R8_UNORM,
            PIPE_PRIM_TRIANGLES, PIPE_SHADER_FRAGMENT, PIPE_SHADER_VERTEX, VIRGL_OBJECT_BLEND,
            VIRGL_OBJECT_DSA, VIRGL_OBJECT_RASTERIZER, VIRGL_OBJECT_VERTEX_ELEMENTS,
        };
        const RT_RES: u32 = 0xF4;
        const VBO_RES: u32 = 0xF5;
        const RT_W: u32 = 64;
        const RT_H: u32 = 64;
        // Object handles (context-scoped namespace).
        const H_BLEND: u32 = 0x20;
        const H_DSA: u32 = 0x21;
        const H_RAST: u32 = 0x22;
        const H_VE: u32 = 0x23;
        const H_SURF: u32 = 0x24;
        const H_VS: u32 = 10; // created by virgl_shader_test
        const H_FS: u32 = 11;

        // Render target with guest backing for readback.
        self.virgl_create_rt(RT_RES, RT_W, RT_H)?;
        let backing_va = self.virgl_attach_backing(RT_RES, (RT_W * RT_H * 4) as usize)?;

        // Vertex buffer resource (host-side storage; filled via INLINE_WRITE).
        {
            use crate::protocol::{VirtioGpuResourceCreate3d, VIRTIO_GPU_CMD_RESOURCE_CREATE_3D};
            let create = VirtioGpuResourceCreate3d {
                hdr: self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_CREATE_3D),
                resource_id: VBO_RES,
                target: PIPE_BUFFER,
                format: PIPE_FORMAT_R8_UNORM,
                bind: PIPE_BIND_VERTEX_BUFFER,
                width: 48, // 3 vertices × vec4 f32
                height: 1,
                depth: 1,
                array_size: 1,
                last_level: 0,
                nr_samples: 0,
                flags: 0,
                _padding: 0,
            };
            self.ctrl_submit_struct(&create)?;
            use crate::protocol::{VirtioGpuCtxAttachResource, VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE};
            let attach = VirtioGpuCtxAttachResource {
                hdr: self.virgl_hdr(VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE),
                resource_id: VBO_RES,
                _padding: 0,
            };
            self.ctrl_submit_struct(&attach)?;
        }

        // Fullscreen triangle (clip space), vec4 positions.
        let verts: [f32; 12] = [-1.0, -1.0, 0.0, 1.0, 3.0, -1.0, 0.0, 1.0, -1.0, 3.0, 0.0, 1.0];
        let mut vbytes = [0u8; 48];
        for (i, v) in verts.iter().enumerate() {
            vbytes[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }

        let mut s = Submit3d::new();
        s.emit_resource_inline_write(VBO_RES, &vbytes);
        s.emit_create_blend_default(H_BLEND);
        s.emit_bind_object(VIRGL_OBJECT_BLEND, H_BLEND);
        s.emit_create_dsa_default(H_DSA);
        s.emit_bind_object(VIRGL_OBJECT_DSA, H_DSA);
        s.emit_create_rasterizer_default(H_RAST);
        s.emit_bind_object(VIRGL_OBJECT_RASTERIZER, H_RAST);
        s.emit_create_vertex_elements(H_VE, &[(0, 0, 0, PIPE_FORMAT_R32G32B32A32_FLOAT)]);
        s.emit_bind_object(VIRGL_OBJECT_VERTEX_ELEMENTS, H_VE);
        s.emit_set_vertex_buffers(&[(16, 0, VBO_RES)]);
        s.emit_create_surface(H_SURF, RT_RES, PIPE_FORMAT_B8G8R8A8_UNORM);
        s.emit_set_framebuffer_state(0, &[H_SURF]);
        s.emit_set_viewport(RT_W as f32, RT_H as f32);
        s.emit_clear(PIPE_CLEAR_COLOR0, [0.0, 0.0, 1.0, 1.0], 1.0, 0); // blue base
        s.emit_bind_shader(H_VS, PIPE_SHADER_VERTEX);
        s.emit_bind_shader(H_FS, PIPE_SHADER_FRAGMENT);
        s.emit_draw_vbo(0, 3, PIPE_PRIM_TRIANGLES);
        let bytes = s.as_bytes();
        let hdr = VirtioGpuSubmit3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_SUBMIT_3D),
            size: bytes.len() as u32,
            _padding: 0,
        };
        self.ctrl_submit_header_tail(&hdr, bytes)?;

        // Read the rendered pixels back into the guest backing and inspect.
        self.virgl_transfer_from_host(RT_RES, 0, 0, RT_W, RT_H, RT_W * 4)?;
        let center = (32 * RT_W as usize + 32) * 4;
        let px = unsafe {
            let p = (backing_va + center) as *const u8;
            [
                p.read_volatile(),
                p.add(1).read_volatile(),
                p.add(2).read_volatile(),
                p.add(3).read_volatile(),
            ]
        };
        Ok(px)
    }

    /// GPU vector pipeline (M1a): render a per-pixel **gradient** quad and read
    /// it back. Uses a colour vertex attribute interpolated by the rasterizer
    /// (the simplest, hardware-exact gradient) — a vertex shader that passes the
    /// colour through and a fragment shader that outputs it. Proves end-to-end
    /// GPU gradient fills before the SDF/analytic-AA fragment shader is added.
    ///
    /// Returns `true` if the read-back top and bottom rows differ (interpolation
    /// happened), `false` if the quad came out flat.
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) fn virgl_gradient_selftest(&mut self) -> Result<bool, GfxError> {
        use crate::protocol::{VirtioGpuSubmit3d, VIRTIO_GPU_CMD_SUBMIT_3D};
        use crate::virgl::{
            Submit3d, PIPE_BIND_VERTEX_BUFFER, PIPE_BUFFER, PIPE_CLEAR_COLOR0,
            PIPE_FORMAT_B8G8R8A8_UNORM, PIPE_FORMAT_R32G32B32A32_FLOAT, PIPE_FORMAT_R8_UNORM,
            PIPE_PRIM_TRIANGLES, PIPE_SHADER_FRAGMENT, PIPE_SHADER_VERTEX, VIRGL_OBJECT_BLEND,
            VIRGL_OBJECT_DSA, VIRGL_OBJECT_RASTERIZER, VIRGL_OBJECT_VERTEX_ELEMENTS,
        };
        const RT_RES: u32 = 0xF6;
        const VBO_RES: u32 = 0xF7;
        const RT_W: u32 = 64;
        const RT_H: u32 = 64;
        // Handle namespace: the virgl context has ONE object table — handles
        // MUST be unique across subsystems (collisions fail silently on the
        // wire). Allocations: 10/11 boot VS/FS, 0x20..0x24 draw-selftest state,
        // 13 blur FS, 14 gl_scanout blit FS, 16/17 gradient VS/FS,
        // 0x30..0x34 blur surfaces/samplers, 0x42 gl_scanout surface,
        // 0x50..0x54 gradient-selftest state.
        const H_BLEND: u32 = 0x50;
        const H_DSA: u32 = 0x51;
        const H_RAST: u32 = 0x52;
        const H_VE: u32 = 0x53;
        const H_SURF: u32 = 0x54;
        const H_VS: u32 = 16;
        const H_FS: u32 = 17;

        // Gradient shaders: VS passes position + a colour varying through, FS
        // emits the interpolated colour.
        const VS: &str = "VERT\n\
DCL IN[0]\n\
DCL IN[1]\n\
DCL OUT[0], POSITION\n\
DCL OUT[1], GENERIC[0]\n\
MOV OUT[0], IN[0]\n\
MOV OUT[1], IN[1]\n\
END\n";
        const FS: &str = "FRAG\n\
DCL IN[0], GENERIC[0], PERSPECTIVE\n\
DCL OUT[0], COLOR\n\
MOV OUT[0], IN[0]\n\
END\n";
        {
            let mut s = Submit3d::new();
            s.emit_create_shader(H_VS, PIPE_SHADER_VERTEX, VS);
            s.emit_create_shader(H_FS, PIPE_SHADER_FRAGMENT, FS);
            let bytes = s.as_bytes();
            let hdr = VirtioGpuSubmit3d {
                hdr: self.virgl_hdr(VIRTIO_GPU_CMD_SUBMIT_3D),
                size: bytes.len() as u32,
                _padding: 0,
            };
            self.ctrl_submit_header_tail(&hdr, bytes)?;
        }

        self.virgl_create_rt(RT_RES, RT_W, RT_H)?;
        let backing_va = self.virgl_attach_backing(RT_RES, (RT_W * RT_H * 4) as usize)?;

        // Vertex buffer: 6 vertices × (vec4 pos + vec4 colour) = 192 bytes.
        const VBYTES: usize = 6 * 8 * 4;
        {
            use crate::protocol::{VirtioGpuResourceCreate3d, VIRTIO_GPU_CMD_RESOURCE_CREATE_3D};
            let create = VirtioGpuResourceCreate3d {
                hdr: self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_CREATE_3D),
                resource_id: VBO_RES,
                target: PIPE_BUFFER,
                format: PIPE_FORMAT_R8_UNORM,
                bind: PIPE_BIND_VERTEX_BUFFER,
                width: VBYTES as u32,
                height: 1,
                depth: 1,
                array_size: 1,
                last_level: 0,
                nr_samples: 0,
                flags: 0,
                _padding: 0,
            };
            self.ctrl_submit_struct(&create)?;
            use crate::protocol::{VirtioGpuCtxAttachResource, VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE};
            let attach = VirtioGpuCtxAttachResource {
                hdr: self.virgl_hdr(VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE),
                resource_id: VBO_RES,
                _padding: 0,
            };
            self.ctrl_submit_struct(&attach)?;
        }

        // Top (y=+1) red, bottom (y=-1) blue → vertical gradient. Each vertex:
        // [x, y, z, w,  r, g, b, a].
        #[rustfmt::skip]
        let verts: [f32; 48] = [
            -1.0, -1.0, 0.0, 1.0,   0.0, 0.0, 1.0, 1.0,
             1.0, -1.0, 0.0, 1.0,   0.0, 0.0, 1.0, 1.0,
            -1.0,  1.0, 0.0, 1.0,   1.0, 0.0, 0.0, 1.0,
             1.0, -1.0, 0.0, 1.0,   0.0, 0.0, 1.0, 1.0,
             1.0,  1.0, 0.0, 1.0,   1.0, 0.0, 0.0, 1.0,
            -1.0,  1.0, 0.0, 1.0,   1.0, 0.0, 0.0, 1.0,
        ];
        let mut vbytes = [0u8; VBYTES];
        for (i, v) in verts.iter().enumerate() {
            vbytes[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }

        let mut s = Submit3d::new();
        s.emit_resource_inline_write(VBO_RES, &vbytes);
        s.emit_create_blend_default(H_BLEND);
        s.emit_bind_object(VIRGL_OBJECT_BLEND, H_BLEND);
        s.emit_create_dsa_default(H_DSA);
        s.emit_bind_object(VIRGL_OBJECT_DSA, H_DSA);
        s.emit_create_rasterizer_default(H_RAST);
        s.emit_bind_object(VIRGL_OBJECT_RASTERIZER, H_RAST);
        // Two attributes: position @0, colour @16; stride 32.
        s.emit_create_vertex_elements(
            H_VE,
            &[
                (0, 0, 0, PIPE_FORMAT_R32G32B32A32_FLOAT),
                (16, 0, 0, PIPE_FORMAT_R32G32B32A32_FLOAT),
            ],
        );
        s.emit_bind_object(VIRGL_OBJECT_VERTEX_ELEMENTS, H_VE);
        s.emit_set_vertex_buffers(&[(32, 0, VBO_RES)]);
        s.emit_create_surface(H_SURF, RT_RES, PIPE_FORMAT_B8G8R8A8_UNORM);
        s.emit_set_framebuffer_state(0, &[H_SURF]);
        s.emit_set_viewport(RT_W as f32, RT_H as f32);
        s.emit_clear(PIPE_CLEAR_COLOR0, [0.0, 0.0, 0.0, 1.0], 1.0, 0);
        s.emit_bind_shader(H_VS, PIPE_SHADER_VERTEX);
        s.emit_bind_shader(H_FS, PIPE_SHADER_FRAGMENT);
        s.emit_draw_vbo(0, 6, PIPE_PRIM_TRIANGLES);
        let bytes = s.as_bytes();
        let hdr = VirtioGpuSubmit3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_SUBMIT_3D),
            size: bytes.len() as u32,
            _padding: 0,
        };
        self.ctrl_submit_header_tail(&hdr, bytes)?;

        self.virgl_transfer_from_host(RT_RES, 0, 0, RT_W, RT_H, RT_W * 4)?;
        // Sample a near-top and near-bottom pixel; the gradient must differ.
        let read = |row: u32| -> [u8; 4] {
            let off = (row as usize * RT_W as usize + (RT_W as usize / 2)) * 4;
            unsafe {
                let p = (backing_va + off) as *const u8;
                [
                    p.read_volatile(),
                    p.add(1).read_volatile(),
                    p.add(2).read_volatile(),
                    p.add(3).read_volatile(),
                ]
            }
        };
        let top = read(4);
        let bottom = read(RT_H - 5);
        // BGRA: R is byte 2. One row should be red-dominant, the other blue.
        let interpolated = (i32::from(top[2]) - i32::from(bottom[2])).abs() > 32
            || (i32::from(top[0]) - i32::from(bottom[0])).abs() > 32;
        Ok(interpolated)
    }

    /// Increment B: create a passthrough vertex shader and a solid-color
    /// fragment shader from TGSI text. virglrenderer parses the text at create
    /// time, so this validates the CREATE_OBJECT(SHADER) text encoding (the
    /// foundation for the gaussian blur fragment shader). Object handles 10/11
    /// are reserved for these shaders within the virgl context.
    ///
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) fn virgl_shader_test(&mut self) -> Result<(), GfxError> {
        use crate::protocol::{VirtioGpuSubmit3d, VIRTIO_GPU_CMD_SUBMIT_3D};
        use crate::virgl::{Submit3d, PIPE_SHADER_FRAGMENT, PIPE_SHADER_VERTEX};
        const VS: &str = "VERT\nDCL IN[0]\nDCL OUT[0], POSITION\nMOV OUT[0], IN[0]\nEND\n";
        const FS: &str = "FRAG\nDCL OUT[0], COLOR\nIMM[0] FLT32 { 1.0000, 0.0000, 0.0000, 1.0000}\nMOV OUT[0], IMM[0]\nEND\n";
        let mut s = Submit3d::new();
        s.emit_create_shader(10, PIPE_SHADER_VERTEX, VS);
        s.emit_create_shader(11, PIPE_SHADER_FRAGMENT, FS);
        let bytes = s.as_bytes();
        let hdr = VirtioGpuSubmit3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_SUBMIT_3D),
            size: bytes.len() as u32,
            _padding: 0,
        };
        self.ctrl_submit_header_tail(&hdr, bytes)
    }

    /// Issue TRANSFER_TO_HOST_3D for a box of `res_id` (guest backing → host
    /// GL texture) and wait for completion.
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
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

    /// Lazily create the GPU blur pipeline. The source/destination texture
    /// ALIASES the framebuffer VMO's display planes (rows 1600..3199), so the
    /// blur is zero-copy: TRANSFER_TO_HOST syncs the region into the GL
    /// texture, two shader passes blur it (H into a scratch RT, V back), and
    /// TRANSFER_FROM_HOST lands the result directly in the scanned-out VMO.
    /// Reuses the boot self-test's blend/DSA/rasterizer/vertex-elements and
    /// vertex shader (context-persistent objects).
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) fn virgl_blur_init(&mut self) -> Result<(), GfxError> {
        use crate::protocol::{
            VirtioGpuCtxAttachResource, VirtioGpuMemEntry, VirtioGpuResourceAttachBacking,
            VirtioGpuResourceCreate3d, VirtioGpuSubmit3d, VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE,
            VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING, VIRTIO_GPU_CMD_RESOURCE_CREATE_3D,
            VIRTIO_GPU_CMD_SUBMIT_3D,
        };
        use crate::virgl::{
            Submit3d, PIPE_BIND_RENDER_TARGET, PIPE_BIND_SAMPLER_VIEW, PIPE_BIND_VERTEX_BUFFER,
            PIPE_BUFFER, PIPE_FORMAT_B8G8R8A8_UNORM, PIPE_FORMAT_R8_UNORM, PIPE_SHADER_FRAGMENT,
            PIPE_TEXTURE_2D,
        };
        // Scanout record carries the fb VMO physical base for the alias.
        let scanout = self.scanout_resource.ok_or(GfxError::DeviceNotFound)?;
        let record = self.find_resource(scanout).ok_or(GfxError::DeviceNotFound)?;
        let fb_pa = record.backing_pa;
        // Display planes: rows 1600..3199 of the 1280×3200 VMO.
        let alias_pa = fb_pa + 1600 * 5120;
        let alias_len = 1600u32 * 5120;

        // FBSRC: 1280×1600 texture aliasing the display planes.
        let create_src = VirtioGpuResourceCreate3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_CREATE_3D),
            resource_id: 0xF8,
            target: PIPE_TEXTURE_2D,
            format: PIPE_FORMAT_B8G8R8A8_UNORM,
            bind: PIPE_BIND_RENDER_TARGET | PIPE_BIND_SAMPLER_VIEW,
            width: 1280,
            height: 1600,
            depth: 1,
            array_size: 1,
            last_level: 0,
            nr_samples: 0,
            flags: 0,
            _padding: 0,
        };
        self.ctrl_submit_struct(&create_src)?;
        let attach = VirtioGpuResourceAttachBacking {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING),
            resource_id: 0xF8,
            nr_entries: 1,
        };
        let entry = VirtioGpuMemEntry { addr: alias_pa, length: alias_len, _padding: 0 };
        self.ctrl_submit_pair(&attach, &entry)?;
        let ctx_attach = VirtioGpuCtxAttachResource {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE),
            resource_id: 0xF8,
            _padding: 0,
        };
        self.ctrl_submit_struct(&ctx_attach)?;

        // TMP: 1280×800 scratch render target (host-side only, no backing).
        let create_tmp = VirtioGpuResourceCreate3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_CREATE_3D),
            resource_id: 0xF9,
            target: PIPE_TEXTURE_2D,
            format: PIPE_FORMAT_B8G8R8A8_UNORM,
            bind: PIPE_BIND_RENDER_TARGET | PIPE_BIND_SAMPLER_VIEW,
            width: 1280,
            height: 800,
            depth: 1,
            array_size: 1,
            last_level: 0,
            nr_samples: 0,
            flags: 0,
            _padding: 0,
        };
        self.ctrl_submit_struct(&create_tmp)?;
        let ctx_attach_tmp = VirtioGpuCtxAttachResource {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE),
            resource_id: 0xF9,
            _padding: 0,
        };
        self.ctrl_submit_struct(&ctx_attach_tmp)?;

        // QUAD: exact −1..1 quad (two triangles) so rasterization covers the
        // viewport box exactly — no scissor needed.
        let create_quad = VirtioGpuResourceCreate3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_CREATE_3D),
            resource_id: 0xFA,
            target: PIPE_BUFFER,
            format: PIPE_FORMAT_R8_UNORM,
            bind: PIPE_BIND_VERTEX_BUFFER,
            width: 96,
            height: 1,
            depth: 1,
            array_size: 1,
            last_level: 0,
            nr_samples: 0,
            flags: 0,
            _padding: 0,
        };
        self.ctrl_submit_struct(&create_quad)?;
        let ctx_attach_quad = VirtioGpuCtxAttachResource {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE),
            resource_id: 0xFA,
            _padding: 0,
        };
        self.ctrl_submit_struct(&ctx_attach_quad)?;

        let quad: [f32; 24] = [
            -1.0, -1.0, 0.0, 1.0, 1.0, -1.0, 0.0, 1.0, -1.0, 1.0, 0.0, 1.0, // tri 1
            1.0, -1.0, 0.0, 1.0, 1.0, 1.0, 0.0, 1.0, -1.0, 1.0, 0.0, 1.0, // tri 2
        ];
        let mut qbytes = [0u8; 96];
        for (i, v) in quad.iter().enumerate() {
            qbytes[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }

        // Separable gaussian fragment shader. CONST[0] = (inv_w, inv_h, radius,
        // k = -1/(2σ²·ln2)); CONST[1] = (dir_x, dir_y, origin_x, origin_y).
        // Per tap: weight = 2^(k·i²) (≡ exp(-i²/2σ²)); the weight sum
        // normalizes, matching the CPU reference in blur_backdrop_separable_vmo.
        const FS_BLUR: &str = "FRAG\n\
            DCL IN[0], POSITION, LINEAR\n\
            DCL OUT[0], COLOR\n\
            DCL SAMP[0]\n\
            DCL SVIEW[0], 2D, FLOAT\n\
            DCL CONST[0..1]\n\
            DCL TEMP[0..5]\n\
            DCL ADDR[0]\n\
            IMM[0] FLT32 { 0.0000, 1.0000, -1.0000, 0.5000}\n\
            MOV TEMP[0], IMM[0].xxxx\n\
            MOV TEMP[1].x, IMM[0].xxxx\n\
            MUL TEMP[2].x, CONST[0].zzzz, IMM[0].zzzz\n\
            BGNLOOP\n\
            SGT TEMP[3].x, TEMP[2].xxxx, CONST[0].zzzz\n\
            IF TEMP[3].xxxx\n\
            BRK\n\
            ENDIF\n\
            MUL TEMP[3].x, TEMP[2].xxxx, TEMP[2].xxxx\n\
            MUL TEMP[3].x, TEMP[3].xxxx, CONST[0].wwww\n\
            EX2 TEMP[3].x, TEMP[3].xxxx\n\
            MAD TEMP[4].xy, CONST[1].xyyy, TEMP[2].xxxx, IN[0].xyyy\n\
            ADD TEMP[4].xy, TEMP[4].xyyy, CONST[1].zwww\n\
            MUL TEMP[4].xy, TEMP[4].xyyy, CONST[0].xyyy\n\
            TEX TEMP[5], TEMP[4], SAMP[0], 2D\n\
            MAD TEMP[0], TEMP[5], TEMP[3].xxxx, TEMP[0]\n\
            ADD TEMP[1].x, TEMP[1].xxxx, TEMP[3].xxxx\n\
            ADD TEMP[2].x, TEMP[2].xxxx, IMM[0].yyyy\n\
            ENDLOOP\n\
            RCP TEMP[1].x, TEMP[1].xxxx\n\
            MUL OUT[0], TEMP[0], TEMP[1].xxxx\n\
            END\n";

        let mut s = Submit3d::new();
        s.emit_resource_inline_write(0xFA, &qbytes);
        s.emit_create_surface(0x30, 0xF8, PIPE_FORMAT_B8G8R8A8_UNORM);
        s.emit_create_surface(0x31, 0xF9, PIPE_FORMAT_B8G8R8A8_UNORM);
        s.emit_create_sampler_view(0x32, 0xF8, PIPE_FORMAT_B8G8R8A8_UNORM);
        s.emit_create_sampler_view(0x33, 0xF9, PIPE_FORMAT_B8G8R8A8_UNORM);
        s.emit_create_sampler_state_default(0x34);
        s.emit_create_shader(13, PIPE_SHADER_FRAGMENT, FS_BLUR);
        let bytes = s.as_bytes();
        let hdr = VirtioGpuSubmit3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_SUBMIT_3D),
            size: bytes.len() as u32,
            _padding: 0,
        };
        self.ctrl_submit_header_tail(&hdr, bytes)?;
        self.virgl_blur_ready = true;
        Ok(())
    }

    /// Two-pass separable gaussian blur on the GPU via virgl.
    ///
    /// `y` is the absolute framebuffer row (display offset already applied by
    /// the caller); the fb-alias texture covers rows 1600..3199. The CPU
    /// fallback in `blur_backdrop_separable_vmo` remains the parity reference —
    /// on the first GPU blur the result is compared against it (interior of
    /// the region, tolerance 2 LSB) and a parity marker is emitted.
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn submit_virgl_blur(
        &mut self,
        fb: *mut u8,
        fb_len: usize,
        fb_w: usize,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        radius: u32,
        // When false, the blurred result is left in the GL texture (0xF8) only
        // and NOT copied back to the VMO — the caller composites over 0xF8
        // directly (glass layer), saving a transfer per frame. The one-shot
        // boot parity check forces a writeback regardless.
        writeback: bool,
    ) -> Result<(), GfxError> {
        use crate::protocol::{VirtioGpuSubmit3d, VIRTIO_GPU_CMD_SUBMIT_3D};
        use crate::virgl::{
            Submit3d, PIPE_PRIM_TRIANGLES, PIPE_SHADER_FRAGMENT, PIPE_SHADER_VERTEX,
        };
        if !self.virgl_capable || !self.virgl_draw_ok || self.virgl_ctx_id == 0 {
            return Err(GfxError::DeviceNotFound);
        }
        if radius == 0 || w == 0 || h == 0 || fb_w != 1280 {
            return Err(GfxError::InvalidArgument);
        }
        // The alias texture covers display rows 1600..3199.
        if y < 1600 || x.saturating_add(w) > 1280 || (y - 1600).saturating_add(h) > 1600 {
            return Err(GfxError::InvalidArgument);
        }
        let y_rel = y - 1600;
        if !self.virgl_blur_ready {
            self.virgl_blur_init()?;
        }
        // First GPU-executed blur (init may have happened earlier via the GL
        // scanout bringup — the marker tracks first USE, not init).
        if !self.virgl_blur_first_done {
            self.virgl_blur_first_done = true;
            let _ = nexus_abi::debug_println(crate::markers::GPUD_VIRGL_BLUR_GPU_ON);
        }

        // One-shot parity: snapshot the region and CPU-blur it for comparison.
        let parity_buf: Option<(usize, usize)> =
            if !self.virgl_parity_done && (w as usize) * (h as usize) * 4 <= 1024 * 1024 {
                self.virgl_parity_done = true;
                match self.virgl_alloc_scratch((w as usize) * (h as usize) * 4) {
                    Ok(va) => {
                        // Tightly pack the region into the scratch and CPU-blur it.
                        for row in 0..h as usize {
                            let src_off = (y as usize + row) * fb_w * 4 + (x as usize) * 4;
                            if src_off + (w as usize) * 4 > fb_len {
                                break;
                            }
                            unsafe {
                                core::ptr::copy_nonoverlapping(
                                    fb.add(src_off),
                                    (va + row * (w as usize) * 4) as *mut u8,
                                    (w as usize) * 4,
                                );
                            }
                        }
                        let _ = blur_backdrop_separable_vmo(
                            va as *mut u8,
                            (w as usize) * (h as usize) * 4,
                            w as usize,
                            0,
                            0,
                            w,
                            h,
                            radius,
                            0,
                        );
                        Some((va, (w as usize) * 4))
                    }
                    Err(_) => None,
                }
            } else {
                None
            };

        // Sync the region from the guest VMO into the GL texture.
        self.virgl_transfer_to_host(0xF8, x, y_rel, w, h, 5120)?;

        let sigma = (radius as f32) / 2.0;
        let k = -1.0 / (2.0 * sigma * sigma * core::f32::consts::LN_2);
        let r = radius as f32;

        let mut s = Submit3d::new();
        // Rebind pipeline state explicitly — other passes (gl_scanout blit,
        // selftests) bind their own objects and virgl state is context-global.
        s.emit_bind_object(crate::virgl::VIRGL_OBJECT_BLEND, 0x20);
        s.emit_bind_object(crate::virgl::VIRGL_OBJECT_DSA, 0x21);
        s.emit_bind_object(crate::virgl::VIRGL_OBJECT_RASTERIZER, 0x22);
        s.emit_bind_object(crate::virgl::VIRGL_OBJECT_VERTEX_ELEMENTS, 0x23);
        // Pass 1: horizontal blur, FBSRC region → TMP at (0,0,w,h).
        s.emit_set_framebuffer_state(0, &[0x31]);
        s.emit_set_viewport_box(0.0, 0.0, w as f32, h as f32);
        s.emit_set_sampler_views(PIPE_SHADER_FRAGMENT, 0, &[0x32]);
        s.emit_bind_sampler_states(PIPE_SHADER_FRAGMENT, 0, &[0x34]);
        s.emit_set_constant_buffer(
            PIPE_SHADER_FRAGMENT,
            &[1.0 / 1280.0, 1.0 / 1600.0, r, k, 1.0, 0.0, x as f32, y_rel as f32],
        );
        s.emit_bind_shader(10, PIPE_SHADER_VERTEX);
        s.emit_bind_shader(13, PIPE_SHADER_FRAGMENT);
        s.emit_set_vertex_buffers(&[(16, 0, 0xFA)]);
        s.emit_draw_vbo(0, 6, PIPE_PRIM_TRIANGLES);
        // Pass 2: vertical blur, TMP (0,0,w,h) → FBSRC region.
        s.emit_set_framebuffer_state(0, &[0x30]);
        s.emit_set_viewport_box(x as f32, y_rel as f32, w as f32, h as f32);
        s.emit_set_sampler_views(PIPE_SHADER_FRAGMENT, 0, &[0x33]);
        s.emit_set_constant_buffer(
            PIPE_SHADER_FRAGMENT,
            &[1.0 / 1280.0, 1.0 / 800.0, r, k, 0.0, 1.0, -(x as f32), -(y_rel as f32)],
        );
        s.emit_draw_vbo(0, 6, PIPE_PRIM_TRIANGLES);
        let bytes = s.as_bytes();
        let hdr = VirtioGpuSubmit3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_SUBMIT_3D),
            size: bytes.len() as u32,
            _padding: 0,
        };
        self.ctrl_submit_header_tail(&hdr, bytes)?;

        // Land the blurred pixels back in the scanned-out guest VMO — unless the
        // caller will composite over 0xF8 directly (glass) and only the boot
        // parity check (which reads the VMO) needs it.
        if writeback || parity_buf.is_some() {
            self.virgl_transfer_from_host(0xF8, x, y_rel, w, h, 5120)?;
        }

        // Compare GPU result vs CPU reference over the interior.
        if let Some((ref_va, ref_stride)) = parity_buf {
            let inset = (radius + 1) as usize;
            let mut max_diff: u8 = 0;
            if (w as usize) > 2 * inset && (h as usize) > 2 * inset {
                for row in inset..(h as usize - inset) {
                    for col in inset..(w as usize - inset) {
                        let gpu_off = (y as usize + row) * fb_w * 4 + (x as usize + col) * 4;
                        let ref_off = row * ref_stride + col * 4;
                        for c in 0..3 {
                            let g = unsafe { fb.add(gpu_off + c).read_volatile() };
                            let r8 = unsafe { ((ref_va + ref_off + c) as *const u8).read() };
                            let d = g.abs_diff(r8);
                            if d > max_diff {
                                max_diff = d;
                            }
                        }
                    }
                }
                let _ = nexus_abi::debug_println(if max_diff <= 2 {
                    crate::markers::GPUD_VIRGL_BLUR_PARITY_OK
                } else {
                    crate::markers::GPUD_VIRGL_BLUR_PARITY_OFF
                });
            }
        }
        Ok(())
    }

    /// Allocate and map a page-aligned scratch VMO in the virgl backing VA
    /// region (no resource attach). Returns the VA.
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) fn virgl_alloc_scratch(&mut self, byte_len: usize) -> Result<usize, GfxError> {
        let slot = self.virgl_backing_count;
        if slot >= 8 {
            return Err(GfxError::ResourceExhausted);
        }
        let va = GPU_VIRGL_BACKING_BASE_VA + slot * GPU_VIRGL_BACKING_STRIDE;
        let len = align_page(byte_len);
        let vmo = nexus_abi::vmo_create(len).map_err(|_| GfxError::ResourceExhausted)?;
        let flags = nexus_abi::page_flags::VALID
            | nexus_abi::page_flags::USER
            | nexus_abi::page_flags::READ
            | nexus_abi::page_flags::WRITE;
        for offset in (0..len).step_by(4096) {
            nexus_abi::vmo_map_page(vmo, va + offset, offset, flags)
                .map_err(|_| GfxError::MmioFault)?;
        }
        self.virgl_backing_count = slot + 1;
        Ok(va)
    }

    /// Read the device feature bits and acknowledge the subset we support for
    /// virgl 3D. Must run after ACKNOWLEDGE|DRIVER and before FEATURES_OK.
    ///
    /// Sets `self.virgl_capable` iff the device offered `VIRTIO_GPU_F_VIRGL`.
    /// We ack VIRGL (3D), CONTEXT_INIT (so CTX_CREATE may select the VIRGL2
    /// capset), and VERSION_1 (modern virtio — the queue is already driven via
    /// the split DESC/DRIVER/DEVICE registers, so VERSION_1 is the correct mode).
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
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
