// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The virtio GPU as the request loop's display (`display::Display`): the framebuffer
//! made for the GPU and attached as its scanout resource, a present executed by the GPU's virgl
//! passes where they exist and by the shared CPU executor otherwise, then transferred and
//! flushed (2D) or composed onto the GL scanout (virgl); the hardware cursor overlay on the 2D
//! scanout, the procedural GL cursor on virgl; the splash held by the GL scanout until the reveal;
//! the frame clock's self-presented phases (bootstrap pulse, splash hold, spin demo). Moved out of
//! the loop when the board's display controller became the second display (TASK-0251 P2a step 2).
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: every QEMU display lane (`smp1` 2D, `visible`/`visible-fhd` virgl)

use nexus_abi::debug_println;
#[cfg(feature = "virgl")]
use nexus_abi::nsec;
use nexus_display_proto::{STATUS_DEVICE_ERROR, STATUS_MALFORMED, STATUS_OK};
use nexus_gfx::backend::error::GfxError;
use nexus_gfx::backend::traits::GfxBackend;
use nexus_gfx::backend::types::Rect;
use nexus_gfx::command::buffer::{Command, CommittedBuffer};

use super::display::Display;
use super::VirtioGpuBackend;
#[cfg(feature = "virgl")]
use crate::service_stats::PresentStats;

/// The resource the attach scans out: the layout's stride and every row of the shared layout
/// (planes + atlas) — the same constants windowd composes against.
const RESOURCE_WIDTH: u32 = nexus_display_proto::layout::LAYOUT_MAX.0;
const RESOURCE_HEIGHT: u32 = nexus_display_proto::layout::RESOURCE_HEIGHT;

/// The virgl build-up's spin-blur demo: the frame clock re-presents the orbiting build-up at
/// 120 Hz once windowd's framebuffer is attached.
#[cfg(feature = "virgl")]
const fn spin_demo() -> bool {
    crate::gl_scanout::COMPOSITOR_BUILDUP && crate::gl_scanout::BUILDUP_SPIN_DEMO
}

impl Display for VirtioGpuBackend {
    fn mode(&self) -> (u32, u32) {
        (self.display_w, self.display_h)
    }

    fn framebuffer(&mut self) -> Option<u32> {
        self.make_framebuffer()
    }

    fn attach(&mut self) -> Result<(), u8> {
        let Some(vmo) = self.framebuffer_vmo else {
            let _ = debug_println("gpud: FAIL attach before a grant");
            return Err(STATUS_MALFORMED);
        };
        self.attach_external_framebuffer(vmo, RESOURCE_WIDTH, RESOURCE_HEIGHT).map_err(|_| {
            let _ = debug_println("gpud: ERROR attach framebuffer failed");
            STATUS_DEVICE_ERROR
        })
    }

    fn attached(&mut self) {
        let _ = GfxBackend::move_cursor(self, 0, 0);
        // The GL scanout now exists, so the frame clock may re-present the build-up.
        #[cfg(feature = "virgl")]
        if spin_demo() {
            let _ = debug_println("gpud: spin-blur demo armed (120Hz)");
        }
    }

    fn execute(&mut self, commands: &[Command]) -> Result<(), GfxError> {
        if !self.is_probed() {
            return Err(GfxError::DeviceNotFound);
        }
        self.execute_commands(commands)
    }

    fn scan_out(&mut self, damage: Rect) -> Result<(), GfxError> {
        self.present_scanout_damage(damage)
    }

    /// P0.3 display truth (one-shot): the centre strip of the frame the display shows, read
    /// through the probe RT (RFC-0093 §5 — never a scanout transfer). A black sample with a green
    /// marker chain = the silent scanout class — loud, inside.
    #[cfg(feature = "virgl")]
    fn first_frame_shown(&mut self) {
        match self.probe_sample() {
            Some(px) if (px[0] as u32 + px[1] as u32 + px[2] as u32) > 24 => {
                let _ = debug_println("gpud: probe sample ok");
                // P0.3c: MEASURED display truth (host-GPU readback through the probe RT), not a
                // compositor claim — #98 discipline.
                let _ = debug_println("SELFTEST: display nonblack ok");
            }
            Some(_) => {
                let _ = debug_println("gpud: FAIL probe black");
            }
            None => {
                let _ = debug_println("gpud: probe sample unavailable");
            }
        }
    }

    /// P0.3 present truth: the ring's deadline-expiry counter around the whole present. The
    /// ring's degraded recovery (reset/abandon after `GPU_WAIT_DEADLINE_NS`) deliberately returns
    /// success so the loop never wedges — but a present that lost commands that way must NOT be
    /// acked as shown. The counter delta catches every such case, including error paths
    /// swallowed inside optional draws (`let _ =`), at the one seam they all share.
    fn begin_present(&mut self) {
        self.deadline_expiries_at_present =
            super::IRQ_DEADLINE_EXPIRED_COUNT.load(core::sync::atomic::Ordering::Relaxed);
    }

