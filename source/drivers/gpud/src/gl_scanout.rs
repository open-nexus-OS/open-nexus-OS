// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: GL-presented scanout — the virgl path's compositor (GPU compositor G0–G3).
//! OWNERS: @ui @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! RFC: docs/rfcs/RFC-0059-ui-v5a-animation-nexusgfx-sdk-gpu-driver-contract.md
//! ADR: docs/adr/0032-gpu-command-ring-and-pipelined-present.md — `compose_frame`
//!   is PIPELINED: it enqueues every SUBMIT_3D draw + the flush into the ring and
//!   `ctrl_batch_end`s WITHOUT blocking; the next frame harvests this frame's
//!   completion (fixes the texture-sampling stall). Hop markers G3b/G3c.
//!   ADR-0072 (TASK-0215 5c-2 P6): every draw is a pipeline of the library
//!   (`virgl_pipelines`) — the WGSL programs the board draws with.
//! TESTS: `tools/nx` `chain_gpu_scanout.rs::chain_gpu_batched_present_hops_in_order`;
//!   `scripts/qemu-test.sh GPU_MODE=virgl` boot proof; the visible lanes' pixel proofs.
//!
//! On a `virtio-gpu-gl` device the displayed scanout is a **virgl 3D render
//! target** that QEMU presents as a host GL texture (`dpy_gl_scanout_texture`
//! + flip on RESOURCE_FLUSH). The guest never 3D-renders into a 2D-scanned
//! resource (the two paths fight over the same host surface and the display
//! goes black — the bug this module exists to fix). Each present composes the
//! whole frame into the back render target and flips:
//!
//!   wallpaper texture (windowd's Plane 0, uploaded once) ──layer──▶ back RT
//!   windowd's retained layers (atlas texture): shadow, glass blur, surface ──▶ back RT
//!   icon sprite, pointer sprite ──layer──▶ back RT ──SET_SCANOUT + FLUSH──▶ host flip
//!
//! DriverKit boundary note: everything in this file is virtio/virgl command
//! encoding — the portable contract is `gl_scanout_init` / `gl_present_damage`
//! (init + present-damage), which a real-GPU backend reimplements.

#![cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]

use nexus_gfx::backend::error::GfxError;
use nexus_gfx::backend::gpu::quads::Quad;
use nexus_gfx::backend::types::Rect;

use crate::backend::VirtioGpuBackend;
use crate::protocol::{
    self, VirtioGpuResourceFlush, VirtioGpuSetScanout, VIRTIO_GPU_CMD_RESOURCE_FLUSH,
    VIRTIO_GPU_CMD_SET_SCANOUT,
};
use crate::virgl::Submit3d;

/// virtio resource id of the GL scanout render target (swapchain RT "A").
pub(crate) const GL_SCANOUT_RES: u32 = 0xE0;
/// Second swapchain RT ("B"). Every present renders the full frame into the
/// BACK RT and flips via SET_SCANOUT + RESOURCE_FLUSH as the batch tail, so the
/// host GTK draw (which samples the scanout texture on QEMU's main loop,
/// asynchronously under MTTCG) can never observe a mid-composite frame — the
/// SMP=4 mouse/drag flicker fix.
pub(crate) const GL_SCANOUT_RES_B: u32 = 0xE7;
/// Surface handle for scanout RT A (context-scoped object namespace — the
/// pipelines' state is 0x20..0x23/0x34/0x62, the glass blur's 0x4B..0x51).
pub(crate) const H_GLS_SURF: u32 = 0x42;
/// Surface handle for scanout RT B.
pub(crate) const H_GLS_SURF_B: u32 = 0x4E;
/// Presentation mode: `true` = per-frame SET_SCANOUT flip between RT A/B
/// (zero-copy, the production path). `false` = one-line bisection fallback:
/// render into RT B, then one fullscreen RESOURCE_COPY_REGION B→A + flush(A)
/// — a single virgl op, atomic w.r.t. the host redraw, at the cost of one
/// fullscreen GPU copy per frame. Flip both ways if a QEMU build misbehaves
/// on per-frame SET_SCANOUT.
const SCANOUT_FLIP: bool = true;

