// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The board's display as the request loop's display (TASK-0251 P2a step 2): windowd's
//! framebuffer is ONE contiguous block for the controller (the whole shared layout, made at
//! bring-up, granted at windowd's first request); a present is drawn by the GPU in windowd's
//! context into the ring's free slot (TASK-0215 5c-3 P4b, `gpu_frame.rs`) — or refused by name:
//! nothing composes on the CPU here (TASK-0215 5d deleted the CPU executor from this display;
//! the controller does not snoop the caches, and what the GPU wrote needs no clean). The splash
//! holds the glass until the first present after windowd's reveal: that present cleans the
//! whole display plane and switches the controller to it. The pointer is the controller's own
//! layer (`cursor.rs`, TASK-0251 P2a step 3c): windowd's upload arms it and hears
//! `CURSOR_REPLY_HW`, a move is the layer's rectangle, a `BlendCursor` in a present is refused
//! — no present carries the pointer on this path.

use nexus_abi::{debug_println, nsec};
use nexus_display_proto::{CURSOR_REPLY_HW, STATUS_DEVICE_ERROR, STATUS_MALFORMED, STATUS_OK};
use nexus_gfx::backend::error::GfxError;
use nexus_gfx::backend::types::Rect;
use nexus_gfx::command::buffer::{Command, CommittedBuffer};

use super::cursor::CursorLayer;
use super::framebuffer::Framebuffer;
use super::{emit, ring, DcDisplay};

/// Notifications drained per wake (xhcid's pattern): a coalesced burst, never a spin.
const VBLANK_DRAIN_MAX: u32 = 8;
use crate::backend::display::Display;
use nexus_gfx::backend::dc::regs;

impl DcDisplay {
    /// A present the GPU did not draw, or one that found no free plane (TASK-0338: a frame
    /// beyond the compositor's budget), is refused — said once with its reason, never composed
    /// another way (TASK-0215 5d: the board's display has one drawing path).
    fn refuse(&mut self, reason: &str) -> GfxError {
        if !self.refused_reported {
            self.refused_reported = true;
            emit(&alloc::format!("gpud: FAIL dc present ({reason})"));
        }
        self.ring.discard();
        GfxError::Unsupported
    }

    /// The pointer is the controller's layer: a software-blended pointer in a present is the
    /// second implementation this path retired — skipped and counted, the present's other
    /// commands still land (cycle 12 refused whole presents and the desktop stopped with the
    /// pointer). Said — the forbidden marker — only once the layer is armed: windowd's first
    /// present blends its sprite before it has uploaded it (ADR-0034 restores that region once
    /// the overlay answers), which is the path's start, not a second path.
    fn blend_cursor_skipped(&mut self) {
        let armed = self.live.as_ref().is_some_and(|l| l.cursor.as_ref().is_some_and(|c| c.armed));
        if armed && self.blend_cursor_refused == 0 {
            emit("gpud: dc refused BlendCursor (the pointer is the controller's layer)");
        }
        if armed {
            self.blend_cursor_refused = self.blend_cursor_refused.saturating_add(1);
        }
    }

    /// The switch from the splash to the desktop: the whole display plane cleaned, the
    /// controller pointed at it, the words read back and the latch observed — one line. Whether
    /// the switch took.
    pub(super) fn reveal(&mut self, row: u32) -> bool {
        let plane_size = (self.mode.0, self.mode.1);
        let Some(live) = self.live.as_mut() else {
            return false;
        };
        let Some(fb) = live.framebuffer.as_ref() else {
            return false;
        };
        let plane = fb.plane_at(row, plane_size.0, plane_size.1);
        let whole = Rect { x: 0, y: 0, width: plane_size.0.into(), height: plane_size.1.into() };
        fb.clean_at(row, &plane, whole);
        let flip = live.controller.flip(&plane);
        let rb = &flip.readback;
        if rb.ndiffer != 0 {
            let mut line = alloc::format!(
                "gpud: FAIL dc reveal flip (readback {} of {}:",
                rb.ndiffer,
                rb.compared
            );
            for (off, wrote, read) in rb.differ.iter().take(rb.ndiffer.min(rb.differ.len())) {
                line.push_str(&alloc::format!(" {off:x}={wrote:x}/{read:x}"));
            }
            line.push(')');
            emit(&line);
            return false;
        }
        let latch = match flip.latched_after {
            Some(reads) => alloc::format!("cfg-ready cleared after {reads} reads"),
            None => alloc::string::String::from("cfg-ready still set"),
        };
        emit(&alloc::format!(
            "gpud: dc reveal flip ok (bus=0x{:x} stride={} readback {}/{} {latch})",
            plane.bus_addr,
            plane.stride,
            rb.compared,
            rb.compared
        ));
        true
    }
}

