// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The board's display — the display controller and the HDMI encoder it drives — as
//! gpud's display (TASK-0251 P2a). init grants the display plane (`device.mmio.display`, two
//! windows in `slots::gpud::DISPLAY_*`) instead of a GPU; gpud has socd bring both nodes up
//! (power domain 7, `hdmi_reset`, `hmclk` at 491.52 MHz — RFC-0106), checks the controller's
//! version, decides the mode (`display_mode::resolve`: the lane's request or the layout maximum
//! until the EDID arrives, P2b) and drives it with its standard CEA timing, programs the encoder
//! from its measured sequence and waits for the PLL, draws the boot splash into a plane of its
//! own, cleans it out of the caches, programs the controller's pipeline
//! (`nexus_gfx::backend::dc::bring_up`) and proves the scan (step 1, first light). windowd's
//! desktop (step 2, `present.rs`): the framebuffer granted to windowd is ONE contiguous block for
//! the controller, a present runs through the CPU executor the 2D virtio path runs
//! (`cpu_frame`) and its damage is cleaned out of the caches; the splash holds the glass until
//! the first present after windowd's reveal, which switches the controller to the framebuffer's
//! display plane (RFC-0093 §5, `splash_hold`).
//! OWNERS: @ui @runtime
//! STATUS: Experimental (the desktop path, step 2)
//! API_STABILITY: Internal
//! TEST_COVERAGE: the sequences, the switch and the damage runs are host-tested in nexus-gfx
//!   (`backend::dc`, `tests/dc_goldens.rs`), the hold in `splash_hold`; this glue by the board
//!   ladder (`scripts/board-test.sh`: `gpud: dc scanout ok (`, `gpud: dc reveal flip ok (`,
//!   `windowd: desktop revealed`)

mod controller;
mod encoder;
mod framebuffer;
mod glue;
mod present;

use nexus_gfx::backend::dc::{cea_mode, Mode};
use nexus_service_topology::slots::gpud as topo;

use self::controller::Controller;
use self::encoder::Encoder;
use self::framebuffer::Framebuffer;
use super::cpu_frame::CpuFrame;
use super::splash_hold::SplashHold;

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

/// The board's display as gpud's [`super::display::Display`].
pub(crate) struct DcDisplay {
    /// The pipeline up and scanning the splash; `None` after a bring-up that named its failure —
    /// the display then refuses windowd's framebuffer and windowd runs display-less.
    live: Option<Live>,
    /// The mode the controller drives.
    mode: (u16, u16),
    /// What the CPU executor reads besides the framebuffer (the cursor sprite windowd blends).
    cpu: CpuFrame,
    /// RFC-0093 §5: the splash holds until the first present after windowd's reveal.
    hold: SplashHold,
    /// The first present's cost, printed once with its clean (`gpud: dc first present (`).
    first_exec_ns: u64,
    first_reported: bool,
}

struct Live {
    controller: Controller,
    _encoder: Encoder,
    /// The boot splash the controller scans until the reveal. Kept for gpud's life: the
    /// controller reads it until the switch latches at a frame boundary, and nothing needs the
    /// memory back.
    _splash: Framebuffer,
    /// windowd's framebuffer, granted at its first request. Made at bring-up, right after the
    /// splash: the controller needs ONE run of the whole layout (99.5 MB at 1080p), found most
    /// surely before the fleet takes memory. `None` after a named failure — the splash stays,
    /// the grant is refused by name.
    framebuffer: Option<Framebuffer>,
}

/// The standard CEA timing for a mode of `w`x`h` at 60 Hz (VIC 16: 1080p60, VIC 4: 720p60).
fn standard_timing(w: u32, h: u32) -> Option<Mode> {
    [16u8, 4]
        .into_iter()
        .filter_map(cea_mode)
        .find(|m| m.h_active as u32 == w && m.v_active as u32 == h)
}

impl DcDisplay {
    /// Bring the display up and show the boot splash. A failure is named on the console and
    /// leaves a display that refuses the grant.
    pub(crate) fn bring_up() -> Self {
        let (w, h) = super::display_mode::resolve(super::display_mode::request_from_tree(), None);
        let mode = (u16::try_from(w).unwrap_or(0), u16::try_from(h).unwrap_or(0));
        DcDisplay {
            live: Live::bring_up(w, h),
            mode,
            cpu: CpuFrame::new(),
            hold: SplashHold::default(),
            first_exec_ns: 0,
            first_reported: false,
        }
    }
}

impl Live {
    fn bring_up(w: u32, h: u32) -> Option<Live> {
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
        let Some(mode) = standard_timing(w, h) else {
            emit(&alloc::format!("gpud: FAIL dc mode (no standard timing for {w}x{h})"));
            return None;
        };
        if let Err(e) = encoder.enable(mode.pixel_clock_khz) {
            emit(&alloc::format!("gpud: FAIL dc encoder ({e}, hpd={})", hot_plug as u8));
            return None;
        }
        emit(&alloc::format!("gpud: dc encoder ok (hpd={} pll=locked)", hot_plug as u8));
        let (pw, ph) = (mode.h_active, mode.v_active);
        let mut splash = match Framebuffer::splash(pw, ph) {
            Ok(f) => f,
            Err(e) => {
                emit(&alloc::format!("gpud: FAIL dc splash ({e})"));
                return None;
            }
        };
        // First light: the boot splash, drawn by the CPU into its own plane.
        super::bootstrap::compose_splash_region(splash.plane_mut(), w, h, 0, 0, w, h, 256);
        let plane = splash.plane(pw, ph);
        splash.clean(&plane, nexus_gfx::backend::types::Rect { x: 0, y: 0, width: w, height: h });
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
        let framebuffer = match Framebuffer::layout() {
            Ok(fb) => Some(fb),
            Err(e) => {
                emit(&alloc::format!("gpud: FAIL dc framebuffer ({e})"));
                None
            }
        };
        Some(Live { controller, _encoder: encoder, _splash: splash, framebuffer })
    }
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