/// Swapchain state stored on the backend (one field there — see
/// `VirtioGpuBackend::gl_swap`).
#[derive(Default)]
pub(crate) struct GlSwapState {
    /// Guest backing VA of the probe RT (`gl_probe`) — the ONE readback surface.
    pub(crate) probe_backing_va: usize,
    /// Which RT the display shows: 0 = A (`GL_SCANOUT_RES`), 1 = B. Every
    /// present renders into the BACK RT and flips, so the host GTK draw can
    /// never sample a mid-composite frame (the SMP=4/MTTCG flicker fix).
    /// Stays 0 forever in the copy-fallback config.
    pub(crate) front: u8,
    /// One-shot latch for the honest `gpud: gl flip on` marker.
    pub(crate) flip_marker_done: bool,
}
/// The wallpaper texture: display-sized, seeded with the splash at init and replaced by
/// windowd's wallpaper (Plane 0) at the reveal — sampled every present, never transferred per
/// frame.
pub(crate) const H_WALLPAPER_TEX: u32 = 0xE2;
/// Sampler view of H_WALLPAPER_TEX.
pub(crate) const H_SV_WALLPAPER: u32 = 0x45;

// self.display_w/H are GONE: the visible mode is `self.display_w/h`, resolved at
// probe from GET_DISPLAY_INFO (fallback 1280×800). The fixed RESOURCE layout
// (plane rows/strides of the shared windowd VMO) stays at its max budget —
// only the GL textures/scanout/viewport follow the visible mode.

impl VirtioGpuBackend {
    /// Swapchain: resource id of the BACK RT — the one every present renders
    /// into. Front/back swap on flip; with `SCANOUT_FLIP = false` the front
    /// stays A forever, so the back is always B.
    pub(crate) fn rt_back_res(&self) -> u32 {
        if self.gl_swap.front == 0 {
            GL_SCANOUT_RES_B
        } else {
            GL_SCANOUT_RES
        }
    }

    /// Swapchain: surface handle of the BACK RT (framebuffer-state target for
    /// every RT draw — buildup base, layers, blur, vector passes).
    pub(crate) fn rt_back_surface(&self) -> u32 {
        if self.gl_swap.front == 0 {
            H_GLS_SURF_B
        } else {
            H_GLS_SURF
        }
    }

