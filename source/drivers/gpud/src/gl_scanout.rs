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
/// Backdrop scratch texture (screen-sized, GPU-only — no guest backing): before
/// each glass layer composites, the RT region beneath it is copied here
/// (`RESOURCE_COPY_REGION`) so the blur pass samples the DESTINATION-SO-FAR
/// (lower windows, chrome) instead of the static wallpaper (TASK-0070 Phase 4).
const H_BACKDROP_TEX: u32 = 0xE5;
/// Sampler view of H_BACKDROP_TEX.
const H_SV_BACKDROP: u32 = 0x4B;
/// Separable-blur ping-pong partner (screen-sized, GPU-only like the backdrop
/// scratch): the HORIZONTAL gaussian pass renders into this, the VERTICAL pass
/// samples it back into the glass RT — a real 2-pass separable gaussian
/// instead of the old single vertical pass.
const H_BLUR_TMP_TEX: u32 = 0xE6;
/// Surface (render-target view) over H_BLUR_TMP_TEX.
const H_BLUR_TMP_SURF: u32 = 0x4C;
/// Sampler view of H_BLUR_TMP_TEX.
const H_SV_BLUR_TMP: u32 = 0x4D;
/// Default sampler state (created by `virgl_blur_init`).
const H_SAMPLER: u32 = 0x34;
/// Fullscreen −1..1 quad VBO (resource 0xFA, created by `virgl_blur_init`).
const QUAD_RES: u32 = 0xFA;

// self.display_w/H are GONE: the visible mode is `self.display_w/h`, resolved at
// probe from GET_DISPLAY_INFO (fallback 1280×800). The fixed RESOURCE layout
// (plane rows/strides of the shared windowd VMO) stays at its max budget —
// only the GL textures/scanout/viewport follow the visible mode.

// Boot-splash wordmark + glow: owned by `backend::bootstrap` (the earliest
// phase that draws it — task #122 unified both splash phases onto one image);
// the GL seed below renders through the same shared compose.
use crate::backend::compose_splash_region;

/// Incremental GPU compositor build-up — a DEBUG harness. From the confirmed-
/// working base (solid clear + gradient panel — pure GL draws that DO present),
/// add ONE feature per `COMPOSITOR_STAGE` (shadow → wallpaper texture → blur → …)
/// and check after each whether the display still presents. The first stage that
/// goes black is the op QEMU's GL-scanout present can't handle.
///
/// This buildup harness is the **incremental "new window"**: we add real
/// compositor pieces one `COMPOSITOR_STAGE` at a time onto the working base
/// (background + cursor), wiring new content as VMO-sourced layers, and find
/// where the GL scanout breaks.
///
/// NOTE: the full-frame `gl_present_damage` path (`false`) — uploading windowd's
/// entire composited display plane into `H_DISPLAY_TEX` and blitting it as the
/// whole present — currently presents BLACK on this QEMU GL scanout. That is a
/// separate debug; the integration route is to add windowd content as *layers*
/// on this presenting buildup base (the wallpaper already does this from Plane 0),
/// NOT to switch the whole present over. Keep `true`.
pub(crate) const COMPOSITOR_BUILDUP: bool = true;
/// Incremental compositor build-up — raise ONE stage at a time and boot
/// (`GPU_MODE=virgl just start`) to find which GL op first goes black:
///   0 = solid clear background + cursor (the absolute minimum)
///   1 = + wallpaper-texture background (our background; sampled, no per-frame transfer)
///   2 = + opaque gradient panel
///   3 = + drop shadow behind the panel
///   4 = + glass blur (frosted panel)
/// The cursor (the mouse) is ALWAYS drawn, on top. Stage 1 = wallpaper + cursor
/// base; the REAL UI (target-test panel etc.) now composites on top as windowd
/// atlas layers (`composite_pending_rt_layers`), carrying their own shadow/glass —
/// so the synthetic panel/shadow/blur stages (2–4) are no longer needed for the
/// live UI. Raise to 2–4 only to bisect the synthetic GL ops in isolation.
const COMPOSITOR_STAGE: u32 = 1;
/// Automated spin-blur demo: when true, an idle gpud re-presents the *orbiting*
/// build-up panel (shadow + glass blur) every frame to exercise the GPU blur/shadow
/// pipeline + the reactive ring-buffer IRQ at the 120 Hz target. The re-present is
/// driven by a recv timeout on gpud's server endpoint (the kernel's timer IRQ wakes
/// the timed-out recv via `wake_expired_ipc_deadlines`), NOT a timer cap on that
/// endpoint — an earlier timer-cap attempt intercepted windowd's present commands
/// and OOM'd the channel.
//
// REACTIVE: off. The build-up now shows windowd's real composited UI (layers),
// so the idle spin-demo re-present is pure waste — gpud presents only on windowd's
// OP_PRESENT_DAMAGE (input/animation), nothing on idle. (Set true only to perf-test
// the blur/shadow orbit in isolation.)
pub(crate) const BUILDUP_SPIN_DEMO: bool = false;
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