impl Display for DcDisplay {
    fn mode(&self) -> (u32, u32) {
        (self.mode.0.into(), self.mode.1.into())
    }

    fn framebuffer(&mut self) -> Option<u32> {
        let Some(live) = self.live.as_ref() else {
            emit("gpud: dc framebuffer refused (the display is down)");
            return None;
        };
        let handle = live.framebuffer.as_ref().map(Framebuffer::handle);
        if handle.is_none() {
            emit("gpud: dc framebuffer refused (no block for the controller)");
        }
        handle
    }

    /// The first frame is composed into the framebuffer; the splash keeps the glass until the
    /// reveal switches the controller to it.
    fn attach(&mut self) -> Result<(), u8> {
        let Some(live) = self.live.as_ref() else {
            return Err(STATUS_DEVICE_ERROR);
        };
        if live.framebuffer.is_none() {
            let _ = debug_println("gpud: FAIL attach before a grant");
            return Err(STATUS_MALFORMED);
        }
        Ok(())
    }

    fn execute(&mut self, commands: &[Command], damage: Rect) -> Result<(), GfxError> {
        self.execute_on(None, commands, damage)
    }

    /// A present into the ring's free slot (RFC-0101 §8): drawn by the GPU in windowd's context
    /// (TASK-0215 5c-3 P4b) — refused by name when no context is open or the GPU did not draw
    /// it (TASK-0215 5d: no composite stands in; the ring's slot is given back).
    fn execute_on(
        &mut self,
        frames: Option<&mut dyn crate::gpu_context::FrameGpu>,
        commands: &[Command],
        damage: Rect,
    ) -> Result<(), GfxError> {
        let _ = damage;
        let t0 = nsec().unwrap_or(0);
        // Presents are the ring's only frames and windowd keeps at most two in flight (TASK-0338),
        // so a ring without a free plane is a broken budget: refused by name, never silently.
        let Ok(slot) = self.ring.begin() else {
            return Err(self.refuse("no free plane: a frame beyond the compositor's budget"));
        };
        if self.live.as_ref().and_then(|live| live.framebuffer.as_ref()).is_none() {
            self.ring.discard();
            return Err(GfxError::DeviceNotFound);
        }
        let Some(frames) = frames else {
            return Err(self.refuse("windowd's GPU context is not open"));
        };
        if !self.draw_on_gpu(frames, commands, slot) {
            return Err(self.refuse("the GPU did not draw the frame"));
        }
        for cmd in commands {
            self.stats.count(cmd);
            if matches!(cmd, Command::BlendCursor { .. }) {
                self.blend_cursor_skipped();
            }
        }
        self.exec_ns = nsec().unwrap_or(t0).saturating_sub(t0);
        if !self.first_reported {
            self.first_exec_ns = self.exec_ns;
        }
        Ok(())
    }

