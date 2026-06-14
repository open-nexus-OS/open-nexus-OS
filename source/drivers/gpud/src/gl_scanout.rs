// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: GL-presented scanout (GPU compositor stages G0/G1).
//! OWNERS: @ui @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! RFC: docs/rfcs/RFC-0059-ui-v5a-animation-nexusgfx-sdk-gpu-driver-contract.md
//!
//! On a `virtio-gpu-gl` device the displayed scanout is a **virgl 3D render
//! target** that QEMU presents as a host GL texture (`dpy_gl_scanout_texture`
//! + flip on RESOURCE_FLUSH). The guest never 3D-renders into a 2D-scanned
//! resource (the two paths fight over the same host surface and the display
//! goes black — the bug this module exists to fix). Instead:
//!
//!   windowd VMO (CPU composite) ──TRANSFER_TO_HOST_3D──▶ display texture
//!     display texture ──fullscreen textured draw──▶ scanout RT
//!       scanout RT ──RESOURCE_FLUSH──▶ host GL flip (visible frame)
//!
//! This is the G1 bridge: CPU compositing stays authoritative while the
//! present itself is GPU-executed. Later stages (G2+) replace the VMO blit
//! with true GPU layer compositing into the same scanout RT.
//!
//! DriverKit boundary note: everything in this file is virtio/virgl command
//! encoding — the portable contract is `gl_scanout_init` / `gl_present_damage`
//! (init + present-damage), which a future real-GPU backend reimplements.

#![cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]

use nexus_gfx::backend::error::GfxError;
use nexus_gfx::backend::types::Rect;

use crate::backend::VirtioGpuBackend;
use crate::protocol::{
    self, VirtioGpuCtxAttachResource, VirtioGpuResourceCreate3d, VirtioGpuResourceFlush,
    VirtioGpuSetScanout, VirtioGpuSubmit3d, VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE,
    VIRTIO_GPU_CMD_RESOURCE_CREATE_3D, VIRTIO_GPU_CMD_RESOURCE_FLUSH, VIRTIO_GPU_CMD_SET_SCANOUT,
    VIRTIO_GPU_CMD_SUBMIT_3D,
};
use crate::virgl::{
    Submit3d, PIPE_BIND_RENDER_TARGET, PIPE_BIND_SAMPLER_VIEW, PIPE_BIND_SCANOUT,
    PIPE_CLEAR_COLOR0, PIPE_FORMAT_B8G8R8A8_UNORM, PIPE_PRIM_TRIANGLES, PIPE_SHADER_FRAGMENT,
    PIPE_SHADER_VERTEX, PIPE_TEXTURE_2D,
};

/// virtio resource id of the GL scanout render target.
const GL_SCANOUT_RES: u32 = 0xE0;
/// Surface handle for the scanout RT (context-scoped object namespace; the
/// blur path owns 0x30..0x34, selftests 0x20..0x24, gradient 0x30s on 0xF6).
const H_GLS_SURF: u32 = 0x42;
/// Fragment shader handle for the display-texture blit (VS is handle 10, the
/// boot self-test's passthrough vertex shader, which persists in the context).
const H_FS_BLIT: u32 = 14;
/// Passthrough vertex shader created by `virgl_shader_test` at bringup.
const H_VS: u32 = 10;
/// Sampler view of the display-plane texture (created by `virgl_blur_init`,
/// resource 0xF8 = fb VMO rows 1600..3199 → display plane is rows 0..800).
const H_SV_DISPLAY: u32 = 0x32;
/// Default sampler state (created by `virgl_blur_init`).
const H_SAMPLER: u32 = 0x34;
/// Fullscreen −1..1 quad VBO (resource 0xFA, created by `virgl_blur_init`).
const QUAD_RES: u32 = 0xFA;

const SCREEN_W: u32 = 1280;
const SCREEN_H: u32 = 800;
/// Height of the display texture 0xF8 (display plane + blur-cache plane).
const DISPLAY_TEX_H: u32 = 1600;
const FB_STRIDE: u32 = SCREEN_W * 4;

/// Single-tap textured blit: window position → display-texture UV via
/// CONST[0] = (scale_x, scale_y, offset_x, offset_y). The scale/offset form
/// lets the caller flip V without a second shader if the host orientation
/// ever requires it.
const FS_BLIT: &str = "FRAG\n\
    DCL IN[0], POSITION, LINEAR\n\
    DCL OUT[0], COLOR\n\
    DCL SAMP[0]\n\
    DCL SVIEW[0], 2D, FLOAT\n\
    DCL CONST[0]\n\
    DCL TEMP[0..1]\n\
    MAD TEMP[0].xy, IN[0].xyyy, CONST[0].xyyy, CONST[0].zwww\n\
    TEX TEMP[1], TEMP[0], SAMP[0], 2D\n\
    MOV OUT[0], TEMP[1]\n\
    END\n";