/// Tinted variant of `FS_BLIT` for the boot-splash hold phase only: the sampled
/// colour is multiplied by CONST[1] (the breathing brightness). A SEPARATE
/// shader so the shared display blit path keeps its untouched fast shader —
/// zero blast radius on steady-state presents.
const H_FS_BLIT_TINT: u32 = 15;
const FS_BLIT_TINT: &str = "FRAG\n\
    DCL IN[0], POSITION, LINEAR\n\
    DCL OUT[0], COLOR\n\
    DCL SAMP[0]\n\
    DCL SVIEW[0], 2D, FLOAT\n\
    DCL CONST[0..1]\n\
    DCL TEMP[0..1]\n\
    MAD TEMP[0].xy, IN[0].xyyy, CONST[0].xyyy, CONST[0].zwww\n\
    TEX TEMP[1], TEMP[0], SAMP[0], 2D\n\
    MUL OUT[0], TEMP[1], CONST[1]\n\
    END\n";

impl VirtioGpuBackend {
    /// P0.3 display truth: reads a small strip of the LIVE scanout render
    /// target back from the host GPU and returns the brightest pixel
    /// (BGRA). `None` when the GL scanout is not active. One-shot caller
    /// (first successful present) — this is the only place display-side
    /// truth exists WITHOUT host tooling: markers above this line are
    /// compositor claims, this is what the screen actually holds.
    pub(crate) fn scanout_sample(&mut self) -> Option<[u8; 4]> {
        if self.gl_scanout_backing_va == 0 {
            return None;
        }
        const SAMPLE_W: u32 = 64;
        const SAMPLE_H: u32 = 4;
        let x = (self.display_w - SAMPLE_W) / 2;
        let y = (self.display_h - SAMPLE_H) / 2;
        self.virgl_transfer_from_host(
            GL_SCANOUT_RES,
            x,
            y,
            SAMPLE_W,
            SAMPLE_H,
            (self.display_w * 4),
        )
        .ok()?;
        let mut best = [0u8; 4];
        let mut best_sum: u32 = 0;
        for row in 0..SAMPLE_H {
            for col in 0..SAMPLE_W {
                let off = ((y + row) * (self.display_w * 4) + (x + col) * 4) as usize;
                let px = unsafe {
                    core::ptr::read_volatile((self.gl_scanout_backing_va + off) as *const [u8; 4])
                };
                let sum = px[0] as u32 + px[1] as u32 + px[2] as u32;
                if sum > best_sum {
                    best_sum = sum;
                    best = px;
                }
            }
        }
        Some(best)
    }

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
            (self.display_w * 4),
        )?;

        // Surface + blit fragment shader (vertex shader 10 persists from boot).
        let mut s = Submit3d::new();
        s.emit_create_surface(H_GLS_SURF, GL_SCANOUT_RES, PIPE_FORMAT_B8G8R8A8_UNORM);
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

    /// Scroll fast path, RECORD half (analogue of `OP_MOVE_CURSOR`): store the
    /// source-row override for the retained scrollable layer with this id. NO
    /// present here — the service loop drains a whole burst of queued scroll
    /// requests first (latest row wins) and then calls
    /// [`Self::flush_layer_scroll`] ONCE. Without the split, every queued
    /// request triggered a full GL re-composite of an already-stale position —
    /// under a fling the backlog snowballed into seconds of dead UI.
    pub(crate) fn record_layer_scroll(
        &mut self,
        scroll_id: u32,
        src_row_abs: u32,
    ) -> Result<(), GfxError> {
        let Some(slot) =
            scroll_id.checked_sub(1).and_then(|i| self.scroll_src_rows.get_mut(i as usize))
        else {
            return Err(GfxError::InvalidArgument); // id 0 / beyond the table
        };
        *slot = Some(src_row_abs);
        // Honest one-shot: the generalized scroll fast path actually took effect.
        if !self.layer_scroll_marker_done {
            self.layer_scroll_marker_done = true;
            let _ = nexus_abi::debug_println(&alloc::format!(
                "gpud: layer scroll live id={scroll_id} row={src_row_abs}"
            ));
        }
        Ok(())
    }

    /// Transform fast path, RECORD half (Track C2 — mirrors
    /// [`Self::record_layer_scroll`]): store the latest translate/opacity/
    /// scale override for `layer_id`; the service loop drains the queued burst
    /// (latest wins) and re-composites ONCE via [`Self::flush_layer_scroll`].
    pub(crate) fn record_layer_transform(
        &mut self,
        layer_id: u32,
        t: crate::backend::LayerTransform,
    ) -> Result<(), GfxError> {
        let Some(slot) =
            layer_id.checked_sub(1).and_then(|i| self.layer_transforms.get_mut(i as usize))
        else {
            return Err(GfxError::InvalidArgument); // id 0 / beyond the table
        };
        *slot = Some(t);
        // Honest one-shot: the unified layer-transform path took effect.
        if !self.layer_transform_marker_done {
            self.layer_transform_marker_done = true;
            let _ = nexus_abi::debug_println(&alloc::format!(
                "gpud: layer transform live id={layer_id}"
            ));
        }
        Ok(())
    }

    /// Scroll fast path, PRESENT half: one GPU re-composite at the latest
    /// recorded override rows. `rt_layers_dirty` stays false → NO atlas
    /// re-upload, just a different source offset into the already-uploaded
    /// texture — a ~54µs GPU pass, no CPU re-render.
    pub(crate) fn flush_layer_scroll(&mut self) -> Result<(), GfxError> {
        self.gl_present_damage(Rect { x: 0, y: 0, width: self.display_w, height: self.display_h })
    }

    /// Boot-time GPU pipeline warmup — absorb the one-time virgl texture-sampling
    /// stall during boot instead of on the user's first present/scroll.
    ///
    /// The FIRST texture-SAMPLING draw on virtio-gpu-gl makes QEMU defer the
    /// used-ring advance, so gpud's synchronous drain waits the full
    /// `GPU_WAIT_DEADLINE_NS` (~500 ms) exactly once; after that the path is warm
    /// (~50 µs). If that first sampling draw is the user's first scroll frame, the
    /// UI appears to "freeze for half a second and not respond" (confirmed by the
    /// stall watchdog: `present stuck 501ms` at `last_seq=1`). Doing ONE throwaway
    /// sampling draw here — synchronously (outside `ctrl_batch_begin/end`, so it
    /// waits), sampling the boot-seeded wallpaper texture into the scanout RT —
    /// pays that 500 ms at boot. The drawn content is overwritten by the first real
    /// present, and NO one-shot upload state (wallpaper/cursor) is touched.
    pub(crate) fn gl_pipeline_warmup(&mut self) -> Result<(), GfxError> {
        if !self.gl_scanout_active {
            return Ok(());
        }
        // Validate the lazy vector (SDF gradient/shadow) shaders' one-time
        // CREATE_OBJECT now too, so their first use later isn't a fresh stall.
        let _ = self.virgl_vector_init();
        // One synchronous textured (sampling) draw — the command that trips QEMU's
        // deferred-used-ring path. A bare submit (no batch) waits for completion.
        let mut sw = Submit3d::new();
        sw.emit_bind_object(crate::virgl::VIRGL_OBJECT_BLEND, 0x20);
        sw.emit_bind_object(crate::virgl::VIRGL_OBJECT_DSA, 0x21);
        sw.emit_bind_object(crate::virgl::VIRGL_OBJECT_RASTERIZER, 0x22);
        sw.emit_bind_object(crate::virgl::VIRGL_OBJECT_VERTEX_ELEMENTS, 0x23);
        sw.emit_set_framebuffer_state(0, &[H_GLS_SURF]);
        sw.emit_set_viewport_box(0.0, 0.0, self.display_w as f32, self.display_h as f32);
        sw.emit_set_sampler_views(PIPE_SHADER_FRAGMENT, 0, &[H_SV_WALLPAPER]);
        sw.emit_bind_sampler_states(PIPE_SHADER_FRAGMENT, 0, &[H_SAMPLER]);
        sw.emit_set_constant_buffer(
            PIPE_SHADER_FRAGMENT,
            &[1.0 / self.display_w as f32, 1.0 / self.display_h as f32, 0.0, 0.0],
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
        let _ =
            self.gl_flush_rect(Rect { x: 0, y: 0, width: self.display_w, height: self.display_h });
        let _ = nexus_abi::trace_line("gpud: pipeline warmup ok");
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
        let x = rect.x.min(self.display_w);
        let y = rect.y.min(self.display_h);
        let w = rect.width.min(self.display_w - x);
        let h = rect.height.min(self.display_h - y);
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
                    let dst_off =
                        (y as usize + row) * (self.display_w as usize * 4) + x as usize * 4;
                    let len = w as usize * 4;
                    if src_off + len <= fb_len {
                        unsafe {
                            core::ptr::copy_nonoverlapping(fb.add(src_off), dst.add(dst_off), len);
                        }
                    }
                }
            }
        }
        self.virgl_transfer_to_host(H_DISPLAY_TEX, x, y, w, h, (self.display_w * 4)).map_err(
            |e| {
                let _ = nexus_abi::debug_println(
                    "gpud: chain G4.1 display-tex upload FAIL (transfer_to_host)",
                );
                e
            },
        )?;

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
            &[1.0 / self.display_w as f32, 1.0 / self.display_h as f32, 0.0, 0.0],
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
            let _ =
                nexus_abi::debug_println("gpud: chain G4.2 scanout blit submit FAIL (submit_3d)");
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
            let _ =
                nexus_abi::debug_println("gpud: chain G4.3 scanout flush FAIL (resource_flush)");
            e
        })
    }

    /// True once windowd has written real content into shared-VMO Plane 0 (the boot
    /// wallpaper it composes on its first frame). Probes a few spread pixels — a decoded
    /// wallpaper is never all-zero everywhere. Drives the atomic boot reveal: the logo
    /// splash is held until this is true (and the cursor is up), so the desktop appears
    /// in one frame rather than wallpaper-first.
    fn plane0_has_content(&self) -> bool {
        let Some((fb, fb_len, fb_w, _display_row)) = self.scanout_fb() else {
            return false;
        };
        if fb.is_null() {
            return false;
        }
        let stride = fb_w * 4;
        if self.display_h as usize * stride > fb_len {
            return false;
        }
        let probes = [
            0usize,
            (self.display_h as usize / 2) * stride + (self.display_w as usize / 2) * 4,
            (self.display_h as usize - 1) * stride + (self.display_w as usize - 1) * 4,
        ];
        for p in probes {
            if p + 3 < fb_len {
                unsafe {
                    if *fb.add(p) != 0 || *fb.add(p + 1) != 0 || *fb.add(p + 2) != 0 {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Copy the real wallpaper from windowd's shared-VMO **Plane 0** (rows
    /// 0..self.display_h — the decoded JPEG it writes once at boot) into the wallpaper
    /// texture's backing and transfer it to the host. One-shot: replaces the boot
    /// color-bands with the real background. No-op (leaves the bands) until Plane 0
    /// is reachable. The shared VMO base is `scanout_fb`'s `fb` (Plane 0 = offset 0).
    fn try_upload_wallpaper_from_vmo(&mut self) {
        let Some((fb, fb_len, fb_w, _display_row)) = self.scanout_fb() else {
            return;
        };
        let dst = self.gl_wallpaper_tex_va as *mut u8;
        if dst.is_null() || fb.is_null() {
            return;
        }
        let stride = fb_w * 4; // VMO row stride (bytes)
        let row_bytes = (self.display_w as usize * 4).min(stride);
        // Plane 0 (wallpaper) occupies rows 0..self.display_h of the shared VMO.
        if self.display_h as usize * stride > fb_len {
            return;
        }
        // Hold the boot splash while Plane 0 is still empty (see `plane0_has_content`):
        // uploading an empty plane would black out the splash. `wallpaper_from_vmo_uploaded`
        // is NOT set on skip, so the next present retries until real content lands.
        if !self.plane0_has_content() {
            return;
        }
        for row in 0..self.display_h as usize {
            let src_off = row * stride;
            let dst_off = row * (self.display_w as usize * 4);
            unsafe {
                core::ptr::copy_nonoverlapping(fb.add(src_off), dst.add(dst_off), row_bytes);
            }
        }
        if self
            .virgl_transfer_to_host(
                H_WALLPAPER_TEX,
                0,
                0,
                self.display_w,
                self.display_h,
                (self.display_w * 4),
            )
            .is_ok()
        {
            self.wallpaper_from_vmo_uploaded = true;
            let _ = nexus_abi::trace_line("gpud: wallpaper uploaded from vmo (jpeg)");
        }
    }

    /// One-shot: create the screen-sized backdrop scratch texture + its sampler
    /// view. GPU-only (no guest backing, no transfers — it is only ever a
    /// `RESOURCE_COPY_REGION` destination and an FS_BLUR sampling source), so it
    /// costs no VMO arena. Issues create/attach ctrl commands — call OUTSIDE a
    /// present batch (like `cursor_tex_init`). Idempotent after first success.
    pub(crate) fn backdrop_tex_init(&mut self) -> Result<(), GfxError> {
        if self.backdrop_tex_ready {
            return Ok(());
        }
        let create = VirtioGpuResourceCreate3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_CREATE_3D),
            resource_id: H_BACKDROP_TEX,
            target: PIPE_TEXTURE_2D,
            format: PIPE_FORMAT_B8G8R8A8_UNORM,
            bind: PIPE_BIND_RENDER_TARGET | PIPE_BIND_SAMPLER_VIEW,
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
            resource_id: H_BACKDROP_TEX,
            _padding: 0,
        };
        self.ctrl_submit_struct(&ctx_attach)?;
        // Ping-pong partner for the separable gaussian (H-pass target).
        let create_tmp = VirtioGpuResourceCreate3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_CREATE_3D),
            resource_id: H_BLUR_TMP_TEX,
            target: PIPE_TEXTURE_2D,
            format: PIPE_FORMAT_B8G8R8A8_UNORM,
            bind: PIPE_BIND_RENDER_TARGET | PIPE_BIND_SAMPLER_VIEW,
            width: self.display_w,
            height: self.display_h,
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
            resource_id: H_BLUR_TMP_TEX,
            _padding: 0,
        };
        self.ctrl_submit_struct(&ctx_attach_tmp)?;
        let mut s = Submit3d::new();
        s.emit_create_sampler_view(H_SV_BACKDROP, H_BACKDROP_TEX, PIPE_FORMAT_B8G8R8A8_UNORM);
        s.emit_create_sampler_view(H_SV_BLUR_TMP, H_BLUR_TMP_TEX, PIPE_FORMAT_B8G8R8A8_UNORM);
        s.emit_create_surface(H_BLUR_TMP_SURF, H_BLUR_TMP_TEX, PIPE_FORMAT_B8G8R8A8_UNORM);
        let bb = s.as_bytes();
        let bh = VirtioGpuSubmit3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_SUBMIT_3D),
            size: bb.len() as u32,
            _padding: 0,
        };
        self.ctrl_submit_header_tail(&bh, bb)?;
        self.backdrop_tex_ready = true;
        Ok(())
    }

    /// Frosted-glass backdrop: GPU-blur what is BENEATH a glass layer into the
    /// glass RT (`H_GLS_SURF`) at the layer's rect, so the translucent glass
    /// composited on top reads as real frosted glass. Run from
    /// `composite_pending_rt_layers` for every glass layer (`backdrop_blur > 0`).
    ///
    /// Destination-so-far (TASK-0070 Phase 4): layers composite back-to-front, so
    /// the correct backdrop is the RT content at this point — lower windows and
    /// chrome included. The RT region (padded by the radius so edge taps stay
    /// inside the copy) is snapshotted into `H_BACKDROP_TEX` in the SAME command
    /// stream as the blur draw, and FS_BLUR samples that snapshot. Until the
    /// scratch texture exists (first boot presents) it falls back to the original
    /// Stage-4 recipe of sampling the static wallpaper. Pure GPU-side work: the
    /// copy and the draws never touch guest memory, no per-frame
    /// TRANSFER_TO_HOST_3D — so it avoids the stall that gated the standalone
    /// VMO `BlurBackdrop`.
    pub(crate) fn blur_rt_backdrop(
        &mut self,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        radius: u32,
    ) -> Result<(), GfxError> {
        if radius == 0 || w == 0 || h == 0 {
            return Ok(());
        }
        let dst_so_far = self.backdrop_tex_ready;
        // Real gaussian falloff: per-tap weight = 2^(k·i²) = exp(−i²/2σ²) with
        // σ = radius/2 — the SAME curve as the CPU reference
        // (`raster::blur_gaussian`) and the fallback `submit_virgl_blur`. The
        // old hardcoded −0.02 falloff fixed the effective σ at ~5px no matter
        // the requested radius, which is why glass read as "tint, no blur".
        let sigma = (radius as f32 / 2.0).max(0.5);
        let k = -1.0 / (2.0 * sigma * sigma * core::f32::consts::LN_2);
        // Padded halo so edge taps of both passes stay inside snapshotted data.
        let x0 = x.saturating_sub(radius);
        let y0 = y.saturating_sub(radius);
        let x1 = (x + w + radius).min(self.display_w);
        let y1 = (y + h + radius).min(self.display_h);
        let mut sb = Submit3d::new();
        if dst_so_far {
            // Snapshot the RT beneath the layer (+radius halo, clamped).
            if x1 > x0 && y1 > y0 {
                sb.emit_resource_copy_region(
                    H_BACKDROP_TEX,
                    x0,
                    y0,
                    GL_SCANOUT_RES,
                    x0,
                    y0,
                    x1 - x0,
                    y1 - y0,
                );
            }
        }
        sb.emit_bind_object(crate::virgl::VIRGL_OBJECT_BLEND, 0x20);
        sb.emit_bind_object(crate::virgl::VIRGL_OBJECT_DSA, 0x21);
        sb.emit_bind_object(crate::virgl::VIRGL_OBJECT_RASTERIZER, 0x22);
        sb.emit_bind_object(crate::virgl::VIRGL_OBJECT_VERTEX_ELEMENTS, 0x23);
        sb.emit_bind_sampler_states(PIPE_SHADER_FRAGMENT, 0, &[H_SAMPLER]);
        sb.emit_bind_shader(H_VS, PIPE_SHADER_VERTEX);
        sb.emit_bind_shader(13, PIPE_SHADER_FRAGMENT); // FS_BLUR (1-D separable)
        sb.emit_set_vertex_buffers(&[(16, 0, QUAD_RES)]);
        if dst_so_far {
            // PASS 1 — HORIZONTAL: backdrop snapshot → ping-pong scratch, over
            // the PADDED rect so the vertical pass's ±radius row taps read
            // horizontally-blurred data (both textures are screen-sized, so
            // fragment coords address both identically).
            sb.emit_set_framebuffer_state(0, &[H_BLUR_TMP_SURF]);
            sb.emit_set_viewport_box(x0 as f32, y0 as f32, (x1 - x0) as f32, (y1 - y0) as f32);
            sb.emit_set_sampler_views(PIPE_SHADER_FRAGMENT, 0, &[H_SV_BACKDROP]);
            sb.emit_set_constant_buffer(
                PIPE_SHADER_FRAGMENT,
                &[
                    1.0 / self.display_w as f32,
                    1.0 / self.display_h as f32,
                    radius as f32,
                    k,
                    1.0,
                    0.0,
                    0.0,
                    0.0,
                ],
            );
            sb.emit_draw_vbo(0, 6, PIPE_PRIM_TRIANGLES);
            // PASS 2 — VERTICAL: scratch → glass RT, exact layer rect.
            sb.emit_set_framebuffer_state(0, &[H_GLS_SURF]);
            sb.emit_set_viewport_box(x as f32, y as f32, w as f32, h as f32);
            sb.emit_set_sampler_views(PIPE_SHADER_FRAGMENT, 0, &[H_SV_BLUR_TMP]);
            sb.emit_set_constant_buffer(
                PIPE_SHADER_FRAGMENT,
                &[
                    1.0 / self.display_w as f32,
                    1.0 / self.display_h as f32,
                    radius as f32,
                    k,
                    0.0,
                    1.0,
                    0.0,
                    0.0,
                ],
            );
            sb.emit_draw_vbo(0, 6, PIPE_PRIM_TRIANGLES);
        } else {
            // Bring-up fallback (scratch textures not created yet): single
            // vertical pass over the static wallpaper, now with the real k.
            sb.emit_set_framebuffer_state(0, &[H_GLS_SURF]);
            sb.emit_set_viewport_box(x as f32, y as f32, w as f32, h as f32);
            sb.emit_set_sampler_views(PIPE_SHADER_FRAGMENT, 0, &[H_SV_WALLPAPER]);
            sb.emit_set_constant_buffer(
                PIPE_SHADER_FRAGMENT,
                &[
                    1.0 / self.display_w as f32,
                    1.0 / self.display_h as f32,
                    radius as f32,
                    k,
                    0.0,
                    1.0,
                    0.0,
                    0.0,
                ],
            );
            sb.emit_draw_vbo(0, 6, PIPE_PRIM_TRIANGLES);
        }
        let bb = sb.as_bytes();
        let bh = VirtioGpuSubmit3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_SUBMIT_3D),
            size: bb.len() as u32,
            _padding: 0,
        };
        self.ctrl_submit_header_tail(&bh, bb)?;
        // Honest one-shot: emitted only after a destination-so-far snapshot+blur
        // actually submitted (never on the wallpaper fallback).
        if dst_so_far && !self.rt_backdrop_marker_done {
            self.rt_backdrop_marker_done = true;
            let _ = nexus_abi::debug_println(crate::markers::GPUD_RT_BACKDROP_DST);
        }
        Ok(())
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
        // One-shot: upload the real cursor sprite (windowd's Mocu cursor, set via
        // store_cursor_sprite) into its GL texture + pre-warm the layer shader, so
        // the cursor composites as a proper layer below. Outside the batch — and
        // BEFORE the reveal gate samples `cursor_tex_ready()`: the present that a
        // cursor upload triggers must be able to reveal in the SAME pass (sampling
        // first left the gate one tick behind, ~234 ms observed).
        let _ = self.cursor_tex_init();
        // One-shot: create the backdrop scratch texture for the destination-so-far
        // glass blur (also outside the batch — create/attach ctrl commands).
        let _ = self.backdrop_tex_init();

        // Atomic boot reveal: keep presenting ONLY the logo splash (the clear + seeded
        // wallpaper-texture blit below) until the whole desktop can appear at once —
        // Plane 0 holds windowd's real wallpaper AND the cursor sprite is up (mouse path
        // live). Two fallbacks, both timed from the first buildup present, guarantee the
        // splash is NEVER held forever: a short one once the wallpaper is up but the cursor
        // lags, and a hard cap that reveals regardless (even if a signal never arrives, the
        // desktop + its markers still appear). Once revealed, `wallpaper_from_vmo_uploaded`
        // latches it so every later frame composites.
        const REVEAL_FALLBACK_NS: u64 = 500_000_000; // 0.5s: wallpaper up, cursor still lagging
        const REVEAL_HARD_CAP_NS: u64 = 1_200_000_000; // 1.2s: reveal no matter what (bound the wait)
        let plane0 = self.plane0_has_content();
        let cursor = self.cursor_tex_ready();
        let should_reveal = self.wallpaper_from_vmo_uploaded || {
            let now = nexus_abi::nsec().unwrap_or(0);
            if self.reveal_content_since_ns == 0 {
                self.reveal_content_since_ns = now;
            }
            let elapsed = now.saturating_sub(self.reveal_content_since_ns);
            (plane0 && (cursor || elapsed > REVEAL_FALLBACK_NS)) || elapsed > REVEAL_HARD_CAP_NS
        };

        // Elastic hold: while still holding the splash, don't pile a new frame onto
        // a control ring that is still busy with the previous one — QEMU may defer
        // textured-draw completions for a long time, and enqueueing anyway would
        // park this single-threaded loop in ring back-pressure, starving the
        // wall-clock gate above (fallback/hard-cap could then fire seconds late).
        // The tick simply re-evaluates the gate a few ms later; a reveal frame is
        // never skipped.
        if !should_reveal && !self.wallpaper_from_vmo_uploaded && self.ctrl_ring_congested() {
            return Ok(());
        }

        // OP_WALLPAPER_DIRTY: windowd rewrote Plane 0 (theme-matched wallpaper
        // swap) — drop the one-shot latch so the block below re-uploads the
        // texture from the fresh plane pixels. `should_reveal` was computed
        // from the still-set latch above, so the splash gate never re-arms.
        let mut wallpaper_reupload = false;
        if self.wallpaper_reupload_pending {
            self.wallpaper_reupload_pending = false;
            if self.wallpaper_from_vmo_uploaded {
                self.wallpaper_from_vmo_uploaded = false;
                wallpaper_reupload = true;
                let _ = nexus_abi::debug_write(b"gpud: wallpaper reupload\n");
            }
        }
        // One-shot: replace the logo splash with the real wallpaper (windowd's decoded
        // JPEG in VMO Plane 0) — only on reveal. Done before the batch — it issues its own
        // transfer_to_host (a ctrl command), like `virgl_vector_init` above. The reveal
        // marker records WHICH condition released it, so a slow boot pins the culprit
        // (wallpaper Plane 0 vs cursor vs the time cap) directly in the UART timeline.
        if COMPOSITOR_STAGE >= 1 && should_reveal && !self.wallpaper_from_vmo_uploaded {
            if !wallpaper_reupload {
                let _ = nexus_abi::debug_println(match (plane0, cursor) {
                    (true, true) => "gpud: desktop reveal (plane0 + cursor ready)",
                    (true, false) => "gpud: desktop reveal (plane0 ready, cursor slow)",
                    (false, _) => "gpud: desktop reveal (TIME CAP — plane0 still empty)",
                });
            }
            self.try_upload_wallpaper_from_vmo();
        }
        // Batch the whole present: every SUBMIT_3D draw below + the final flush is
        // ENQUEUED into the multi-entry ring without a per-command wait, then drained
        // once at the end. A textured (sampling) draw whose completion QEMU defers no
        // longer blocks the next command — only the single drain waits for it.
        self.ctrl_batch_begin();
        // Background: solid clear (a later stage replaces this with the wallpaper).
        let mut s = Submit3d::new();
        s.emit_set_framebuffer_state(0, &[H_GLS_SURF]);
        s.emit_set_viewport_box(0.0, 0.0, self.display_w as f32, self.display_h as f32);
        s.emit_clear(PIPE_CLEAR_COLOR0, [0.05, 0.07, 0.12, 1.0], 1.0, 0);
        let bytes = s.as_bytes();
        let hdr = VirtioGpuSubmit3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_SUBMIT_3D),
            size: bytes.len() as u32,
            _padding: 0,
        };
        self.ctrl_submit_header_tail(&hdr, bytes)?;

        // Stage 1: our wallpaper background — fullscreen blit of the texture
        // uploaded ONCE at init (sampled, NO per-frame transfer). Tests whether
        // sampling a texture uploaded once presents — vs the per-frame-transfer
        // content path (black).
        if COMPOSITOR_STAGE >= 1 {
            // While the splash is still held, the blit breathes via the tinted
            // shader (shared wall-clock curve — continuous with the 2D text
            // pulse and the init-batch splash frame). Once the real wallpaper
            // is uploaded (reveal), the untouched fast blit takes over.
            let holding_pulse = !self.wallpaper_from_vmo_uploaded;
            let mut sw = Submit3d::new();
            sw.emit_bind_object(crate::virgl::VIRGL_OBJECT_BLEND, 0x20);
            sw.emit_bind_object(crate::virgl::VIRGL_OBJECT_DSA, 0x21);
            sw.emit_bind_object(crate::virgl::VIRGL_OBJECT_RASTERIZER, 0x22);
            sw.emit_bind_object(crate::virgl::VIRGL_OBJECT_VERTEX_ELEMENTS, 0x23);
            sw.emit_set_framebuffer_state(0, &[H_GLS_SURF]);
            sw.emit_set_viewport_box(0.0, 0.0, self.display_w as f32, self.display_h as f32);
            sw.emit_set_sampler_views(PIPE_SHADER_FRAGMENT, 0, &[H_SV_WALLPAPER]);
            sw.emit_bind_sampler_states(PIPE_SHADER_FRAGMENT, 0, &[H_SAMPLER]);
            if holding_pulse {
                let f =
                    crate::backend::splash_pulse_q8(nexus_abi::nsec().unwrap_or(0)) as f32 / 256.0;
                sw.emit_set_constant_buffer(
                    PIPE_SHADER_FRAGMENT,
                    &[
                        1.0 / self.display_w as f32,
                        1.0 / self.display_h as f32,
                        0.0,
                        0.0,
                        f,
                        f,
                        f,
                        1.0,
                    ],
                );
                sw.emit_bind_shader(H_VS, PIPE_SHADER_VERTEX);
                sw.emit_bind_shader(H_FS_BLIT_TINT, PIPE_SHADER_FRAGMENT);
            } else {
                sw.emit_set_constant_buffer(
                    PIPE_SHADER_FRAGMENT,
                    &[1.0 / self.display_w as f32, 1.0 / self.display_h as f32, 0.0, 0.0],
                );
                sw.emit_bind_shader(H_VS, PIPE_SHADER_VERTEX);
                sw.emit_bind_shader(H_FS_BLIT, PIPE_SHADER_FRAGMENT);
            }
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

        // Stage 2+: the panel and its effects (only while building up the UI on
        // top of the background + mouse). Nothing panel-related is emitted below
        // stage 2, so stage 0/1 is purely background + cursor.
        if COMPOSITOR_STAGE >= 2 {
            // Spin-blur demo: orbit the panel on a fixed circle so the shadow +
            // glass blur recompute every frame (reactive GPU/blur perf test; gpud
            // drives the re-presents on a 60Hz timer cap, no input). Disabled →
            // static panel.
            let (px, py, pw, ph) = if BUILDUP_SPIN_DEMO {
                let (dx, dy) =
                    SPIN_ORBIT_LUT[(self.buildup_frame % SPIN_ORBIT_LUT.len() as u64) as usize];
                self.buildup_frame = self.buildup_frame.wrapping_add(1);
                ((200i32 + dx).max(0) as u32, (140i32 + dy).max(0) as u32, 880u32, 520u32)
            } else {
                (200u32, 140u32, 880u32, 520u32)
            };

            // Stage 3: drop shadow behind the panel (computed SDF, alpha-blended).
            if COMPOSITOR_STAGE >= 3 {
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

            if COMPOSITOR_STAGE >= 4 {
                // Stage 4: GLASS panel — blur the persistent wallpaper behind the
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
                        1.0 / self.display_w as f32,
                        1.0 / self.display_h as f32,
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
                // Stage 2: opaque gradient panel — pure GL draw, over the shadow.
                let _ = self.diag_gradient_rt(
                    px,
                    py,
                    pw,
                    ph,
                    RgbaColor::new(56, 122, 230, 255),
                    RgbaColor::new(20, 44, 96, 255),
                );
            }
        }

        // Reveal the whole desktop in one frame (gated by `should_reveal` above):
        // windowd's REAL atlas layers, the icon sprite, and the cursor all composite
        // together over the just-uploaded wallpaper — so boot reads logo splash →
        // complete desktop, with NO wallpaper→menu→cursor staggering. Held back entirely
        // (only the clear + splash blit present) until the desktop is ready.
        if should_reveal {
            // windowd's REAL atlas layers (the shell/panels) straight onto the scanout
            // RT, over the wallpaper base. `present_committed` (run by the service before
            // this) populated the pending layers from windowd's CompositeLayer commands.
            self.composite_pending_rt_layers();

            // Real icon layer: windowd's uploaded icon sprite (rendered from an SVG via the
            // nexus-svg HiDPI pipeline) as its own GPU sprite layer, above the wallpaper and
            // below the cursor. One-shot texture upload; no-op until windowd uploads it.
            let _ = self.icon_tex_init();
            if self.icon_tex_ready() {
                let _ = self.composite_icon_rt();
            }

            // Cursor on top of everything (cursor_ox/oy, updated by OP_MOVE_CURSOR from
            // windowd as the mouse moves).
            // Hotspot-corrected sprite origin: the pointer POSITION is
            // cursor_ox/oy; the sprite's top-left sits hotspot-left/up of it
            // (resize shapes center the hotspot — TASK-0070 Phase 3).
            let (hot_x, hot_y) = self.cursor_hot;
            let cx = (self.cursor_ox - hot_x as i32).clamp(0, self.display_w as i32 - 20) as u32;
            let cy = (self.cursor_oy - hot_y as i32).clamp(0, self.display_h as i32 - 28) as u32;
            if self.cursor_tex_ready() {
                // Production path: composite the real cursor sprite as a layer
                // (alpha-blended; its own alpha shapes the arrow). Reuses the generic
                // layer compositor, not a bespoke draw.
                let _ = self.composite_cursor_rt(cx, cy);
            } else {
                // Fallback (reveal forced by the timer before the sprite landed): a
                // procedural arrow so the pointer is never invisible.
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
        }

        // Enqueue the flush as the last command in the batch. Pipelined: we do NOT
        // wait for this frame's completion — `ctrl_batch_end` only harvests prior
        // frames (the NEXT present's enqueues drive this frame's completion). A
        // textured draw whose completion QEMU defers therefore never blocks the
        // present; it is reaped one frame later. (G3c "pipeline flowing" is emitted
        // by ctrl_batch_end once a prior batch is reclaimed.)
        let first = !self.gl_present_parity_done;
        let _ =
            self.gl_flush_rect(Rect { x: 0, y: 0, width: self.display_w, height: self.display_h });
        if first {
            self.gl_present_parity_done = true;
            let _ = nexus_abi::trace_line("gpud: compositor buildup present");
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
}
