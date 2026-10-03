// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The board's display controller: its window, its version (the register map is the measured
//! version's; another is refused), the pipeline programmed from TASK-0250's model over the
//! window, and the proof that it scans — the output's line counter moves.

use nexus_abi::MmioWindow;
use nexus_driverkit::Mmio;
use nexus_gfx::backend::dc::{self, regs, Mode, Plane, RegWriter, Sequence};
use nexus_hal::Bus;
use nexus_service_topology::slots::gpud as topo;

/// The RDMA channel the plane is read through — the one the stock system used at 1080p60
/// (`0x560 = 0x00040002`, the model's golden).
const RDMA: u32 = 1;
/// Reads of the line counter before the scan is declared stuck (bus reads, no clock: one line
/// lasts ~15 µs at 1080p60, so a scanning output moves within a few reads).
const SCAN_POLL_READS: usize = 100_000;

/// Why the controller did not come up.
#[derive(Clone, Copy, Debug)]
pub(super) enum ControllerError {
    /// The granted window is missing or shorter than the register map.
    Window,
    /// A version the register map was not measured on.
    Version(u32),
}

impl core::fmt::Display for ControllerError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ControllerError::Window => f.write_str("no window"),
            ControllerError::Version(v) => {
                write!(f, "version=0x{v:08x}, the register map is 0x{:08x}'s", regs::KNOWN_VERSION)
            }
        }
    }
}

pub(super) struct Controller {
    regs: Mmio,
}

/// The model's writer over the window, recording what it wrote (for the read-back).
struct Writer<'a> {
    bus: &'a Mmio,
    written: Sequence,
}

impl RegWriter for Writer<'_> {
    fn write(&mut self, offset: u32, value: u32) {
        self.bus.write(offset as usize, value);
        self.written.write(offset, value);
    }
}

/// How the scan showed itself: the output's line counter moved (two reads apart), or the
/// pipeline's raw vsync was raised — either is the timing generator running.
#[derive(Clone, Copy, Debug)]
pub(super) struct Scan {
    pub lines: Option<(u32, u32)>,
    pub vsync: bool,
}

/// The words the bring-up wrote that read back otherwise (`offset`, wrote, read), and how many
/// were compared — the latch and start words (`CFG_READY`, `SW_START`) are commands, not state.
pub(super) struct Readback {
    pub compared: usize,
    pub differ: [(u32, u32, u32); 8],
    pub ndiffer: usize,
}

impl Controller {
    /// Map the controller's window and check its version.
    pub(super) fn map() -> Result<Self, ControllerError> {
        let mut info = nexus_abi::CapQuery::default();
        nexus_abi::cap_query(topo::DISPLAY_CONTROLLER, &mut info)
            .map_err(|_| ControllerError::Window)?;
        let len = usize::try_from(info.len).map_err(|_| ControllerError::Window)?;
        if info.kind_tag != 2 || len < regs::WINDOW_LEN as usize {
            return Err(ControllerError::Window);
        }
        let window = MmioWindow::map(topo::DISPLAY_CONTROLLER, 0, len)
            .map_err(|_| ControllerError::Window)?;
        let regs_bus = Mmio::new(window);
        let version = regs_bus.read(regs::TOP_VERSION as usize);
        if version != regs::KNOWN_VERSION {
            return Err(ControllerError::Version(version));
        }
        Ok(Controller { regs: regs_bus })
    }

    /// Program the pipeline for `mode`, scanning `plane`; returns what was written.
    pub(super) fn program(&self, mode: &Mode, plane: &Plane) -> Sequence {
        let mut w = Writer { bus: &self.regs, written: Sequence::new() };
        dc::bring_up(&mut w, mode, plane, RDMA);
        w.written
    }

    /// Read every word `written` names back (the last value per offset).
    pub(super) fn read_back(&self, written: &Sequence) -> Readback {
        let mut rb = Readback { compared: 0, differ: [(0, 0, 0); 8], ndiffer: 0 };
        for (i, w) in written.as_slice().iter().enumerate() {
            let command = w.offset == regs::CTL2_CFG_READY || w.offset == regs::CTL2_SW_START;
            let later = written.as_slice()[i + 1..].iter().any(|n| n.offset == w.offset);
            if command || later {
                continue;
            }
            rb.compared += 1;
            let read = self.regs.read(w.offset as usize);
            if read != w.value {
                if rb.ndiffer < rb.differ.len() {
                    rb.differ[rb.ndiffer] = (w.offset, w.value, read);
                }
                rb.ndiffer += 1;
            }
        }
        rb
    }

    /// The live words of the regions first light touches — the top, the control words, the
    /// clock gates, the interrupts, the DMA top, the plane's channel, composer and output control
    /// of the pipeline — for a failed scan to be judged by.
    pub(super) fn census(&self) {
        let regions = [
            (0x000, 0x100),
            (regs::DPU_CTL_BASE, 0x100),
            (0x700, 0x100),
            (0x900, 0x100),
            (regs::DMA_TOP_BASE, 0x80),
            (regs::rdma_base(RDMA), regs::RDMA_STRIDE),
            (regs::cmps_base(regs::PIPELINE), regs::CMPS_STRIDE),
            (regs::outctrl_base(regs::PIPELINE), 0x100),
        ];
        for (base, len) in regions {
            super::census(&self.regs, "controller", base, len);
        }
    }

    /// Whether the pipeline scans: the output's line counter moving or the raw vsync raised,
    /// polled within the bound. `Err` names the counter's value and the raw interrupt word.
    pub(super) fn scanning(&self) -> Result<Scan, (u32, u32)> {
        let counter = (regs::outctrl_base(regs::PIPELINE) + regs::OUTCTRL_LINE_COUNT) as usize;
        let raw = regs::INT_RAW_ONL2 as usize;
        let first = self.regs.read(counter);
        for _ in 0..SCAN_POLL_READS {
            let now = self.regs.read(counter);
            let vsync = self.regs.read(raw) & regs::INT_RAW_VSYNC != 0;
            if now != first || vsync {
                let lines = (now != first).then_some((first, now));
                return Ok(Scan { lines, vsync });
            }
        }
        Err((first, self.regs.read(raw)))
    }
}
