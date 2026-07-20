// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: GL-scanout bring-up (G0) — creates the double-buffered swapchain
//! RTs (A/B), the display/wallpaper textures, the blit shaders, paints the
//! splash and points the display at RT A. Split out of `gl_scanout.rs`
//! (structure-gate); the present half stays there.
//! OWNERS: @ui @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable

#![cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]

use nexus_gfx::backend::error::GfxError;
use nexus_gfx::backend::types::Rect;

use crate::backend::{compose_splash_region, VirtioGpuBackend};
use crate::gl_scanout::{
    FS_BLIT, FS_BLIT_TINT, GL_SCANOUT_RES, GL_SCANOUT_RES_B, H_DISPLAY_TEX, H_FS_BLIT,
    H_FS_BLIT_TINT, H_GLS_SURF, H_GLS_SURF_B, H_SAMPLER, H_SV_DISPLAY_TEX, H_SV_WALLPAPER, H_VS,
    H_WALLPAPER_TEX, QUAD_RES,
};
use crate::protocol::{
    self, VirtioGpuCtxAttachResource, VirtioGpuResourceCreate3d, VirtioGpuSetScanout,
    VirtioGpuSubmit3d, VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE, VIRTIO_GPU_CMD_RESOURCE_CREATE_3D,
    VIRTIO_GPU_CMD_SET_SCANOUT, VIRTIO_GPU_CMD_SUBMIT_3D,
};
use crate::virgl::{
    Submit3d, PIPE_BIND_RENDER_TARGET, PIPE_BIND_SAMPLER_VIEW, PIPE_BIND_SCANOUT,
    PIPE_CLEAR_COLOR0, PIPE_FORMAT_B8G8R8A8_UNORM, PIPE_PRIM_TRIANGLES, PIPE_SHADER_FRAGMENT,
    PIPE_SHADER_VERTEX, PIPE_TEXTURE_2D,
};

impl VirtioGpuBackend {
    /// G0: create the GL scanout render target, point the display at it, and
    /// prove the GPU can put pixels on it (clear + flush). Requires the virgl
    /// draw pipeline (boot self-tests green) and the framebuffer handoff
    /// (display texture aliasing needs the fb VMO's physical base).
    pub(crate) fn gl_scanout_init(&mut self) -> Result<(), GfxError> {
        if !self.virgl_capable || !self.virgl_draw_ok || self.virgl_ctx_id == 0 {
            return Err(GfxError::DeviceNotFound);
        }
        // Batch the whole GL-scanout bring-up (~49 virgl commands, incl. virgl_blur_init's):
        // enqueue them onto the DriverKit ring WITHOUT a per-command wait. virgl processes the
        // ring IN ORDER, so resource/shader creation still precedes the draws that use them.
        // Previously each command blocked up to GPU_WAIT_DEADLINE_NS (500ms) on QEMU's deferred
        // used-ring advance — ~49 × 500ms froze the bootsplash for ~12s. This is the same
        // pipelining the present already uses; the init path was never migrated onto it.
        // `ctrl_batch_end` runs even on the error path so a failed init can't leave the backend
        // stuck in batch mode for the 2D fallback's synchronous commands.
        self.ctrl_batch_begin();
        let result = self.gl_scanout_init_batched();
        let _ = self.ctrl_batch_end();
        result
    }

