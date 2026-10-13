// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The GPU's register window as `nexus_gfx::backend::gpu::start::Regs` (TASK-0329 G1): the
//! granted window mapped whole, 32- and 64-bit words accessed whole (a reset word written as two
//! halves would release every reset between them), a microsecond delay on the kernel clock (the
//! start sequence's two 3 µs holds — below any timer's grain; a bounded busy wait on `nsec`,
//! never on an event), polls bounded by the same clock.

use nexus_driverkit::Mmio;
use nexus_gfx::backend::gpu::start::{cr, Regs, Timeout};
use nexus_hal::Bus as _;
use nexus_service_topology::DEVICE_MMIO_SLOT;

pub(super) struct Window {
    bus: Mmio,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum WindowError {
    /// The window is not granted, or shorter than the register map.
    Missing,
}

impl core::fmt::Display for WindowError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("no window")
    }
}

fn now_ns() -> u64 {
    nexus_abi::nsec().unwrap_or(0)
}

impl Window {
    /// Map the granted GPU window.
    pub(super) fn map() -> Result<Self, WindowError> {
        let window = nexus_abi::MmioWindow::map(DEVICE_MMIO_SLOT, 0, cr::WINDOW_LEN as usize)
            .map_err(|_| WindowError::Missing)?;
        Ok(Self { bus: Mmio::new(window) })
    }

    /// The core's packed id (the 64-bit id register).
    pub(super) fn core_id(&self) -> u64 {
        self.bus.read64(cr::CORE_ID_PBVNC as usize)
    }

    /// A few registers for a FAIL line: `(MIPS_EXCEPTION_STATUS, MARS_IDLE, EVENT_STATUS,
    /// BIF_FAULT_BANK0_MMU_STATUS, BIF_FAULT_BANK0_REQ_STATUS, MMU_FAULT_STATUS)`.
    pub(super) fn fault_witness(&self) -> (u32, u32, u32, u32, u64, u32) {
        (
            self.bus.read(cr::MIPS_EXCEPTION_STATUS as usize),
            self.bus.read(cr::MARS_IDLE as usize),
            self.bus.read(cr::EVENT_STATUS as usize),
            self.bus.read(cr::BIF_FAULT_BANK0_MMU_STATUS as usize),
            self.bus.read64(cr::BIF_FAULT_BANK0_REQ_STATUS as usize),
            self.bus.read(cr::MMU_FAULT_STATUS as usize),
        )
    }
}

impl Regs for Window {
    fn write32(&mut self, offset: u32, value: u32) {
        self.bus.write(offset as usize, value);
    }
    fn write64(&mut self, offset: u32, value: u64) {
        self.bus.write64(offset as usize, value);
    }
    fn read32(&mut self, offset: u32) -> u32 {
        self.bus.read(offset as usize)
    }
    fn read64(&mut self, offset: u32) -> u64 {
        self.bus.read64(offset as usize)
    }
    fn delay_us(&mut self, us: u32) {
        let until = now_ns().saturating_add(u64::from(us) * 1000);
        // Bounded by the clock itself; a clock that does not advance ends it after a fixed
        // number of reads (never an endless spin).
        for _ in 0..1_000_000u32 {
            if now_ns() >= until {
                break;
            }
            core::hint::spin_loop();
        }
    }
    fn poll32(
        &mut self,
        offset: u32,
        mask: u32,
        expected: u32,
        timeout_us: u32,
    ) -> Result<(), Timeout> {
        let until = now_ns().saturating_add(u64::from(timeout_us) * 1000);
        let mut last = self.read32(offset);
        while last & mask != expected {
            if now_ns() >= until {
                return Err(Timeout { offset, last: u64::from(last) });
            }
            core::hint::spin_loop();
            last = self.read32(offset);
        }
        Ok(())
    }
    fn poll64(
        &mut self,
        offset: u32,
        mask: u64,
        expected: u64,
        timeout_us: u32,
    ) -> Result<(), Timeout> {
        let until = now_ns().saturating_add(u64::from(timeout_us) * 1000);
        let mut last = self.read64(offset);
        while last & mask != expected {
            if now_ns() >= until {
                return Err(Timeout { offset, last });
            }
            core::hint::spin_loop();
            last = self.read64(offset);
        }
        Ok(())
    }
}
