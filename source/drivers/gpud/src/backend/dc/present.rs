// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The board's display as the request loop's display (TASK-0251 P2a step 2): windowd's
//! framebuffer is ONE contiguous block for the controller (the whole shared layout, made at
//! bring-up, granted at windowd's first request); a present runs through the CPU executor the
//! virtio 2D path runs, then its damage is cleaned out of the caches (the controller does not
//! snoop them). The splash holds the glass until the first present after windowd's reveal: that
//! present cleans the whole display plane and switches the controller to it. The cursor is
//! windowd's software sprite (`BlendCursor` in its presents); the controller's cursor layer is
//! not measured yet.

use nexus_abi::{debug_println, nsec};
use nexus_display_proto::{CURSOR_REPLY_SW, STATUS_DEVICE_ERROR, STATUS_MALFORMED, STATUS_OK};
use nexus_gfx::backend::error::GfxError;
use nexus_gfx::backend::types::Rect;
use nexus_gfx::command::buffer::{Command, CommittedBuffer};

use super::framebuffer::Framebuffer;
use super::{emit, DcDisplay};
use crate::backend::cpu_frame::Blur;
use crate::backend::display::Display;

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

    /// The software cursor: windowd blends the sprite into its presents (`BlendCursor`).
    fn upload_cursor(&mut self, bgra: &[u8], w: u32, h: u32, hot: (u32, u32)) -> (u8, Option<u32>) {
        self.cpu.cursor_hot = hot;
        match self.cpu.store_cursor(bgra, w, h) {
            Ok(()) => {
                let _ = debug_println("gpud: cursor uploaded");
                (STATUS_OK, Some(CURSOR_REPLY_SW))
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

    fn select_cursor_shape(&mut self, id: u8) -> Result<(), GfxError> {
        self.cpu.select_shape(id)
    }

    /// windowd composites the software cursor where the pointer is; nothing to move here.
    fn move_cursor(&mut self, _x: i32, _y: i32) -> Result<(), GfxError> {
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

    fn reveal_requested(&self) -> bool {
        self.hold.asked()
    }

    fn holding_splash(&self) -> bool {
        self.hold.holding()
    }
}