    /// The drawn slot's flip queued for the next vertical blank (the GPU's frame is in memory:
    /// nothing to clean) — or, while the splash holds the glass, discarded (the reveal's present
    /// switches the controller at once, the one immediate flip).
    fn scan_out(&mut self, damage: Rect) -> Result<(), GfxError> {
        let t0 = nsec().unwrap_or(0);
        // A GPU frame's damage is what its slot's pass redrew, not what the present named.
        let damage = self.gpu_area.take().unwrap_or(damage);
        let (w, h) = self.mode;
        let Some(slot) = self.ring.drawing() else {
            return Err(GfxError::CommandRejected);
        };
        let row = ring::ROWS[slot];
        // The tear gate (RFC-0101 §8): the plane on screen is never drawn into. By the ring's
        // construction this cannot happen; the FAIL form keeps the lane honest if it does.
        let front = self.ring.scanned() == Some(slot);
        if front && !self.front_reported {
            self.front_reported = true;
            emit(&alloc::format!("gpud: FAIL dc present front (slot={slot} drawn while scanned)"));
        }
        let clean_ns = nsec().unwrap_or(t0).saturating_sub(t0);
        let damage_px = u64::from(damage.width) * u64::from(damage.height);
        if let Some(window) = self.stats.present(self.exec_ns, clean_ns, damage_px, front) {
            emit(&alloc::format!("{window}"));
            emit(&alloc::format!("{}", window.mix()));
        }
        if !self.first_reported {
            // The first present's cost on the board's harts, once (a stats window takes 32).
            self.first_reported = true;
            emit(&alloc::format!(
                "gpud: dc first present (exec_us={} clean_us={} damage={}x{})",
                self.first_exec_ns / 1000,
                clean_ns / 1000,
                damage.width,
                damage.height
            ));
        }
        self.ring.damage(damage);
        if self.hold.holding() {
            if self.hold.switch_due() {
                let took = self.reveal(row);
                self.hold.switched(took);
                if took {
                    let _ = self.ring.shown_now();
                } else {
                    self.ring.discard();
                }
            } else {
                // Composed while the splash holds the glass: never shown.
                self.ring.discard();
            }
            return Ok(());
        }
        let Some(live) = self.live.as_ref() else {
            return Err(GfxError::DeviceNotFound);
        };
        if self.vblank.is_none() {
            // No interrupt (named at the bind): the flip is written now — between two complete
            // frames, never half-drawn — and the present is acked now.
            if let Some(fb) = live.framebuffer.as_ref() {
                let _ = live.controller.flip_in_blank(&fb.plane_at(row, w, h));
            }
            return self.ring.shown_now().map_err(|_| GfxError::ResourceExhausted);
        }
        self.ring.drawn().map_err(|_| GfxError::ResourceExhausted)?;
        // A flip waits for the blank: the interrupt is live until it lands.
        live.controller.vblank_enable();
        Ok(())
    }

    fn gpu_canvas(&mut self) -> Option<crate::backend::display::GpuCanvas> {
        self.canvas_begin()
    }

    fn gpu_canvas_bytes(&mut self, row: u32) -> Option<&mut [u8]> {
        self.canvas_bytes(row)
    }

    fn gpu_canvas_show(&mut self, row: u32) -> Result<(), GfxError> {
        self.canvas_show(row)
    }

    fn gpu_canvas_discard(&mut self, _row: u32) {
        self.canvas_discard();
    }

    /// A present whose flip is queued or waiting is acked when it lands (`vblank_wake`).
    fn present_ack_deferred(&mut self, seq: u32, status: u8) -> bool {
        self.vblank.is_some() && self.ring.attach_ack((seq, status))
    }