    /// Swapchain: resource id of the FRONT RT — what the display shows.
    pub(crate) fn rt_front_res(&self) -> u32 {
        if self.gl_swap.front == 0 {
            GL_SCANOUT_RES
        } else {
            GL_SCANOUT_RES_B
        }
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
        if !self.frame.layers.set_scroll(scroll_id, src_row_abs) {
            return Err(GfxError::InvalidArgument); // id 0 / beyond the table
        }
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
        t: nexus_gfx::frame::Transform,
    ) -> Result<(), GfxError> {
        if !self.frame.layers.set_transform(layer_id, t) {
            return Err(GfxError::InvalidArgument); // id 0 / beyond the table
        }
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
    /// recorded override rows. The set stays clean → NO atlas
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
    /// waits), the wallpaper layer into the back RT — pays that 500 ms at boot. The
    /// drawn content is overwritten by the first real present, and NO one-shot
    /// upload state (wallpaper/cursor) is touched.
    pub(crate) fn gl_pipeline_warmup(&mut self) -> Result<(), GfxError> {
        if !self.gl_scanout_active {
            return Ok(());
        }
        // The back RT: the goal is only to trip QEMU's deferred-used-ring path once; the
        // next present covers it, and the visible front keeps the splash.
        let (w, h) = (self.display_w as f32, self.display_h as f32);
        let q = Quad::layer(0.0, 0.0, w, h, [1.0; 4], 0.0);
        self.draw(self.pipelines.layer_opaque, self.rt_back_surface(), &q, H_SV_WALLPAPER)?;
        let _ =
            self.gl_flush_rect(Rect { x: 0, y: 0, width: self.display_w, height: self.display_h });
        let _ = nexus_abi::trace_line("gpud: pipeline warmup ok");
        Ok(())
    }

    /// Present: the whole frame composed into the back RT and flipped (`compose_frame`); the
    /// damage rectangle is windowd's, the frame is always whole.
    pub(crate) fn gl_present_damage(&mut self, _rect: Rect) -> Result<(), GfxError> {
        if !self.gl_scanout_active {
            return Err(GfxError::DeviceNotFound);
        }
        self.compose_frame()
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
        // Hold the boot splash until windowd says the desktop is complete (`OP_REVEAL`):
        // uploading a plane windowd has not written yet would black out the splash. The
        // handshake IS the evidence that Plane 0 holds the wallpaper — the 3-pixel probe that
        // used to guess at it is gone (RFC-0093 §5).
        if !self.reveal_requested {
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
                self.display_w * 4,
            )
            .is_ok()
        {
            self.wallpaper_from_vmo_uploaded = true;
            // New RT base: every cached glass backdrop is stale.
            self.frame.glass.wallpaper_epoch = self.frame.glass.wallpaper_epoch.wrapping_add(1);
            let _ = nexus_abi::trace_line("gpud: wallpaper uploaded from vmo (jpeg)");
        }
    }

    /// The frame: the wallpaper (breathing with the splash's pulse until the reveal), then —
    /// once windowd has revealed the desktop — its retained layers, the icon and the pointer,
    /// all into the back RT, flipped as the batch's tail.
    fn compose_frame(&mut self) -> Result<(), GfxError> {
        use nexus_gfx::command::buffer::RgbaColor;
        // One-shot: upload the real cursor sprite (windowd's Mocu cursor, set via
        // store_cursor_sprite) into its GL texture, so the cursor composites as a proper layer
        // below. Outside the batch — and BEFORE the reveal gate samples `cursor_tex_ready()`: the
        // present that a cursor upload triggers must be able to reveal in the SAME pass (sampling
        // first left the gate one tick behind, ~234 ms observed).
        let _ = self.cursor_tex_init();
        // One-shot: the glass blur's scratch and cache textures.
        let _ = self.glass_tex_init();

        // Atomic boot reveal (ADR-0041, RFC-0093 §5): keep presenting ONLY the logo splash
        // until windowd reports the desktop complete with `OP_REVEAL` — wallpaper in Plane 0,
        // cursor uploaded, first frame presented. A handshake, not a heuristic: the pixel
        // probe and the two time caps that used to release the splash on a guess are gone.
        // Never-black holds by construction — the splash stays until the report; a dead
        // windowd is a supervision restart (ADR-0057), not a black screen. Once revealed,
        // `wallpaper_from_vmo_uploaded` latches it so every later frame composites.
        let should_reveal = self.wallpaper_from_vmo_uploaded || self.reveal_requested;

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
        if self.wallpaper_reupload_pending {
            self.wallpaper_reupload_pending = false;
            if self.wallpaper_from_vmo_uploaded {
                self.wallpaper_from_vmo_uploaded = false;
                let _ = nexus_abi::debug_write(b"gpud: wallpaper reupload\n");
            }
        }
        // One-shot: replace the logo splash with the real wallpaper (windowd's decoded
        // JPEG in VMO Plane 0) — only on reveal. Done before the batch — it issues its own
        // transfer_to_host (a ctrl command). The ONE reveal marker is printed by the request
        // loop on the `STATUS_REVEALED` ack, with the seq of the frame that revealed — a fact
        // about a presented frame, not a guess about why.
        if should_reveal && !self.wallpaper_from_vmo_uploaded {
            self.try_upload_wallpaper_from_vmo();
        }
        // Batch the whole present: every SUBMIT_3D draw below + the final flush is
        // ENQUEUED into the multi-entry ring without a per-command wait, then drained
        // once at the end. A textured (sampling) draw whose completion QEMU defers no
        // longer blocks the next command — only the single drain waits for it.
        self.ctrl_batch_begin();
        // Present-cost triage: split the batch into ENQUEUE (command building +
        // ring writes below) and DRAIN (`ctrl_batch_end`, which waits on the
        // PRIOR batch) — reported per stats window as `enq_us`/`drain_us`.
        let phase_t0 = nexus_abi::nsec().unwrap_or(0);
        // Swapchain: EVERY draw of this present targets the BACK RT; the flip
        // (SET_SCANOUT + flush) is the batch tail, so the host can only ever
        // display complete frames.
        let back = self.rt_back_surface();

        // The wallpaper over the whole frame, as it is (replace — no clear under it). While the
        // splash is still held it breathes: the layer's tint on the shared wall-clock curve,
        // continuous with the 2D text pulse and the init-batch splash frame.
        let (w, h) = (self.display_w as f32, self.display_h as f32);
        let wallpaper = if self.wallpaper_from_vmo_uploaded {
            (self.pipelines.layer_opaque, [1.0; 4])
        } else {
            let f = crate::backend::splash_pulse_q8(nexus_abi::nsec().unwrap_or(0)) as f32 / 256.0;
            (self.pipelines.layer_tinted, [f, f, f, 1.0])
        };
        let q = Quad::layer(0.0, 0.0, w, h, wallpaper.1, 0.0);
        self.draw(wallpaper.0, back, &q, H_SV_WALLPAPER)?;

        // Reveal the whole desktop in one frame (gated by `should_reveal` above):
        // windowd's REAL atlas layers, the icon sprite, and the cursor all composite
        // together over the just-uploaded wallpaper — so boot reads logo splash →
        // complete desktop, with NO wallpaper→menu→cursor staggering. Held back entirely
        // (only the splash presents) until the desktop is ready.
        if should_reveal {
            // windowd's REAL atlas layers (the shell/panels) straight onto the scanout
            // RT, over the wallpaper base. `Display::execute` (run by the request loop before
            // this) populated the pending layers from windowd's CompositeLayer commands.
            self.composite_pending_rt_layers();

            // Real icon layer: windowd's uploaded icon sprite (rendered from an SVG via the
            // nexus-svg HiDPI pipeline) as its own GPU sprite layer, above the wallpaper and
            // below the cursor. One-shot texture upload; no-op until windowd uploads it.
            let _ = self.icon_tex_init();
            if self.icon_tex_ready() {
                let _ = self.composite_icon_rt();
            }

            // Cursor on top of everything at windowd's pointer position (OP_MOVE_CURSOR), the
            // sprite's top-left hotspot-left/up of it (resize shapes center the hotspot —
            // TASK-0070 Phase 3); a negative position is "no pointer in this frame" (RFC-0095:
            // the cursor-free frame a capture freeze reads back).
            let pointer_shown = self.cursor_ox >= 0 && self.cursor_oy >= 0;
            let (hot_x, hot_y) = self.cpu.cursor_hot;
            let cx = (self.cursor_ox - hot_x as i32).clamp(0, self.display_w as i32 - 20) as u32;
            let cy = (self.cursor_oy - hot_y as i32).clamp(0, self.display_h as i32 - 28) as u32;
            if pointer_shown && self.cursor_tex_ready() {
                // Production path: the real cursor sprite as a premultiplied layer.
                let _ = self.composite_cursor_rt(cx, cy);
            } else if pointer_shown {
                // Before the sprite lands: a procedural arrow so the pointer is never
                // invisible — a dark outline under a light fill.
                let _ = self.fill_gradient_rt(
                    cx.saturating_sub(2),
                    cy.saturating_sub(2),
                    22,
                    30,
                    RgbaColor::new(20, 20, 24, 255),
                    RgbaColor::new(20, 20, 24, 255),
                );
                let _ = self.fill_gradient_rt(
                    cx,
                    cy,
                    18,
                    26,
                    RgbaColor::new(255, 255, 255, 255),
                    RgbaColor::new(225, 230, 240, 255),
                );
            }
        }

        // Make the just-rendered back RT visible as the last commands in the
        // batch (SET_SCANOUT flip or the atomic copy fallback — see
        // `gl_present_flip`). Pipelined: we do NOT wait for this frame's
        // completion — `ctrl_batch_end` only harvests prior frames (the NEXT
        // present's enqueues drive this frame's completion). A textured draw
        // whose completion QEMU defers therefore never blocks the present; it
        // is reaped one frame later. (G3c "pipeline flowing" is emitted by
        // ctrl_batch_end once a prior batch is reclaimed.)
        let first = !self.gl_present_parity_done;
        let flip_submitted = self.gl_present_flip().is_ok();
        if first {
            self.gl_present_parity_done = true;
            let _ = nexus_abi::trace_line("gpud: compositor buildup present");
            let _ = nexus_abi::debug_println(crate::markers::GPUD_CHAIN_BATCH_SUBMIT);
        }
        let phase_t1 = nexus_abi::nsec().unwrap_or(phase_t0);
        let end = self.ctrl_batch_end();
        let phase_t2 = nexus_abi::nsec().unwrap_or(phase_t1);
        crate::service_stats::note_present_phases(
            phase_t1.saturating_sub(phase_t0),
            phase_t2.saturating_sub(phase_t1),
        );
        // Honest one-shot: the first back-RT frame + flip actually reached the
        // ring and the batch closed cleanly — the swapchain is live.
        if SCANOUT_FLIP && flip_submitted && end.is_ok() && !self.gl_swap.flip_marker_done {
            self.gl_swap.flip_marker_done = true;
            let _ = nexus_abi::debug_println("gpud: gl flip on");
        }
        end
    }

    /// Present the BACK RT. `SCANOUT_FLIP` (production): SET_SCANOUT to the
    /// back RT, swap front/back, flush the new front — zero-copy page flip.
    /// Fallback: one fullscreen RESOURCE_COPY_REGION back→front + flush —
    /// a single virgl op, atomic w.r.t. the host redraw (no swap; the front
    /// stays RT A). Both variants ride the current batch: ring order places
    /// them after every draw of the frame.
    fn gl_present_flip(&mut self) -> Result<(), GfxError> {
        let full = Rect { x: 0, y: 0, width: self.display_w, height: self.display_h };
        if SCANOUT_FLIP {
            let back = self.rt_back_res();
            let scanout = VirtioGpuSetScanout {
                hdr: crate::backend::ctrl_hdr(VIRTIO_GPU_CMD_SET_SCANOUT),
                r: protocol::VirtioGpuRect {
                    x: 0,
                    y: 0,
                    width: self.display_w,
                    height: self.display_h,
                },
                scanout_id: 0,
                resource_id: back,
            };
            self.ctrl_submit_struct(&scanout)?;
            // The back RT is now the front; the next present renders the other one.
            self.gl_swap.front ^= 1;
            self.gl_flush_rect(full)
        } else {
            let (front, back) = (self.rt_front_res(), self.rt_back_res());
            let mut s = Submit3d::new();
            s.emit_resource_copy_region(front, 0, 0, back, 0, 0, self.display_w, self.display_h);
            self.submit3d(&s)?;
            self.gl_flush_rect(full)
        }
    }

    /// RESOURCE_FLUSH on the FRONT scanout RT — on virtio-gpu-gl this triggers
    /// the host display update (`dpy_gl_update`), i.e. the visible flip.
    pub(crate) fn gl_flush_rect(&mut self, rect: Rect) -> Result<(), GfxError> {
        let flush = VirtioGpuResourceFlush {
            hdr: crate::backend::ctrl_hdr(VIRTIO_GPU_CMD_RESOURCE_FLUSH),
            r: protocol::VirtioGpuRect {
                x: rect.x,
                y: rect.y,
                width: rect.width,
                height: rect.height,
            },
            resource_id: self.rt_front_res(),
            _padding: 0,
        };
        self.ctrl_submit_struct(&flush)
    }
}
