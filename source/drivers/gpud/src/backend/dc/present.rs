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
use super::{emit, DcDisplay};
use crate::backend::cpu_frame::Blur;
use crate::backend::display::Display;
use nexus_gfx::backend::dc::regs;

impl DcDisplay {
    /// The switch from the splash to the desktop: the whole display plane cleaned, the
    /// controller pointed at it, the words read back and the latch observed — one line. Whether
    /// the switch took.
    fn reveal(&mut self) -> bool {
        let plane_size = (self.mode.0, self.mode.1);
        let Some(live) = self.live.as_mut() else {
            return false;
        };
        let Some(fb) = live.framebuffer.as_ref() else {
            return false;
        };
        let plane = fb.plane(plane_size.0, plane_size.1);
        let whole = Rect { x: 0, y: 0, width: plane_size.0.into(), height: plane_size.1.into() };
        fb.clean(&plane, whole);
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

    fn execute(&mut self, commands: &[Command]) -> Result<(), GfxError> {
        let t0 = nsec().unwrap_or(0);
        let fb = self
            .live
            .as_mut()
            .and_then(|live| live.framebuffer.as_mut())
            .ok_or(GfxError::DeviceNotFound)?;
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
            self.cpu.execute(&target, cmd, Blur::Box)?;
        }
        if !self.first_reported {
            self.first_exec_ns = nsec().unwrap_or(t0).saturating_sub(t0);
        }
        Ok(())
    }

    fn scan_out(&mut self, damage: Rect) -> Result<(), GfxError> {
        let t0 = nsec().unwrap_or(0);
        let (w, h) = self.mode;
        let fb = self
            .live
            .as_ref()
            .and_then(|live| live.framebuffer.as_ref())
            .ok_or(GfxError::DeviceNotFound)?;
        fb.clean(&fb.plane(w, h), damage);
        if !self.first_reported {
            // The first present's cost on the board's harts, once (a stats window takes 120).
            self.first_reported = true;
            emit(&alloc::format!(
                "gpud: dc first present (exec_us={} clean_us={} damage={}x{})",
                self.first_exec_ns / 1000,
                nsec().unwrap_or(t0).saturating_sub(t0) / 1000,
                damage.width,
                damage.height
            ));
        }
        if self.hold.switch_due() {
            let took = self.reveal();
            self.hold.switched(took);
        }
        Ok(())
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
        // The witness for the eye's verdict: the channel, the composer and the control words
        // as the controller holds them once the layer is armed.
        live.controller.cursor_census();
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
        self.execute(commands.commands())
    }

    fn request_reveal(&mut self) {
        self.hold.ask();
    }

    /// RFC-0095: the controller scans the CPU-written display plane; a readback copies it. The
    /// controller never writes the block, so the CPU reads it without cache maintenance.
    fn readback(&mut self, dest: u32, req: &nexus_display_proto::readback::Readback) -> u8 {
        let (w, h) = (u32::from(self.mode.0), u32::from(self.mode.1));
        let Some(fb) = self.live.as_mut().and_then(|live| live.framebuffer.as_mut()) else {
            return nexus_display_proto::STATUS_DEVICE_ERROR;
        };
        fb.readback(dest, req, w, h)
    }

    fn reveal_requested(&self) -> bool {
        self.hold.asked()
    }

    fn holding_splash(&self) -> bool {
        self.hold.holding()
    }
}