impl VirtioGpuBackend {
    /// G0: create the GL scanout render target, point the display at it, and
    /// prove the GPU can put pixels on it (clear + flush). Requires the virgl
    /// draw pipeline (boot self-tests green) and the framebuffer handoff
    /// (display texture aliasing needs the fb VMO's physical base).
    pub(crate) fn gl_scanout_init(&mut self) -> Result<(), GfxError> {
        if !self.virgl_capable || !self.virgl_draw_ok || self.virgl_ctx_id == 0 {
            return Err(GfxError::DeviceNotFound);
        }
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
            width: SCREEN_W,
            height: SCREEN_H,
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
        self.gl_scanout_backing_va =
            self.virgl_attach_backing(GL_SCANOUT_RES, (SCREEN_W * SCREEN_H * 4) as usize)?;

        // Surface + blit fragment shader (vertex shader 10 persists from boot).
        let mut s = Submit3d::new();
        s.emit_create_surface(H_GLS_SURF, GL_SCANOUT_RES, PIPE_FORMAT_B8G8R8A8_UNORM);
        s.emit_create_shader(H_FS_BLIT, PIPE_SHADER_FRAGMENT, FS_BLIT);
        // G0 proof: GPU-clear the scanout RT so the first flip shows GPU output
        // (dark slate, replaced by the real UI on the first present).
        s.emit_set_framebuffer_state(0, &[H_GLS_SURF]);
        s.emit_set_viewport_box(0.0, 0.0, SCREEN_W as f32, SCREEN_H as f32);
        s.emit_clear(PIPE_CLEAR_COLOR0, [0.09, 0.10, 0.12, 1.0], 1.0, 0);
        let bytes = s.as_bytes();
        let hdr = VirtioGpuSubmit3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_SUBMIT_3D),
            size: bytes.len() as u32,
            _padding: 0,
        };
        self.ctrl_submit_header_tail(&hdr, &bytes)?;

        // Point the display at the GL RT (full resource — no plane-row window;
        // the 2D path's tall-VMO row addressing ends here).
        let scanout = VirtioGpuSetScanout {
            hdr: crate::backend::ctrl_hdr(VIRTIO_GPU_CMD_SET_SCANOUT),
            r: protocol::VirtioGpuRect { x: 0, y: 0, width: SCREEN_W, height: SCREEN_H },
            scanout_id: 0,
            resource_id: GL_SCANOUT_RES,
        };
        self.ctrl_submit_struct(&scanout)?;
        self.gl_flush_rect(Rect { x: 0, y: 0, width: SCREEN_W, height: SCREEN_H })?;

        self.gl_scanout_active = true;
        let _ = nexus_abi::debug_println(crate::markers::GPUD_GL_SCANOUT_OK);
        Ok(())
    }

    /// G1 present: sync the damaged display-plane region from the windowd VMO
    /// into the display texture, GPU-blit it into the scanout RT, and flush
    /// (host GL flip). `rect` is screen-relative (0..800).
    pub(crate) fn gl_present_damage(&mut self, rect: Rect) -> Result<(), GfxError> {
        if !self.gl_scanout_active {
            return Err(GfxError::DeviceNotFound);
        }
        let x = rect.x.min(SCREEN_W);
        let y = rect.y.min(SCREEN_H);
        let w = rect.width.min(SCREEN_W - x);
        let h = rect.height.min(SCREEN_H - y);
        if w == 0 || h == 0 {
            return Ok(());
        }
        // Display plane row 0 == display texture row 0 (0xF8 aliases fb rows
        // 1600..3199, and the display plane starts at fb row 1600).
        self.virgl_transfer_to_host(0xF8, x, y, w, h, FB_STRIDE)?;

        let mut s = Submit3d::new();
        // Pipeline state is context-global and other passes (selftests, blur)
        // rebind their own objects — never rely on leftovers. 0x20..0x23 are
        // the boot draw-selftest's blend/DSA/rasterizer/vertex-elements
        // (single vec4 position attribute, stride 16 — matches QUAD_RES).
        s.emit_bind_object(crate::virgl::VIRGL_OBJECT_BLEND, 0x20);
        s.emit_bind_object(crate::virgl::VIRGL_OBJECT_DSA, 0x21);
        s.emit_bind_object(crate::virgl::VIRGL_OBJECT_RASTERIZER, 0x22);
        s.emit_bind_object(crate::virgl::VIRGL_OBJECT_VERTEX_ELEMENTS, 0x23);
        s.emit_set_framebuffer_state(0, &[H_GLS_SURF]);
        s.emit_set_viewport_box(x as f32, y as f32, w as f32, h as f32);
        s.emit_set_sampler_views(PIPE_SHADER_FRAGMENT, 0, &[H_SV_DISPLAY]);
        s.emit_bind_sampler_states(PIPE_SHADER_FRAGMENT, 0, &[H_SAMPLER]);
        s.emit_set_constant_buffer(
            PIPE_SHADER_FRAGMENT,
            &[1.0 / SCREEN_W as f32, 1.0 / DISPLAY_TEX_H as f32, 0.0, 0.0],
        );
        s.emit_bind_shader(H_VS, PIPE_SHADER_VERTEX);
        s.emit_bind_shader(H_FS_BLIT, PIPE_SHADER_FRAGMENT);
        s.emit_set_vertex_buffers(&[(16, 0, QUAD_RES)]);
        s.emit_draw_vbo(0, 6, PIPE_PRIM_TRIANGLES);
        let bytes = s.as_bytes();
        let hdr = VirtioGpuSubmit3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_SUBMIT_3D),
            size: bytes.len() as u32,
            _padding: 0,
        };
        self.ctrl_submit_header_tail(&hdr, &bytes)?;

        // One-shot on-device proof: read the scanout RT back and compare a
        // pixel sample against the source plane (G1 marker).
        if !self.gl_present_parity_done {
            self.gl_present_parity_done = true;
            self.gl_present_parity_check();
            let _ = nexus_abi::debug_println(crate::markers::GPUD_GL_PRESENT_OK);
        }

        self.gl_flush_rect(Rect { x, y, width: w, height: h })
    }

    /// RESOURCE_FLUSH on the scanout RT — on virtio-gpu-gl this triggers the
    /// host display update (`dpy_gl_update`), i.e. the visible flip.
    fn gl_flush_rect(&mut self, rect: Rect) -> Result<(), GfxError> {
        let flush = VirtioGpuResourceFlush {
            hdr: crate::backend::ctrl_hdr(VIRTIO_GPU_CMD_RESOURCE_FLUSH),
            r: protocol::VirtioGpuRect {
                x: rect.x,
                y: rect.y,
                width: rect.width,
                height: rect.height,
            },
            resource_id: GL_SCANOUT_RES,
            _padding: 0,
        };
        self.ctrl_submit_struct(&flush)
    }

    /// Compare a sparse pixel grid of the scanout RT against the windowd VMO's
    /// display plane. Detects both content mismatch and a vertically flipped
    /// blit (the classic GL FBO orientation trap) and reports via markers.
    fn gl_present_parity_check(&mut self) {
        let Ok(()) = self.virgl_transfer_from_host(GL_SCANOUT_RES, 0, 0, SCREEN_W, SCREEN_H, FB_STRIDE)
        else {
            let _ = nexus_abi::debug_println(crate::markers::GPUD_GL_PRESENT_PARITY_OFF);
            return;
        };
        let Some((fb, fb_len, fb_w, display_row)) = self.scanout_fb() else {
            return;
        };
        let rt = self.gl_scanout_backing_va as *const u8;
        let mut same = 0u32;
        let mut flipped = 0u32;
        let mut total = 0u32;
        for gy in (40..SCREEN_H as usize - 40).step_by(97) {
            for gx in (40..SCREEN_W as usize - 40).step_by(101) {
                let rt_off = (gy * SCREEN_W as usize + gx) * 4;
                let src_off = ((display_row as usize + gy) * fb_w + gx) * 4;
                let flip_off =
                    ((display_row as usize + (SCREEN_H as usize - 1 - gy)) * fb_w + gx) * 4;
                if src_off + 4 > fb_len || flip_off + 4 > fb_len {
                    continue;
                }
                total += 1;
                let mut d_same = 0u8;
                let mut d_flip = 0u8;
                for c in 0..3 {
                    let r = unsafe { rt.add(rt_off + c).read_volatile() };
                    let s = unsafe { fb.add(src_off + c).read_volatile() };
                    let f = unsafe { fb.add(flip_off + c).read_volatile() };
                    d_same = d_same.max(r.abs_diff(s));
                    d_flip = d_flip.max(r.abs_diff(f));
                }
                if d_same <= 2 {
                    same += 1;
                }
                if d_flip <= 2 {
                    flipped += 1;
                }
            }
        }
        let _ = nexus_abi::debug_println(if total > 0 && same * 10 >= total * 9 {
            crate::markers::GPUD_GL_PRESENT_PARITY_OK
        } else if total > 0 && flipped * 10 >= total * 9 {
            crate::markers::GPUD_GL_PRESENT_FLIPPED
        } else {
            crate::markers::GPUD_GL_PRESENT_PARITY_OFF
        });
    }
}
