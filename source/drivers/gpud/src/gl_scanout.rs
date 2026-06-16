// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: GL-presented scanout (GPU compositor stages G0/G1).
//! OWNERS: @ui @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! RFC: docs/rfcs/RFC-0059-ui-v5a-animation-nexusgfx-sdk-gpu-driver-contract.md
//! ADR: docs/adr/0032-gpu-command-ring-and-pipelined-present.md — `compositor_buildup_present`
//!   is PIPELINED: it enqueues every SUBMIT_3D draw + the flush into the ring and
//!   `ctrl_batch_end`s WITHOUT blocking; the next frame harvests this frame's
//!   completion (fixes the texture-sampling stall). Hop markers G3b/G3c.
//! TESTS: `tools/nx` `chain_gpu_scanout.rs::chain_gpu_batched_present_hops_in_order`;
//!   `scripts/qemu-test.sh GPU_MODE=virgl` boot proof.
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
pub(crate) const H_GLS_SURF: u32 = 0x42;
/// Fragment shader handle for the display-texture blit (VS is handle 10, the
/// boot self-test's passthrough vertex shader, which persists in the context).
const H_FS_BLIT: u32 = 14;
/// Passthrough vertex shader created by `virgl_shader_test` at bringup.
const H_VS: u32 = 10;
/// Sampler view of the display-plane texture (created by `virgl_blur_init`,
/// resource 0xF8 = fb VMO rows 1600..3199 → display plane is rows 0..800).
const H_SV_DISPLAY: u32 = 0x32;
/// NON-ALIASED display texture: a 1280×800 GL texture with its OWN backing (not
/// a VMO alias). The present copies windowd's composed frame into its backing,
/// uploads it, and blits it to the scanout RT. Unlike the 0xF8 VMO-alias, QEMU
/// presents a draw that samples this to the display (the black-screen fix).
const H_DISPLAY_TEX: u32 = 0xE1;
/// Sampler view of H_DISPLAY_TEX.
const H_SV_DISPLAY_TEX: u32 = 0x44;
/// Stage-2 wallpaper texture: a 1280×800 GL texture uploaded ONCE at init (no
/// per-frame transfer) to test whether sampling a pre-uploaded texture presents.
const H_WALLPAPER_TEX: u32 = 0xE2;
/// Sampler view of H_WALLPAPER_TEX.
const H_SV_WALLPAPER: u32 = 0x45;
/// Default sampler state (created by `virgl_blur_init`).
const H_SAMPLER: u32 = 0x34;
/// Fullscreen −1..1 quad VBO (resource 0xFA, created by `virgl_blur_init`).
const QUAD_RES: u32 = 0xFA;

const SCREEN_W: u32 = 1280;
const SCREEN_H: u32 = 800;