    fn gl_scanout_init_batched(&mut self) -> Result<(), GfxError> {
        // The display texture (0xF8), quad (0xFA), sampler view/state and the
        // blit's source plumbing are shared with the blur pipeline.
        if !self.virgl_blur_ready {
            self.virgl_blur_init()?;
        }

        // Scanout RT: host GL texture QEMU can present directly. Guest backing
        // is attached for the one-shot present parity readback (G1 proof).
        let create = VirtioGpuResourceCreate3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_CREATE_3D),
            resource_id: GL_SCANOUT_RES,
            target: PIPE_TEXTURE_2D,
            format: PIPE_FORMAT_B8G8R8A8_UNORM,
            bind: PIPE_BIND_RENDER_TARGET | PIPE_BIND_SAMPLER_VIEW | PIPE_BIND_SCANOUT,
            width: self.display_w,
            height: self.display_h,
            depth: 1,
            array_size: 1,
            last_level: 0,
            nr_samples: 0,
            flags: 0,
            _padding: 0,
        };
        self.ctrl_submit_struct(&create)?;
        let ctx_attach = VirtioGpuCtxAttachResource {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE),
            resource_id: GL_SCANOUT_RES,
            _padding: 0,
        };
        self.ctrl_submit_struct(&ctx_attach)?;
        self.gl_scanout_backing_va = self
            .virgl_attach_backing(GL_SCANOUT_RES, (self.display_w * self.display_h * 4) as usize)?;

        // Swapchain RT B — identical twin of A (same bind flags incl. SCANOUT so
        // SET_SCANOUT accepts it). Every present renders into the back RT and
        // flips; the splash below stays on A (the initial front).
        let create_b = VirtioGpuResourceCreate3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_CREATE_3D),
            resource_id: GL_SCANOUT_RES_B,
            target: PIPE_TEXTURE_2D,
            format: PIPE_FORMAT_B8G8R8A8_UNORM,
            bind: PIPE_BIND_RENDER_TARGET | PIPE_BIND_SAMPLER_VIEW | PIPE_BIND_SCANOUT,
            width: self.display_w,
            height: self.display_h,
            depth: 1,
            array_size: 1,
            last_level: 0,
            nr_samples: 0,
            flags: 0,
            _padding: 0,
        };
        self.ctrl_submit_struct(&create_b)?;
        let ctx_attach_b = VirtioGpuCtxAttachResource {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE),
            resource_id: GL_SCANOUT_RES_B,
            _padding: 0,
        };
        self.ctrl_submit_struct(&ctx_attach_b)?;
        self.gl_swap.b_backing_va = self.virgl_attach_backing(
            GL_SCANOUT_RES_B,
            (self.display_w * self.display_h * 4) as usize,
        )?;

        // NON-ALIASED display texture (1280×800, own backing — NOT a VMO alias).
        // The present copies windowd's composed frame here and blits it to the RT;
        // sampling this in the scanout draw presents (unlike the 0xF8 VMO-alias).
        let create_dt = VirtioGpuResourceCreate3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_CREATE_3D),
            resource_id: H_DISPLAY_TEX,
            target: PIPE_TEXTURE_2D,
            format: PIPE_FORMAT_B8G8R8A8_UNORM,
            bind: PIPE_BIND_SAMPLER_VIEW,
            width: self.display_w,
            height: self.display_h,
            depth: 1,
            array_size: 1,
            last_level: 0,
            nr_samples: 0,
            flags: 0,
            _padding: 0,
        };
        self.ctrl_submit_struct(&create_dt)?;
        let dt_ctx = VirtioGpuCtxAttachResource {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE),
            resource_id: H_DISPLAY_TEX,
            _padding: 0,
        };
        self.ctrl_submit_struct(&dt_ctx)?;
        self.gl_display_tex_va = self
            .virgl_attach_backing(H_DISPLAY_TEX, (self.display_w * self.display_h * 4) as usize)?;

        // Wallpaper texture: created here and seeded with recognizable BGRA color
        // bands as a boot fallback (proves "a sampled texture renders"). The first
        // build-up present replaces this content with the real wallpaper from VMO
        // Plane 0 (see `try_upload_wallpaper_from_vmo`) — no per-frame transfer.
        let create_wp = VirtioGpuResourceCreate3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_CREATE_3D),
            resource_id: H_WALLPAPER_TEX,
            target: PIPE_TEXTURE_2D,
            format: PIPE_FORMAT_B8G8R8A8_UNORM,
            bind: PIPE_BIND_SAMPLER_VIEW,
            width: self.display_w,
            height: self.display_h,
            depth: 1,
            array_size: 1,
            last_level: 0,
            nr_samples: 0,
            flags: 0,
            _padding: 0,
        };
        self.ctrl_submit_struct(&create_wp)?;
        let wp_ctx = VirtioGpuCtxAttachResource {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE),
            resource_id: H_WALLPAPER_TEX,
            _padding: 0,
        };
        self.ctrl_submit_struct(&wp_ctx)?;
        let wp_va = self.virgl_attach_backing(
            H_WALLPAPER_TEX,
            (self.display_w * self.display_h * 4) as usize,
        )?;
        // Remember the backing so the first build-up present can fill it with the real
        // wallpaper (windowd's decoded JPEG in VMO Plane 0, via try_upload_wallpaper_from_vmo).
        self.gl_wallpaper_tex_va = wp_va as usize;
        // Seed with the boot splash (brand radial glow + wordmark) via the ONE
        // shared compose in `backend::bootstrap` — the identical image the 2D
        // bootstrap scanout already shows (task #122), so the scanout switch to
        // GL is visually seamless. The single frame before the real wallpaper
        // lands reads as one uniform splash → desktop, never a fallback pattern.
        {
            let len = self.display_w as usize * self.display_h as usize * 4;
            let dst = unsafe { core::slice::from_raw_parts_mut(wp_va as *mut u8, len) };
            compose_splash_region(
                dst,
                self.display_w,
                self.display_h,
                0,
                0,
                self.display_w,
                self.display_h,
                256,
            );
        }
        self.virgl_transfer_to_host(
            H_WALLPAPER_TEX,
            0,
            0,
            self.display_w,
            self.display_h,
            self.display_w * 4,
        )?;

        // Surface + blit fragment shader (vertex shader 10 persists from boot).
        let mut s = Submit3d::new();
        s.emit_create_surface(H_GLS_SURF, GL_SCANOUT_RES, PIPE_FORMAT_B8G8R8A8_UNORM);
        s.emit_create_surface(H_GLS_SURF_B, GL_SCANOUT_RES_B, PIPE_FORMAT_B8G8R8A8_UNORM);
        s.emit_create_sampler_view(H_SV_DISPLAY_TEX, H_DISPLAY_TEX, PIPE_FORMAT_B8G8R8A8_UNORM);
        s.emit_create_sampler_view(H_SV_WALLPAPER, H_WALLPAPER_TEX, PIPE_FORMAT_B8G8R8A8_UNORM);
        s.emit_create_shader(H_FS_BLIT, PIPE_SHADER_FRAGMENT, FS_BLIT);
        s.emit_create_shader(H_FS_BLIT_TINT, PIPE_SHADER_FRAGMENT, FS_BLIT_TINT);
        // G0 proof: GPU-clear the scanout RT so the first flip shows GPU output
        // (dark slate, replaced by the real UI on the first present).
        s.emit_set_framebuffer_state(0, &[H_GLS_SURF]);
        s.emit_set_viewport_box(0.0, 0.0, self.display_w as f32, self.display_h as f32);
        // Initial GPU clear so the first flip shows GPU output (dark slate).
        s.emit_clear(PIPE_CLEAR_COLOR0, [0.09, 0.10, 0.12, 1.0], 1.0, 0);
        let bytes = s.as_bytes();
        let hdr = VirtioGpuSubmit3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_SUBMIT_3D),
            size: bytes.len() as u32,
            _padding: 0,
        };
        self.ctrl_submit_header_tail(&hdr, bytes)?;

        // Paint the splash INTO the RT before the scanout switches to it, so the
        // first frame the display ever shows is the branded glow + wordmark —
        // never the bare clear above. Same fullscreen wallpaper-tex blit as the
        // build-up present's stage-1 (the texture was seeded with the splash image
        // earlier in this fn); enqueued in the same batch, so ring order
        // guarantees it lands before SET_SCANOUT below. Tinted with the shared
        // pulse curve so this frame is phase-continuous with the 2D text pulse
        // before it and the hold-phase breathing after it.
        let splash_f =
            crate::backend::splash_pulse_q8(nexus_abi::nsec().unwrap_or(0)) as f32 / 256.0;
        let mut sp = Submit3d::new();
        sp.emit_bind_object(crate::virgl::VIRGL_OBJECT_BLEND, 0x20);
        sp.emit_bind_object(crate::virgl::VIRGL_OBJECT_DSA, 0x21);
        sp.emit_bind_object(crate::virgl::VIRGL_OBJECT_RASTERIZER, 0x22);
        sp.emit_bind_object(crate::virgl::VIRGL_OBJECT_VERTEX_ELEMENTS, 0x23);
        sp.emit_set_framebuffer_state(0, &[H_GLS_SURF]);
        sp.emit_set_viewport_box(0.0, 0.0, self.display_w as f32, self.display_h as f32);
        sp.emit_set_sampler_views(PIPE_SHADER_FRAGMENT, 0, &[H_SV_WALLPAPER]);
        sp.emit_bind_sampler_states(PIPE_SHADER_FRAGMENT, 0, &[H_SAMPLER]);
        sp.emit_set_constant_buffer(
            PIPE_SHADER_FRAGMENT,
            &[
                1.0 / self.display_w as f32,
                1.0 / self.display_h as f32,
                0.0,
                0.0,
                splash_f,
                splash_f,
                splash_f,
                1.0,
            ],
        );
        sp.emit_bind_shader(H_VS, PIPE_SHADER_VERTEX);
        sp.emit_bind_shader(H_FS_BLIT_TINT, PIPE_SHADER_FRAGMENT);
        sp.emit_set_vertex_buffers(&[(16, 0, QUAD_RES)]);
        sp.emit_draw_vbo(0, 6, PIPE_PRIM_TRIANGLES);
        let sp_bytes = sp.as_bytes();
        let sp_hdr = VirtioGpuSubmit3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_SUBMIT_3D),
            size: sp_bytes.len() as u32,
            _padding: 0,
        };
        self.ctrl_submit_header_tail(&sp_hdr, sp_bytes)?;

        // Point the display at the GL RT (full resource — no plane-row window;
        // the 2D path's tall-VMO row addressing ends here).
        let scanout = VirtioGpuSetScanout {
            hdr: crate::backend::ctrl_hdr(VIRTIO_GPU_CMD_SET_SCANOUT),
            r: protocol::VirtioGpuRect {
                x: 0,
                y: 0,
                width: self.display_w,
                height: self.display_h,
            },
            scanout_id: 0,
            resource_id: GL_SCANOUT_RES,
        };
        self.ctrl_submit_struct(&scanout)?;
        self.gl_flush_rect(Rect { x: 0, y: 0, width: self.display_w, height: self.display_h })?;

        self.gl_scanout_active = true;
        let _ = nexus_abi::debug_println(crate::markers::GPUD_GL_SCANOUT_OK);
        Ok(())
    }
}
