// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The board's HDMI encoder: its block (the page init granted, the block at the tree's in-page
//! offset), the monitor's hot-plug line, and the measured first-light sequence
//! (`nexus_gfx::backend::dc::encoder`) with a bounded wait for the PLL's lock.

use nexus_abi::MmioWindow;
use nexus_driverkit::Mmio;
use nexus_gfx::backend::dc::encoder as model;
use nexus_hal::Bus;
use nexus_service_topology::slots::gpud as topo;

/// Reads of the PLL-lock bit before the encoder is declared unlocked (bus reads, no clock: the
/// boot loader's driver allows 100 µs, a locked PLL answers within the first few reads).
const LOCK_POLL_READS: usize = 100_000;

/// Why the encoder did not come up.
#[derive(Clone, Copy, Debug)]
pub(super) enum EncoderError {
    /// The granted window is missing or does not hold the block.
    Window,
    /// No measured sequence for this pixel clock (kHz).
    Unmeasured(u32),
    /// The PLL never reported its lock (the PHY control word read last).
    NoLock(u32),
}

impl core::fmt::Display for EncoderError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            EncoderError::Window => f.write_str("no window"),
            EncoderError::Unmeasured(khz) => write!(f, "no measured sequence for {khz} kHz"),
            EncoderError::NoLock(word) => write!(f, "pll not locked, phy ctrl=0x{word:08x}"),
        }
    }
}

pub(super) struct Encoder {
    regs: Mmio,
}

impl Encoder {
    /// Map the encoder's window and find its block at `offset` inside it.
    pub(super) fn map(offset: usize) -> Result<Self, EncoderError> {
        let mut info = nexus_abi::CapQuery::default();
        nexus_abi::cap_query(topo::DISPLAY_ENCODER, &mut info).map_err(|_| EncoderError::Window)?;
        let len = usize::try_from(info.len).map_err(|_| EncoderError::Window)?;
        if info.kind_tag != 2 || len == 0 {
            return Err(EncoderError::Window);
        }
        let page =
            MmioWindow::map(topo::DISPLAY_ENCODER, 0, len).map_err(|_| EncoderError::Window)?;
        let block = page.window(offset, model::WINDOW_LEN as usize).ok_or(EncoderError::Window)?;
        Ok(Encoder { regs: Mmio::new(block) })
    }

    /// The monitor's hot-plug detect.
    pub(super) fn hot_plug(&self) -> bool {
        self.regs.read(model::PHY_STATUS as usize) & model::PHY_STATUS_HPD != 0
    }

    /// The encoder block's live words (its whole register block).
    pub(super) fn census(&self) {
        super::census(&self.regs, "encoder", 0, model::WINDOW_LEN);
    }

    /// Program the encoder for `pixel_clock_khz` from its measured sequence and wait for the PLL.
    pub(super) fn enable(&self, pixel_clock_khz: u32) -> Result<(), EncoderError> {
        let seq =
            model::sequence(pixel_clock_khz).ok_or(EncoderError::Unmeasured(pixel_clock_khz))?;
        for w in seq.iter().chain([&model::PHY_ENABLE]) {
            self.regs.write(w.offset as usize, w.value);
        }
        let ctrl = model::PHY_CTRL as usize;
        if !(0..LOCK_POLL_READS).any(|_| self.regs.read(ctrl) & model::PHY_CTRL_PLL_LOCKED != 0) {
            return Err(EncoderError::NoLock(self.regs.read(ctrl)));
        }
        self.regs.write(model::TRANSMITTER_ON.offset as usize, model::TRANSMITTER_ON.value);
        Ok(())
    }
}