    fn present_lost(&mut self) -> bool {
        let expired = super::IRQ_DEADLINE_EXPIRED_COUNT
            .load(core::sync::atomic::Ordering::Relaxed)
            .wrapping_sub(self.deadline_expiries_at_present);
        if expired == 0 {
            return false;
        }
        crate::service_stats::emit_present_deadline_fail(expired);
        // The abandoned batch may have dropped an atlas TRANSFER — invalidate the uploaded epoch
        // so the re-presented frame uploads fresh content.
        #[cfg(feature = "virgl")]
        {
            self.atlas_uploaded_epoch = 0;
            self.rt_layers_dirty = true;
        }
        true
    }

    /// On the CPU/mmio scanout, arm the virtio-gpu **hardware cursor overlay** (cursor virtqueue)
    /// so the host composites the pointer at scanout — cursor moves then never touch windowd's
    /// present pipeline. Reply `CURSOR_REPLY_HW` so windowd suppresses its software BlendCursor.
    ///
    /// On the virgl GL scanout the overlay's `transfer_to_host` blanks the present, so there (and
    /// if arming the overlay fails for any reason) the sprite is stored for the GL cursor or
    /// windowd's BlendCursor and the reply names that path.
    fn upload_cursor(&mut self, bgra: &[u8], w: u32, h: u32, hot: (u32, u32)) -> (u8, Option<u32>) {
        #[cfg(not(feature = "virgl"))]
        if self.arm_hw_cursor(bgra, w, h, hot.0, hot.1).is_ok() {
            let _ = debug_println("gpud: hw cursor armed");
            return (STATUS_OK, Some(nexus_display_proto::CURSOR_REPLY_HW));
        }
        // virgl GL scanout, or HW arm failed: the GL/SW draw subtracts the hotspot, so record it
        // (resize shapes center it at 16,16).
        self.cpu.cursor_hot = hot;
        // On virgl the build-up present owns the scanout and draws the cursor at
        // `cursor_ox/oy` — reply GL so windowd ships moves + a present (its software BlendCursor
        // into the VMO would be ignored here). Elsewhere (HW arm failed) fall back to windowd's
        // BlendCursor (SW).
        #[cfg(feature = "virgl")]
        const NON_HW_REPLY: u32 = nexus_display_proto::CURSOR_REPLY_GL;
        #[cfg(not(feature = "virgl"))]
        const NON_HW_REPLY: u32 = nexus_display_proto::CURSOR_REPLY_SW;
        match self.cpu.store_cursor(bgra, w, h) {
            Ok(()) => {
                // Pointer-shape switch (TASK-0070 Phase 3): if the GL cursor texture is already
                // live, refresh it from the new sprite now (outside any present batch) so the
                // shape changes immediately.
                #[cfg(feature = "virgl")]
                let _ = self.cursor_tex_refresh();
                let _ = debug_println("gpud: cursor uploaded");
                (STATUS_OK, Some(NON_HW_REPLY))
            }
            Err(_) => (STATUS_DEVICE_ERROR, None),
        }
    }

    fn cache_cursor_shape(
        &mut self,
        id: u8,
        bgra: &[u8],
        w: u32,
        h: u32,
        hot: (u32, u32),
    ) -> Result<(), GfxError> {
        self.cpu.cache_shape(id, bgra, w, h, hot)
    }

    /// GL/SW paths pick the new sprite up on their next present; when the hardware overlay is
    /// armed the 64×64 cursor resource is refreshed too.
    fn select_cursor_shape(&mut self, id: u8) -> Result<(), GfxError> {
        self.cpu.select_shape(id)?;
        // virgl GL scanout: the build-up samples the cursor from a GL TEXTURE initialized from
        // the sprite — refresh it or the shape switch stays invisible.
        #[cfg(feature = "virgl")]
        let _ = self.cursor_tex_refresh();
        // HW overlay armed (mmio path): refresh the cursor resource so the host-composited
        // pointer shows the new shape. Never reached on the virgl GL scanout (the overlay is not
        // armed there by design). The sprite is taken for the upload and put back (the heap never
        // frees; no clone).
        if self.hw_cursor_active() {
            let sprite = core::mem::take(&mut self.cpu.cursor);
            let (w, h, (hot_x, hot_y)) =
                (self.cpu.cursor_w, self.cpu.cursor_h, self.cpu.cursor_hot);
            let result = self.arm_hw_cursor(&sprite, w, h, hot_x, hot_y);
            self.cpu.cursor = sprite;
            return result;
        }
        Ok(())
    }