    /// The vertical blank: the notification drained, the queued flip written if the scan is
    /// inside the blank (else deferred to the next blank, counted late), the blank acknowledged
    /// at the controller and the kernel, the ring advanced — the present that landed.
    fn vblank_wake(&mut self) -> Option<(u32, u8)> {
        let vb = self.vblank.as_mut()?;
        let mut drained = 0u32;
        let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut note = [0u8; 16];
        while drained < VBLANK_DRAIN_MAX
            && nexus_abi::ipc_recv_v1_nb(vb.slot, &mut hdr, &mut note, true).is_ok()
        {
            drained += 1;
        }
        if drained == 0 {
            return None;
        }
        let now = nsec().unwrap_or(0);
        let (w, h) = self.mode;
        let live = self.live.as_ref()?;
        let line = live.controller.line();
        if vb.n == 0 {
            vb.first_line = line;
        } else {
            vb.min_period_us = vb.min_period_us.min(now.saturating_sub(vb.last_ns) / 1000);
        }
        vb.n = vb.n.saturating_add(1);
        vb.last_ns = now;
        let mut landed = None;
        match self.ring.flip_pending() {
            Some(slot) if nexus_gfx::backend::dc::in_blank(line, u32::from(h)) => {
                if let Some(fb) = live.framebuffer.as_ref() {
                    let row = ring::ROWS[slot];
                    let seen = live.controller.flip_in_blank(&fb.plane_at(row, w, h));
                    if !vb.flip_reported {
                        vb.flip_reported = true;
                        emit(&alloc::format!("gpud: dc flip ok (slot={slot} line={line})"));
                    }
                    vb.flips = vb.flips.saturating_add(1);
                    vb.pending += u32::from(seen.pending);
                    // The controller takes the plane at a later frame boundary (the latch
                    // still set, about one flip in seven on the board — cycle 34): the old
                    // plane is scanned for one more frame, so the flip stays queued and lands
                    // at the next blank; freeing the old slot now would have the next frame
                    // drawn into a plane on screen — the tear of cycles 33–34.
                    let taken = !seen.pending;
                    vb.crossed +=
                        u32::from(!nexus_gfx::backend::dc::in_blank(seen.line_after, u32::from(h)));
                    vb.line_after_max = vb.line_after_max.max(seen.line_after);
                    if vb.flips % 64 == 0 {
                        // `wakes`/`late`: blank wakes served and those served after the blank
                        // had passed (the flip waits a whole frame more) — the measurement of
                        // whether the flip waits on gpud's own wake latency.
                        emit(&alloc::format!(
                            "gpud: dc flips (n={} pending={} crossed={} line_after_max={} \
                             wakes={} late={})",
                            vb.flips,
                            vb.pending,
                            vb.crossed,
                            vb.line_after_max,
                            vb.n,
                            vb.late
                        ));
                    }
                    if taken {
                        landed = self.ring.landed();
                    } else {
                        self.ring.passed();
                    }
                }
            }
            Some(_) => {
                vb.late = vb.late.saturating_add(1);
                self.ring.passed();
            }
            None => self.ring.passed(),
        }
        live.controller.vblank_ack();
        if self.ring.flip_pending().is_none() {
            // Nothing waits for the next blank: the idle display wakes nobody.
            live.controller.vblank_disable();
        }
        let _ = nexus_abi::irq_complete(vb.irq);
        if !vb.reported && vb.n >= super::VBLANK_WITNESS_N {
            vb.reported = true;
            emit(&alloc::format!(
                "gpud: dc vblank ok (min_period_us={} line={} late={} n={})",
                vb.min_period_us,
                vb.first_line,
                vb.late,
                vb.n
            ));
        }
        landed.and_then(|d| d.ack)
    }

    /// `OP_UPLOAD_CURSOR`: the sprite into the pointer's block, the layer armed on the composer
    /// at the pointer's position, the words read back — `CURSOR_REPLY_HW`, so windowd moves the
    /// pointer with `OP_MOVE_CURSOR` and blends nothing. A failure is named and refused: no
    /// software pointer stands in (the operator sees no arrow, the lane sees the FAIL).
    fn upload_cursor(&mut self, bgra: &[u8], w: u32, h: u32, hot: (u32, u32)) -> (u8, Option<u32>) {
        let mode = self.mode;
        let Some(live) = self.live.as_mut() else {
            return (STATUS_DEVICE_ERROR, None);
        };
        if live.cursor.is_none() {
            match CursorLayer::make() {
                Ok(layer) => live.cursor = Some(layer),
                Err(e) => {
                    emit(&alloc::format!("gpud: FAIL dc cursor layer (block: {e})"));
                    return (STATUS_DEVICE_ERROR, None);
                }
            }
        }
        let Some(layer) = live.cursor.as_mut() else {
            return (STATUS_DEVICE_ERROR, None);
        };
        if layer.store(bgra, w, h, hot).is_err() {
            emit(&alloc::format!("gpud: FAIL dc cursor layer (sprite {w}x{h} does not fit 64x64)"));
            return (STATUS_MALFORMED, None);
        }
        let Some(rect) = layer.rect(mode.0, mode.1) else {
            return (STATUS_DEVICE_ERROR, None);
        };
        let rb = live.controller.cursor_on(&layer.plane(), &rect);
        if rb.ndiffer != 0 {
            let mut line = alloc::format!(
                "gpud: FAIL dc cursor layer (readback {} of {}:",
                rb.ndiffer,
                rb.compared
            );
            for (off, wrote, read) in rb.differ.iter().take(rb.ndiffer.min(rb.differ.len())) {
                line.push_str(&alloc::format!(" {off:x}={wrote:x}/{read:x}"));
            }
            line.push(')');
            emit(&line);
            return (STATUS_DEVICE_ERROR, None);
        }
        layer.armed = true;
        emit(&alloc::format!(
            "gpud: dc cursor layer ok (rdma={} layer={} fmt={:#x} blend={} sprite={w}x{h} hot={},{} readback {}/{})",
            regs::CURSOR_RDMA,
            regs::CURSOR_LAYER,
            regs::RDMA_FORMAT_ARGB8888,
            regs::CURSOR_BLEND_MODE,
            hot.0,
            hot.1,
            rb.compared,
            rb.compared
        ));
        (STATUS_OK, Some(CURSOR_REPLY_HW))
    }

