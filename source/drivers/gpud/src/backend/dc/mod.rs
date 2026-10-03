// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The board's display — the display controller and the HDMI encoder it drives — in
//! gpud (TASK-0251 P2a, first light). init grants the display plane (`device.mmio.display`, two
//! windows in `slots::gpud::DISPLAY_*`) instead of a GPU; gpud has socd bring both nodes up
//! (power domain 7, `hdmi_reset`, `hmclk` at 491.52 MHz — RFC-0106), checks the controller's
//! version, decides the mode (`display_mode::resolve`: the lane's request or the layout maximum
//! until the EDID arrives, P2b) and drives it with its standard CEA timing, programs the encoder
//! from its measured sequence and waits for the PLL, makes the framebuffer as ONE contiguous
//! block for the controller, draws the boot splash into the display plane and cleans it out of
//! the caches, programs the controller's pipeline (`nexus_gfx::backend::dc::bring_up`) and
//! proves the scan by a moving line counter. windowd's desktop on this path is P2a's second
//! step; until then gpud names the refusal of windowd's framebuffer request.
//! OWNERS: @ui @runtime
//! STATUS: Experimental (first light)
//! API_STABILITY: Internal
//! TEST_COVERAGE: the sequences are host-tested in nexus-gfx (`backend::dc`,
//!   `tests/dc_goldens.rs`: the controller's and the encoder's measured words); this glue by the
//!   board ladder (`scripts/board-test.sh`: `gpud: dc scanout ok (`)

mod controller;
mod encoder;
mod framebuffer;
mod glue;

use nexus_display_proto::{encode_framebuffer_grant, FramebufferGrant, STATUS_DEVICE_ERROR};
use nexus_gfx::backend::dc::{cea_mode, Mode};
use nexus_ipc::{KernelServer, Server as _, Wait};
use nexus_service_topology::slots::gpud as topo;

use self::controller::Controller;
use self::encoder::Encoder;
use self::framebuffer::Framebuffer;

fn emit(line: &str) {
    let _ = nexus_abi::debug_println(line);
}

/// The live words of `len` bytes from `base` of a block, as `gpud: dc census <block>+0x<base>:`
/// lines of `offset=word` pairs (zero words left out, each line under the console's): what a
/// bring-up on hardware is judged from against the stock system's dump
/// (`docs/board/measurements/2026-09-29-display-regs/`).
fn census(bus: &nexus_driverkit::Mmio, block: &str, base: u32, len: u32) {
    use core::fmt::Write as _;
    use nexus_hal::Bus as _;
    let head = |line: &mut alloc::string::String| {
        line.clear();
        let _ = write!(line, "gpud: dc census {block}+0x{base:x}:");
    };
    let mut line = alloc::string::String::new();
    head(&mut line);
    let empty = line.len();
    for off in (0..len).step_by(4) {
        let word = bus.read((base + off) as usize);
        if word == 0 {
            continue;
        }
        let _ = write!(line, " {off:x}={word:x}");
        if line.len() > 200 {
            emit(&line);
            head(&mut line);
        }
    }
    if line.len() > empty {
        emit(&line);
    }
}

/// Whether init granted gpud the board's display plane (a device window in the controller's
/// slot) — the board; QEMU grants the GPU's window instead.
pub(crate) fn granted() -> bool {
    let mut info = nexus_abi::CapQuery::default();
    nexus_abi::cap_query(topo::DISPLAY_CONTROLLER, &mut info).is_ok() && info.kind_tag == 2
}

/// The board's display, up and scanning (held for gpud's life: dropping it would free the
/// memory the controller reads).
pub(crate) struct Display {
    _controller: Controller,
    _encoder: Encoder,
    _framebuffer: Framebuffer,
}

/// The standard CEA timing for a mode of `w`x`h` at 60 Hz (VIC 16: 1080p60, VIC 4: 720p60).
fn standard_timing(w: u32, h: u32) -> Option<Mode> {
    [16u8, 4]
        .into_iter()
        .filter_map(cea_mode)
        .find(|m| m.h_active as u32 == w && m.v_active as u32 == h)
}

