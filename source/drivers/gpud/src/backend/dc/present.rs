// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The board's display as the request loop's display (TASK-0251 P2a step 2): windowd's
//! framebuffer is ONE contiguous block for the controller (the whole shared layout, made at
//! bring-up, granted at windowd's first request); a present runs through the CPU executor the
//! virtio 2D path runs, then its damage is cleaned out of the caches (the controller does not
//! snoop them). The splash holds the glass until the first present after windowd's reveal: that
//! present cleans the whole display plane and switches the controller to it. The pointer is the
//! controller's own layer (`cursor.rs`, TASK-0251 P2a step 3c): windowd's upload arms it and
//! hears `CURSOR_REPLY_HW`, a move is the layer's rectangle, a `BlendCursor` in a present is
//! refused — no present carries the pointer on this path.

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
use crate::backend::cpu_frame::Blur;
use crate::backend::display::Display;
use nexus_gfx::backend::dc::frame_stats::Stage;
use nexus_gfx::backend::dc::regs;

impl DcDisplay {
    /// The switch from the splash to the desktop: the whole display plane cleaned, the
    /// controller pointed at it, the words read back and the latch observed — one line. Whether
    /// the switch took.
    fn reveal(&mut self, row: u32) -> bool {
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

    /// A present into the ring's free slot (RFC-0101 §8): screen-relative commands land on that
    /// slot's rows; what the present does not redraw comes from the frame on screen first.
    fn execute(&mut self, commands: &[Command], damage: Rect) -> Result<(), GfxError> {
        let t0 = nsec().unwrap_or(0);
        let (w, h) = (u32::from(self.mode.0), u32::from(self.mode.1));
        let slot = self.ring.begin().map_err(|_| GfxError::ResourceExhausted)?;
        let row = ring::ROWS[slot];
        let Some(fb) = self.live.as_mut().and_then(|live| live.framebuffer.as_mut()) else {
            self.ring.discard();
            return Err(GfxError::DeviceNotFound);
        };
        // The frame composed off-screen starts from its base: the retained plane — windowd's
        // cursor-free scene, kept current by its damage writes — copied whole (about 3 ms of
        // 8.3 MB; the scene's own retained blits restore only its damage, and a layer blended
        // over a plane three frames old would ghost, over the last frame it darkened: the
        // shadows of 2026-10-10). G4 deletes this composite; the copy goes with it.
        let _ = damage;
        fb.copy_plane(nexus_display_proto::layout::RETAINED_ROW, row, w, h);
        self.cpu.display_row = row;
        let target = fb.target();
        for cmd in commands {
            // The pointer is the controller's layer: a software-blended pointer in a present is
            // the second implementation this path retired — skipped and counted, the present's
            // other commands still land (cycle 12 refused whole presents and the desktop
            // stopped with the pointer). Said — the forbidden marker — only once the layer is
            // armed: windowd's first present blends its sprite before it has uploaded it (ADR-0034
            // restores that region once the overlay answers), which is the path's start, not
            // a second path.
            if matches!(cmd, Command::BlendCursor { .. }) {
                let armed =
                    self.live.as_ref().is_some_and(|l| l.cursor.as_ref().is_some_and(|c| c.armed));
                if armed && self.blend_cursor_refused == 0 {
                    emit("gpud: dc refused BlendCursor (the pointer is the controller's layer)");
                }
                if armed {
                    self.blend_cursor_refused = self.blend_cursor_refused.saturating_add(1);
                }
                continue;
            }
            let c0 = nsec().unwrap_or(0);
            self.cpu.execute(&target, cmd, Blur::Box)?;
            self.stats.command(Stage::of(cmd), nsec().unwrap_or(c0).saturating_sub(c0));
        }
        self.exec_ns = nsec().unwrap_or(t0).saturating_sub(t0);
        if !self.first_reported {
            self.first_exec_ns = self.exec_ns;
        }
        Ok(())
    }

    /// The drawn slot cleaned out of the caches and its flip queued for the next vertical
    /// blank — or, while the splash holds the glass, discarded (the reveal's present switches
    /// the controller at once, the one immediate flip).
    fn scan_out(&mut self, damage: Rect) -> Result<(), GfxError> {
        let t0 = nsec().unwrap_or(0);
        let (w, h) = self.mode;
        let Some(slot) = self.ring.drawing() else {
            return Err(GfxError::CommandRejected);
        };
        let row = ring::ROWS[slot];
        {
            let fb = self
                .live
                .as_ref()
                .and_then(|live| live.framebuffer.as_ref())
                .ok_or(GfxError::DeviceNotFound)?;
            fb.clean_at(row, &fb.plane_at(row, w, h), damage);
        }
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
                live.controller.flip_in_blank(&fb.plane_at(row, w, h));
            }
            return self.ring.shown_now().map_err(|_| GfxError::ResourceExhausted);
        }
        self.ring.drawn().map_err(|_| GfxError::ResourceExhausted)?;
        // A flip waits for the blank: the interrupt is live until it lands.
        live.controller.vblank_enable();
        Ok(())
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
                    live.controller.flip_in_blank(&fb.plane_at(row, w, h));
                    if !vb.flip_reported {
                        vb.flip_reported = true;
                        emit(&alloc::format!("gpud: dc flip ok (slot={slot} line={line})"));
                    }
                    landed = self.ring.landed();
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
        self.cpu.cache_shape(id, bgra, w, h, hot)
    }

    /// `OP_SELECT_CURSOR_SHAPE`: the cached shape into the pointer's block, the layer moved to
    /// the new hotspot's rectangle (a shape change is a few bus writes, no present).
    fn select_cursor_shape(&mut self, id: u8) -> Result<(), GfxError> {
        let mode = self.mode;
        let (bgra, w, h, hot) = self.cpu.shape(id).ok_or(GfxError::InvalidArgument)?;
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

    fn request_reveal(&mut self) {
        self.hold.ask();
    }

    /// RFC-0095: the controller scans the CPU-written display plane; a readback copies it. The
    /// controller never writes the block, so the CPU reads it without cache maintenance.
    fn readback(&mut self, dest: u32, req: &nexus_display_proto::readback::Readback) -> u8 {
        let (w, h) = (u32::from(self.mode.0), u32::from(self.mode.1));
        let front = self.ring.scanned_row().unwrap_or(nexus_display_proto::layout::DISPLAY_ROW);
        let Some(fb) = self.live.as_mut().and_then(|live| live.framebuffer.as_mut()) else {
            return nexus_display_proto::STATUS_DEVICE_ERROR;
        };
        fb.readback(dest, req, w, h, front)
    }

    fn reveal_requested(&self) -> bool {
        self.hold.asked()
    }

    fn holding_splash(&self) -> bool {
        self.hold.holding()
    }
}