    /// The position feeds the GL scanout's cursor (the build-up draws the sprite at
    /// `cursor_ox/oy` each present — no transfer, safe on the GL scanout); windowd also presents
    /// on a move. With the hardware overlay armed the move goes to the cursor virtqueue instead
    /// (submit-no-response): no re-render, no present — cursor moves fully decoupled from
    /// compositing.
    fn move_cursor(&mut self, x: i32, y: i32) -> Result<(), GfxError> {
        self.set_pointer_pos(x, y);
        if self.hw_cursor_active() && x >= 0 && y >= 0 {
            return self.move_hw_cursor(x as u32, y as u32);
        }
        Ok(())
    }

    fn upload_icon(&mut self, bgra: &[u8], w: u32, h: u32, dst: Rect) -> Result<(), GfxError> {
        self.store_icon_sprite(bgra, w, h, dst.x, dst.y, dst.width, dst.height)
    }

    fn submit(&mut self, commands: CommittedBuffer) -> Result<(), GfxError> {
        GfxBackend::submit(self, commands).map(|_| ())
    }

    /// RECORD the override only — the loop drains the whole queued burst (latest row wins) and
    /// re-composites ONCE; presenting per request turned a fling into a backlog of full
    /// re-composites of stale positions (seconds of dead UI).
    #[cfg(feature = "virgl")]
    fn set_layer_scroll(&mut self, scroll_id: u32, src_row: u32) -> Result<bool, GfxError> {
        self.record_layer_scroll(scroll_id, src_row).map(|()| true)
    }

    /// Track C2 (the scroll generalization): record-only + the same coalesced flush.
    #[cfg(feature = "virgl")]
    fn set_layer_transform(
        &mut self,
        layer_id: u32,
        (dx, dy): (i16, i16),
        opacity: u8,
        scale_pct: u16,
    ) -> Result<bool, GfxError> {
        let transform = super::LayerTransform { dx, dy, opacity, scale_pct };
        self.record_layer_transform(layer_id, transform).map(|()| true)
    }

    #[cfg(feature = "virgl")]
    fn flush_layer_overrides(&mut self) {
        let _ = self.flush_layer_scroll();
    }

    /// windowd rewrote the wallpaper SOURCE plane (theme swap): the next build-up present
    /// re-uploads the wallpaper texture (a frame-clock tick picks it up).
    #[cfg(feature = "virgl")]
    fn wallpaper_dirty(&mut self) {
        self.wallpaper_reupload_pending = true;
    }

    fn request_reveal(&mut self) {
        self.reveal_requested = true;
    }

    fn reveal_requested(&self) -> bool {
        self.reveal_requested
    }

    fn holding_splash(&self) -> bool {
        self.is_holding_boot_splash()
    }

    /// While the boot splash is held (or the spin demo runs, or the 2D bootstrap splash
    /// breathes) gpud self-presents so the reveal gate re-evaluates the instant the desktop is
    /// ready: windowd stalls its present loop after its first frame.
    #[cfg(feature = "virgl")]
    fn pacing(&self) -> bool {
        spin_demo() || self.is_holding_boot_splash() || self.bootstrap_splash_active()
    }

    #[cfg(feature = "virgl")]
    fn frame_tick(&mut self, attached: bool, stats: &mut PresentStats) {
        // One-shot liveness proof: pin that the frame clock drives the hold.
        if !self.hold_tick_logged && self.is_holding_boot_splash() {
            self.hold_tick_logged = true;
            let _ = debug_println("gpud: hold tick alive");
        }
        if self.bootstrap_splash_active() {
            // 2D text phase (before windowd's handoff): breathe the title line so the very first
            // thing on screen already lives. ~30 Hz redraw is plenty for the slow curve; the
            // wall-clock pulse stays continuous into the GL splash after the switch.
            let now = nsec().unwrap_or(0);
            if now.saturating_sub(self.last_splash_pulse_ns) >= 33_000_000 {
                self.last_splash_pulse_ns = now;
                let _ = self.pulse_bootstrap_splash(super::splash_pulse_q8(now));
            }
        } else if (spin_demo() && attached) || self.is_holding_boot_splash() {
            // The hold-phase tick gates on `is_holding_boot_splash()` alone — holding implies
            // the GL scanout is attached; the spin demo keeps the attach gate.
            let t0 = nsec().unwrap_or(0);
            let (width, height) = (self.display_w, self.display_h);
            let _ = self.present_scanout_damage(Rect { x: 0, y: 0, width, height });
            stats.record(t0, nsec().unwrap_or(t0));
        }
    }
}