/// Bring the display up and show the boot splash; `None` after a line that names the failure.
pub(crate) fn bring_up() -> Option<Display> {
    let nodes = glue::bring_up()?;
    let controller = match Controller::map() {
        Ok(c) => c,
        Err(e) => {
            emit(&alloc::format!("gpud: FAIL dc controller ({e})"));
            return None;
        }
    };
    emit("gpud: dc controller ok (version=0x03001030)");
    let encoder = match Encoder::map(nodes.encoder_offset) {
        Ok(e) => e,
        Err(e) => {
            emit(&alloc::format!("gpud: FAIL dc encoder ({e})"));
            return None;
        }
    };
    let hot_plug = encoder.hot_plug();
    let (w, h) = super::display_mode::resolve(super::display_mode::request_from_tree(), None);
    let Some(mode) = standard_timing(w, h) else {
        emit(&alloc::format!("gpud: FAIL dc mode (no standard timing for {w}x{h})"));
        return None;
    };
    if let Err(e) = encoder.enable(mode.pixel_clock_khz) {
        emit(&alloc::format!("gpud: FAIL dc encoder ({e}, hpd={})", hot_plug as u8));
        return None;
    }
    emit(&alloc::format!("gpud: dc encoder ok (hpd={} pll=locked)", hot_plug as u8));
    let mut framebuffer = match Framebuffer::make() {
        Ok(f) => f,
        Err(e) => {
            emit(&alloc::format!("gpud: FAIL dc framebuffer ({e})"));
            return None;
        }
    };
    // First light: the boot splash, drawn by the CPU into the display plane.
    let stride_px = nexus_display_proto::layout::LAYOUT_MAX.0;
    let rows = nexus_display_proto::layout::PLANE_ROWS;
    super::bootstrap::compose_splash_region(
        framebuffer.display_plane_mut(),
        stride_px,
        rows,
        0,
        0,
        w,
        h,
        256,
    );
    framebuffer.clean_display_plane();
    let plane = framebuffer.display_plane(w as u16, h as u16);
    let written = controller.program(&mode, &plane);
    let hz = (mode.refresh_mhz() + 500) / 1000;
    encoder.census();
    let scan = controller.scanning();
    report_read_back(&controller, &written);
    match scan {
        Ok(scan) => {
            let mut witness = alloc::string::String::new();
            if let Some((a, b)) = scan.lines {
                witness.push_str(&alloc::format!(" lines {a}->{b}"));
            }
            if scan.vsync {
                witness.push_str(" vsync");
            }
            emit(&alloc::format!(
                "gpud: dc scanout ok ({w}x{h}@{hz} cea bus=0x{:x}{witness})",
                plane.bus_addr
            ));
        }
        Err((stuck, raw)) => {
            emit(&alloc::format!(
                "gpud: FAIL dc scanout (line counter stuck at {stuck}, raw interrupts 0x{raw:x})"
            ));
            controller.census();
            return None;
        }
    }
    Some(Display { _controller: controller, _encoder: encoder, _framebuffer: framebuffer })
}

/// Every controller word the bring-up wrote, read back: `gpud: dc readback ok (N words)`, or
/// the words that read otherwise (shadowed configuration that never latched shows up here).
fn report_read_back(controller: &Controller, written: &nexus_gfx::backend::dc::Sequence) {
    let rb = controller.read_back(written);
    if rb.ndiffer == 0 {
        emit(&alloc::format!("gpud: dc readback ok ({} words)", rb.compared));
        return;
    }
    let mut line = alloc::format!("gpud: dc readback differs ({} of {}):", rb.ndiffer, rb.compared);
    for (off, wrote, read) in rb.differ.iter().take(rb.ndiffer.min(rb.differ.len())) {
        line.push_str(&alloc::format!(" {off:x}={wrote:x}/{read:x}"));
    }
    emit(&line);
}

/// gpud on the board (P2a, first light): the display up with the splash, then serve. windowd's
/// framebuffer request is answered with a named refusal until the desktop path lands (P2a's
/// second step); windowd then runs display-less, as on a board without this package.
pub(crate) fn service_main_loop() -> Result<(), nexus_abi::AbiError> {
    let display = bring_up();
    let server = crate::service::bind_server()?;
    nexus_service_entry::ready(crate::markers::GPUD_READY)?;
    nexus_abi::service_verdict_flush("gpud");
    let mut frame = [0u8; 256];
    loop {
        match server.recv_request_with_meta_into(Wait::Blocking, &mut frame) {
            Ok((len, _sid, moved)) => {
                if let Some(cap) = moved {
                    cap.close();
                }
                answer(
                    &server,
                    frame.get(..len).and_then(|f| f.first().copied()),
                    display.is_some(),
                );
            }
            Err(_) => {
                let _ = nexus_abi::yield_();
            }
        }
    }
}

fn answer(server: &KernelServer, op: Option<u8>, display_up: bool) {
    if op == Some(nexus_display_proto::OP_FRAMEBUFFER_REQUEST) {
        let refused =
            FramebufferGrant { status: STATUS_DEVICE_ERROR, mode_w: 0, mode_h: 0, len: 0 };
        let _ = server.send(&encode_framebuffer_grant(&refused), Wait::Blocking);
        emit(if display_up {
            "gpud: dc framebuffer request refused (the desktop path is TASK-0251 P2a step 2)"
        } else {
            "gpud: dc framebuffer request refused (the display is down)"
        });
    } else {
        let _ = server.send(&[nexus_display_proto::STATUS_MALFORMED], Wait::Blocking);
    }
}