/// Incremental GPU compositor build-up. From the confirmed-working base (solid
/// clear + gradient panel — pure GL draws that DO present), add ONE feature per
/// `COMPOSITOR_STAGE` (shadow → wallpaper texture → blur → …) and check after
/// each whether the display still presents. The first stage that goes black is
/// the op QEMU's GL-scanout present can't handle — and every working stage is a
/// real compositor feature. While `true`, the present renders this synthetic
/// scene instead of windowd's VMO content.
pub(crate) const COMPOSITOR_BUILDUP: bool = true;
/// Features added on top of the base (0 = clear + gradient only).
/// 1 = + drop shadow. 2 = + wallpaper texture. 3 = + glass blur. 4 = + cursor (input).
const COMPOSITOR_STAGE: u32 = 4;
/// Automated spin-blur demo: when true, an idle gpud re-presents the *orbiting*
/// build-up panel (shadow + glass blur) every frame to exercise the GPU blur/shadow
/// pipeline + the reactive ring-buffer IRQ at the 120 Hz target. The re-present is
/// driven by a recv timeout on gpud's server endpoint (the kernel's timer IRQ wakes
/// the timed-out recv via `wake_expired_ipc_deadlines`), NOT a timer cap on that
/// endpoint — an earlier timer-cap attempt intercepted windowd's present commands
/// and OOM'd the channel.
//
// The virgl glass-blur G3-exec stall ([[virgl-blur-g3-exec-flaky-hang]]) is being
// debugged, not worked around: it is intermittent and independent of this flag (it
// hits the FIRST windowd present-damage before the spin runs; spin OFF still stalls).
// Keep the orbit on so the perf test exercises the blur once the stall is fixed.
pub(crate) const BUILDUP_SPIN_DEMO: bool = true;
/// Integer cos/sin LUT (16 steps, amplitude 48 px) for the spin orbit — avoids any
/// float trig in the present hot path. `[dx, dy]` per step.
const SPIN_ORBIT_LUT: [(i32, i32); 16] = [
    (48, 0),
    (44, 18),
    (34, 34),
    (18, 44),
    (0, 48),
    (-18, 44),
    (-34, 34),
    (-44, 18),
    (-48, 0),
    (-44, -18),
    (-34, -34),
    (-18, -44),
    (0, -48),
    (18, -44),
    (34, -34),
    (44, -18),
];
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

        // NON-ALIASED display texture (1280×800, own backing — NOT a VMO alias).
        // The present copies windowd's composed frame here and blits it to the RT;
        // sampling this in the scanout draw presents (unlike the 0xF8 VMO-alias).
        let create_dt = VirtioGpuResourceCreate3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_CREATE_3D),
            resource_id: H_DISPLAY_TEX,
            target: PIPE_TEXTURE_2D,
            format: PIPE_FORMAT_B8G8R8A8_UNORM,
            bind: PIPE_BIND_SAMPLER_VIEW,
            width: SCREEN_W,
            height: SCREEN_H,
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
        self.gl_display_tex_va =
            self.virgl_attach_backing(H_DISPLAY_TEX, (SCREEN_W * SCREEN_H * 4) as usize)?;

        // Stage-2 wallpaper texture: uploaded ONCE here (no per-frame transfer).
        // Filled with recognizable BGRA color bands so "a sampled texture renders"
        // is unmistakable; tests whether sampling a pre-uploaded texture presents.
        let create_wp = VirtioGpuResourceCreate3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_CREATE_3D),
            resource_id: H_WALLPAPER_TEX,
            target: PIPE_TEXTURE_2D,
            format: PIPE_FORMAT_B8G8R8A8_UNORM,
            bind: PIPE_BIND_SAMPLER_VIEW,
            width: SCREEN_W,
            height: SCREEN_H,
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
        let wp_va = self.virgl_attach_backing(H_WALLPAPER_TEX, (SCREEN_W * SCREEN_H * 4) as usize)?;
        {
            let dst = wp_va as *mut u8;
            const BANDS: [[u8; 4]; 8] = [
                [40, 40, 220, 255],   // red    (BGRA)
                [40, 150, 240, 255],  // orange
                [40, 220, 240, 255],  // yellow
                [60, 200, 60, 255],   // green
                [200, 200, 40, 255],  // cyan
                [220, 80, 40, 255],   // blue
                [220, 40, 200, 255],  // magenta
                [240, 240, 240, 255], // white
            ];
            for y in 0..SCREEN_H as usize {
                let c = BANDS[(y * 8 / SCREEN_H as usize).min(7)];
                for x in 0..SCREEN_W as usize {
                    let off = (y * SCREEN_W as usize + x) * 4;
                    unsafe {
                        dst.add(off).write_volatile(c[0]);
                        dst.add(off + 1).write_volatile(c[1]);
                        dst.add(off + 2).write_volatile(c[2]);
                        dst.add(off + 3).write_volatile(c[3]);
                    }
                }
            }
        }
        self.virgl_transfer_to_host(H_WALLPAPER_TEX, 0, 0, SCREEN_W, SCREEN_H, FB_STRIDE)?;

        // Surface + blit fragment shader (vertex shader 10 persists from boot).
        let mut s = Submit3d::new();
        s.emit_create_surface(H_GLS_SURF, GL_SCANOUT_RES, PIPE_FORMAT_B8G8R8A8_UNORM);
        s.emit_create_sampler_view(H_SV_DISPLAY_TEX, H_DISPLAY_TEX, PIPE_FORMAT_B8G8R8A8_UNORM);
        s.emit_create_sampler_view(H_SV_WALLPAPER, H_WALLPAPER_TEX, PIPE_FORMAT_B8G8R8A8_UNORM);
        s.emit_create_shader(H_FS_BLIT, PIPE_SHADER_FRAGMENT, FS_BLIT);
        // G0 proof: GPU-clear the scanout RT so the first flip shows GPU output
        // (dark slate, replaced by the real UI on the first present).
        s.emit_set_framebuffer_state(0, &[H_GLS_SURF]);
        s.emit_set_viewport_box(0.0, 0.0, SCREEN_W as f32, SCREEN_H as f32);
        // Initial GPU clear so the first flip shows GPU output (dark slate).
        s.emit_clear(PIPE_CLEAR_COLOR0, [0.09, 0.10, 0.12, 1.0], 1.0, 0);
        let bytes = s.as_bytes();
        let hdr = VirtioGpuSubmit3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_SUBMIT_3D),
            size: bytes.len() as u32,
            _padding: 0,
        };
        self.ctrl_submit_header_tail(&hdr, bytes)?;

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
        if COMPOSITOR_BUILDUP {
            return self.compositor_buildup_present();
        }
        let x = rect.x.min(SCREEN_W);
        let y = rect.y.min(SCREEN_H);
        let w = rect.width.min(SCREEN_W - x);
        let h = rect.height.min(SCREEN_H - y);
        if w == 0 || h == 0 {
            return Ok(());
        }
        // Copy windowd's VMO display-plane damage (rows 1600+) into the
        // NON-ALIASED display texture's own backing (rows 0+), then upload it.
        // Sampling a VMO-aliased texture (0xF8) in the scanout draw does NOT
        // present on the GL scanout (confirmed black); a texture with its own
        // backing does. This is the black-screen fix.
        if let Some((fb, fb_len, fb_w, display_row)) = self.scanout_fb() {
            let stride = fb_w * 4;
            let dst = self.gl_display_tex_va as *mut u8;
            if !dst.is_null() {
                for row in 0..h as usize {
                    let src_off =
                        (display_row as usize + y as usize + row) * stride + x as usize * 4;
                    let dst_off = (y as usize + row) * (SCREEN_W as usize * 4) + x as usize * 4;
                    let len = w as usize * 4;
                    if src_off + len <= fb_len {
                        unsafe {
                            core::ptr::copy_nonoverlapping(
                                fb.add(src_off),
                                dst.add(dst_off),
                                len,
                            );
                        }
                    }
                }
            }
        }
        self.virgl_transfer_to_host(H_DISPLAY_TEX, x, y, w, h, FB_STRIDE)
            .map_err(|e| {
                let _ = nexus_abi::debug_println(
                    "gpud: chain G4.1 display-tex upload FAIL (transfer_to_host)",
                );
                e
            })?;

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
        s.emit_set_sampler_views(PIPE_SHADER_FRAGMENT, 0, &[H_SV_DISPLAY_TEX]);
        s.emit_bind_sampler_states(PIPE_SHADER_FRAGMENT, 0, &[H_SAMPLER]);
        // H_DISPLAY_TEX is exactly screen-sized (1280×800), so UV = fragcoord/screen.
        s.emit_set_constant_buffer(
            PIPE_SHADER_FRAGMENT,
            &[1.0 / SCREEN_W as f32, 1.0 / SCREEN_H as f32, 0.0, 0.0],
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
        self.ctrl_submit_header_tail(&hdr, bytes).map_err(|e| {
            let _ = nexus_abi::debug_println(
                "gpud: chain G4.2 scanout blit submit FAIL (submit_3d)",
            );
            e
        })?;

        // NOTE: the G1 parity check did `transfer_from_host(GL_SCANOUT_RES)` —
        // reading the scanout texture BACK to guest memory. That readback
        // desyncs QEMU's GL-scanout present (the display goes black even though
        // the RT is correct). The pure-GL DIAG draws presented precisely because
        // they skipped this. So the live present must NOT read the scanout back.
        if !self.gl_present_parity_done {
            self.gl_present_parity_done = true;
            let _ = nexus_abi::debug_println(crate::markers::GPUD_GL_PRESENT_OK);
        }

        // RT-direct (Increment 1): composite the layers deferred this frame
        // straight onto the scanout RT, over the base just blitted here — no VMO
        // render + re-upload. No-op when nothing was deferred.
        self.composite_pending_rt_layers();

        self.gl_flush_rect(Rect { x, y, width: w, height: h }).map_err(|e| {
            let _ = nexus_abi::debug_println(
                "gpud: chain G4.3 scanout flush FAIL (resource_flush)",
            );
            e
        })
    }

    /// Incremental GPU compositor (build-up). Renders a synthetic scene into the
    /// scanout RT via GL draws only, adding one feature per `COMPOSITOR_STAGE`
    /// (see the const docs) — to find which GL op breaks the present while
    /// building real compositor features. Pure GL draws (clear + SDF/gradient +
    /// shadow) are confirmed to present; textured stages test sampling.
    fn compositor_buildup_present(&mut self) -> Result<(), GfxError> {
        use nexus_gfx::command::buffer::RgbaColor;
        // Pre-warm the lazy vector shaders (SDF gradient/shadow) SYNCHRONOUSLY so
        // their one-time CREATE_OBJECT commands are validated outside the batch and
        // can't silently fail inside it. Idempotent — a no-op after the first present.
        let _ = self.virgl_vector_init();
        // Batch the whole present: every SUBMIT_3D draw below + the final flush is
        // ENQUEUED into the multi-entry ring without a per-command wait, then drained
        // once at the end. A textured (sampling) draw whose completion QEMU defers no
        // longer blocks the next command — only the single drain waits for it.
        self.ctrl_batch_begin();
        // Background: solid clear (a later stage replaces this with the wallpaper).
        let mut s = Submit3d::new();
        s.emit_set_framebuffer_state(0, &[H_GLS_SURF]);
        s.emit_set_viewport_box(0.0, 0.0, SCREEN_W as f32, SCREEN_H as f32);
        s.emit_clear(PIPE_CLEAR_COLOR0, [0.05, 0.07, 0.12, 1.0], 1.0, 0);
        let bytes = s.as_bytes();
        let hdr = VirtioGpuSubmit3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_SUBMIT_3D),
            size: bytes.len() as u32,
            _padding: 0,
        };
        self.ctrl_submit_header_tail(&hdr, bytes)?;

        // Stage 2: fullscreen blit of the pre-uploaded wallpaper texture (sampled,
        // NO per-frame transfer). Tests whether sampling a texture uploaded once
        // at init presents — vs the per-frame-transfer content path (black).
        if COMPOSITOR_STAGE >= 2 {
            let mut sw = Submit3d::new();
            sw.emit_bind_object(crate::virgl::VIRGL_OBJECT_BLEND, 0x20);
            sw.emit_bind_object(crate::virgl::VIRGL_OBJECT_DSA, 0x21);
            sw.emit_bind_object(crate::virgl::VIRGL_OBJECT_RASTERIZER, 0x22);
            sw.emit_bind_object(crate::virgl::VIRGL_OBJECT_VERTEX_ELEMENTS, 0x23);
            sw.emit_set_framebuffer_state(0, &[H_GLS_SURF]);
            sw.emit_set_viewport_box(0.0, 0.0, SCREEN_W as f32, SCREEN_H as f32);
            sw.emit_set_sampler_views(PIPE_SHADER_FRAGMENT, 0, &[H_SV_WALLPAPER]);
            sw.emit_bind_sampler_states(PIPE_SHADER_FRAGMENT, 0, &[H_SAMPLER]);
            sw.emit_set_constant_buffer(
                PIPE_SHADER_FRAGMENT,
                &[1.0 / SCREEN_W as f32, 1.0 / SCREEN_H as f32, 0.0, 0.0],
            );
            sw.emit_bind_shader(H_VS, PIPE_SHADER_VERTEX);
            sw.emit_bind_shader(H_FS_BLIT, PIPE_SHADER_FRAGMENT);
            sw.emit_set_vertex_buffers(&[(16, 0, QUAD_RES)]);
            sw.emit_draw_vbo(0, 6, PIPE_PRIM_TRIANGLES);
            let wb = sw.as_bytes();
            let wh = VirtioGpuSubmit3d {
                hdr: self.virgl_hdr(VIRTIO_GPU_CMD_SUBMIT_3D),
                size: wb.len() as u32,
                _padding: 0,
            };
            self.ctrl_submit_header_tail(&wh, wb)?;
        }

        // Spin-blur demo: orbit the panel on a fixed circle so the shadow + glass
        // blur recompute every frame (reactive GPU/blur perf test; gpud drives the
        // re-presents on a 60Hz timer cap, no input). Disabled → static panel.
        let (px, py, pw, ph) = if BUILDUP_SPIN_DEMO {
            let (dx, dy) =
                SPIN_ORBIT_LUT[(self.buildup_frame % SPIN_ORBIT_LUT.len() as u64) as usize];
            self.buildup_frame = self.buildup_frame.wrapping_add(1);
            ((200i32 + dx).max(0) as u32, (140i32 + dy).max(0) as u32, 880u32, 520u32)
        } else {
            (200u32, 140u32, 880u32, 520u32)
        };

        // Stage 1: drop shadow behind the panel (computed SDF, alpha-blended).
        if COMPOSITOR_STAGE >= 1 {
            let _ = self.submit_drop_shadow_rt(
                px,
                py,
                pw,
                ph,
                28,
                36,
                0,
                24,
                RgbaColor::new(0, 0, 0, 180),
            );
        }

        if COMPOSITOR_STAGE >= 3 {
            // Stage 3: GLASS panel — blur the persistent wallpaper behind the
            // panel (FS_BLUR sampling H_WALLPAPER, vertical so the horizontal
            // bands visibly soften), then a translucent tint = frosted glass.
            // Pure GL draws + sampling a persistent texture — no per-frame transfer.
            let mut sb = Submit3d::new();
            sb.emit_bind_object(crate::virgl::VIRGL_OBJECT_BLEND, 0x20);
            sb.emit_bind_object(crate::virgl::VIRGL_OBJECT_DSA, 0x21);
            sb.emit_bind_object(crate::virgl::VIRGL_OBJECT_RASTERIZER, 0x22);
            sb.emit_bind_object(crate::virgl::VIRGL_OBJECT_VERTEX_ELEMENTS, 0x23);
            sb.emit_set_framebuffer_state(0, &[H_GLS_SURF]);
            sb.emit_set_viewport_box(px as f32, py as f32, pw as f32, ph as f32);
            sb.emit_set_sampler_views(PIPE_SHADER_FRAGMENT, 0, &[H_SV_WALLPAPER]);
            sb.emit_bind_sampler_states(PIPE_SHADER_FRAGMENT, 0, &[H_SAMPLER]);
            // FS_BLUR (handle 13): CONST[0]=(inv_w,inv_h,radius,falloff),
            // CONST[1]=(dir_x,dir_y,off_x,off_y). Vertical, radius 20, soft.
            sb.emit_set_constant_buffer(
                PIPE_SHADER_FRAGMENT,
                &[
                    1.0 / SCREEN_W as f32,
                    1.0 / SCREEN_H as f32,
                    20.0,
                    -0.02,
                    0.0,
                    1.0,
                    0.0,
                    0.0,
                ],
            );
            sb.emit_bind_shader(H_VS, PIPE_SHADER_VERTEX);
            sb.emit_bind_shader(13, PIPE_SHADER_FRAGMENT); // FS_BLUR
            sb.emit_set_vertex_buffers(&[(16, 0, QUAD_RES)]);
            sb.emit_draw_vbo(0, 6, PIPE_PRIM_TRIANGLES);
            let bb = sb.as_bytes();
            let bh = VirtioGpuSubmit3d {
                hdr: self.virgl_hdr(VIRTIO_GPU_CMD_SUBMIT_3D),
                size: bb.len() as u32,
                _padding: 0,
            };
            self.ctrl_submit_header_tail(&bh, bb)?;
            // Translucent glass tint over the blurred backdrop.
            let _ = self.diag_gradient_rt(
                px,
                py,
                pw,
                ph,
                RgbaColor::new(255, 255, 255, 70),
                RgbaColor::new(150, 180, 230, 96),
            );
        } else {
            // Base (Stage 0): opaque gradient panel — pure GL draw, over the shadow.
            let _ = self.diag_gradient_rt(
                px,
                py,
                pw,
                ph,
                RgbaColor::new(56, 122, 230, 255),
                RgbaColor::new(20, 44, 96, 255),
            );
        }

        // Stage 4: INPUT — draw a GL cursor at gpud's current pointer position
        // (cursor_ox/oy, updated by OP_MOVE_CURSOR from windowd as the mouse
        // moves; windowd also sends OP_PRESENT_DAMAGE on move so this re-renders).
        // Moving the mouse moves this marker = input live on the GPU compositor.
        if COMPOSITOR_STAGE >= 4 {
            let cx = self.cursor_ox.clamp(0, SCREEN_W as i32 - 20) as u32;
            let cy = self.cursor_oy.clamp(0, SCREEN_H as i32 - 28) as u32;
            // Dark outline then bright fill, so the cursor reads on any backdrop.
            let _ = self.diag_gradient_rt(
                cx.saturating_sub(2),
                cy.saturating_sub(2),
                22,
                30,
                RgbaColor::new(20, 20, 24, 255),
                RgbaColor::new(20, 20, 24, 255),
            );
            let _ = self.diag_gradient_rt(
                cx,
                cy,
                18,
                26,
                RgbaColor::new(255, 255, 255, 255),
                RgbaColor::new(225, 230, 240, 255),
            );
        }

        // Enqueue the flush as the last command in the batch. Pipelined: we do NOT
        // wait for this frame's completion — `ctrl_batch_end` only harvests prior
        // frames (the NEXT present's enqueues drive this frame's completion). A
        // textured draw whose completion QEMU defers therefore never blocks the
        // present; it is reaped one frame later. (G3c "pipeline flowing" is emitted
        // by ctrl_batch_end once a prior batch is reclaimed.)
        let first = !self.gl_present_parity_done;
        let _ = self.gl_flush_rect(Rect { x: 0, y: 0, width: SCREEN_W, height: SCREEN_H });
        if first {
            self.gl_present_parity_done = true;
            let _ = nexus_abi::debug_println("gpud: compositor buildup present");
            let _ = nexus_abi::debug_println(crate::markers::GPUD_CHAIN_BATCH_SUBMIT);
        }
        self.ctrl_batch_end()
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

    /// Debug: emit ASCII thumbnails of both pipeline ends — the windowd source
    /// plane (`gl-src`) and the presented scanout RT read back from the host
    /// (`gl-rt`). Lets us SEE our GPU output headlessly and localize where the
    /// frame goes wrong: `gl-src` blank => windowd; `gl-src` good but `gl-rt`
    /// blank => the GPU composite/present. See the `debug_thumbnail` module.
    pub(crate) fn gl_emit_thumbnails(&mut self) {
        // Stage 1: windowd-composited display plane in the shared VMO.
        if let Some((fb, fb_len, fb_w, display_row)) = self.scanout_fb() {
            unsafe {
                crate::debug_thumbnail::emit_ascii_thumbnail(
                    "gl-src",
                    fb as *const u8,
                    fb_len,
                    fb_w,
                    0,
                    display_row as usize,
                    SCREEN_W as usize,
                    SCREEN_H as usize,
                );
            }
        }
        // Stage 3: read the presented scanout RT back into guest memory.
        if self
            .virgl_transfer_from_host(GL_SCANOUT_RES, 0, 0, SCREEN_W, SCREEN_H, FB_STRIDE)
            .is_ok()
        {
            let rt = self.gl_scanout_backing_va as *const u8;
            let len = SCREEN_W as usize * SCREEN_H as usize * 4;
            unsafe {
                crate::debug_thumbnail::emit_ascii_thumbnail(
                    "gl-rt",
                    rt,
                    len,
                    SCREEN_W as usize,
                    0,
                    0,
                    SCREEN_W as usize,
                    SCREEN_H as usize,
                );
            }
        }
    }
}