    fn cache_cursor_shape(
        &mut self,
        id: u8,
        bgra: &[u8],
        w: u32,
        h: u32,
        hot: (u32, u32),
    ) -> Result<(), GfxError> {
        self.shapes.cache(id, bgra, w, h, hot)
    }

    /// `OP_SELECT_CURSOR_SHAPE`: the cached shape into the pointer's block, the layer moved to
    /// the new hotspot's rectangle (a shape change is a few bus writes, no present).
    fn select_cursor_shape(&mut self, id: u8) -> Result<(), GfxError> {
        let mode = self.mode;
        let (bgra, w, h, hot) = self.shapes.get(id).ok_or(GfxError::InvalidArgument)?;
        let live = self.live.as_mut().ok_or(GfxError::DeviceNotFound)?;
        let layer = live.cursor.as_mut().filter(|l| l.armed).ok_or(GfxError::DeviceNotFound)?;
        layer.store(bgra, w, h, hot)?;
        match layer.rect(mode.0, mode.1) {
            Some(rect) => live.controller.cursor_move(&rect),
            None => live.controller.cursor_off(),
        }
        Ok(())
    }

    /// `OP_MOVE_CURSOR`: the layer's rectangle at the pointer — the fast path (no present).
    fn move_cursor(&mut self, x: i32, y: i32) -> Result<(), GfxError> {
        let mode = self.mode;
        let live = self.live.as_mut().ok_or(GfxError::DeviceNotFound)?;
        let layer = live.cursor.as_mut().filter(|l| l.armed).ok_or(GfxError::DeviceNotFound)?;
        match layer.place(x, y, mode.0, mode.1) {
            Some(rect) => live.controller.cursor_move(&rect),
            None => live.controller.cursor_off(),
        }
        Ok(())
    }

    /// No sprite layer of the controller's own is driven yet.
    fn upload_icon(&mut self, _bgra: &[u8], _w: u32, _h: u32, _dst: Rect) -> Result<(), GfxError> {
        Err(GfxError::Unsupported)
    }

    fn submit(&mut self, commands: CommittedBuffer) -> Result<(), GfxError> {
        commands.validate().map_err(crate::backend::map_nexus_error)?;
        let whole = Rect { x: 0, y: 0, width: self.mode.0.into(), height: self.mode.1.into() };
        self.execute(commands.commands(), whole)?;
        self.scan_out(whole)
    }

    /// windowd's reveal: the wallpaper plane is written (RFC-0093 §5) — the GPU's base is
    /// cleaned again before its next frame.
    fn request_reveal(&mut self) {
        self.hold.ask();
        self.base_dirty = true;
    }

    /// The thaw after a capture, or a new wallpaper: the wallpaper plane is the GPU's base again,
    /// cleaned before its next frame.
    fn wallpaper_dirty(&mut self) {
        self.frozen = false;
        self.base_dirty = true;
    }

    /// RFC-0095: a readback copies the frame on screen; the CPU reads a plane the GPU drew
    /// after dropping its stale lines (the controller never writes the block). A freeze copies
    /// the frame into the retained plane, the base the GPU's scene starts from until the thaw.
    fn readback(&mut self, dest: u32, req: &nexus_display_proto::readback::Readback) -> u8 {
        use nexus_display_proto::readback::READBACK_FREEZE;
        let (w, h) = (u32::from(self.mode.0), u32::from(self.mode.1));
        let front = self.ring.scanned_row().unwrap_or(nexus_display_proto::layout::DISPLAY_ROW);
        self.front_for_cpu();
        let Some(fb) = self.live.as_mut().and_then(|live| live.framebuffer.as_mut()) else {
            return nexus_display_proto::STATUS_DEVICE_ERROR;
        };
        let status = fb.readback(dest, req, w, h, front);
        if status == STATUS_OK && req.flags & READBACK_FREEZE != 0 {
            self.frozen = true;
            self.base_dirty = true;
        }
        status
    }

    fn reveal_requested(&self) -> bool {
        self.hold.asked()
    }

    fn holding_splash(&self) -> bool {
        self.hold.holding()
    }
}
