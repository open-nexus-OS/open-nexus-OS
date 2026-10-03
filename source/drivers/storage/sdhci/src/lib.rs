// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the SDHCI host + eMMC driver core (TASK-0246 P2, RFC-0098 C5, ADR-0067). Pure:
//! it speaks to the controller through `nexus_hal::Bus` (aligned 32-bit words only) and to
//! its environment through [`Platform`] (a clock, a wait for the interrupt), so the host is
//! the oracle — `tests/` drive the whole driver against a behavioural controller and card
//! model with an exact non-coherent cache. Two layers: the standard core (reset, power, the
//! clock divider, the command engine with R1/R1b/R2/R3, PIO and ADMA2 data with 32-bit
//! descriptors built from `DmaRun`s, line resets after an error) and the K1 layer ([`k1`]:
//! the vendor PHY registers, HS400 enhanced strobe with the DLL). [`Card::init`] takes an
//! eMMC from power-up to HS52 on the widest bus the controller and the board share, or to
//! HS400 enhanced strobe (the K1 with a card that has the strobe) — no tuning anywhere.
//! [`Disk`] moves sectors with ADMA2 through `DmaBuffer`, so every byte the device sees
//! crosses its typestate; [`Card::read_pio`] and [`Card::write_pio`] move them without DMA or
//! interrupts (the boot loader, TASK-0246B — it writes the BSB before it loads); a PIO transfer
//! the controller ends early is a named short transfer, not a wait for a block that never comes. The OS glue is `os` (feature `os-lite`: the window mapped from the
//! device capability, the line bound, kernel one-shots for every wait, DMA memory made for the
//! device) — the crate's only `unsafe`, the volatile register access. The `BlockDevice`
//! adapters live with their consumers (`storage::sdhci` for blkd, nxboot in 0246B); this crate
//! depends on no service. The behavioural machine the core is proven against is its own crate,
//! `storage-sdhci-model`.
//! OWNERS: @runtime @drivers
//! STATUS: Functional (host-proven; the OS glue since TASK-0246 P4b, the QEMU lane P5, the board P6)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests (divider, descriptors, protocol decoding, the board's measured
//!   EXT_CSD); `tests/sdhci.rs` (init to HS52 and HS400ES, the K1 register sequence, ADMA and
//!   PIO transfers under a non-coherent cache, the `test_reject_*` matrix, determinism)
//! INVARIANTS: every wait is bounded and named ([`Stage`]); the only polls are controller
//!   states that raise no interrupt (reset, internal clock, command/data inhibit, the DLL
//!   lock); card data (responses, CID/CSD, EXT_CSD) is untrusted and range-checked; no
//!   descriptor names memory the device cannot reach (below 4 GiB, 4-byte aligned).

#![cfg_attr(not(test), no_std)]
// No `unsafe` anywhere: the OS glue maps its registers through the ABI's window (TASK-0251 P2).
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod adma;
mod card;
pub mod clock;
pub mod cmd;
mod disk;
mod engine;
mod error;
mod host;
pub mod k1;
pub mod os;
pub mod proto;
pub mod regs;

/// Every bound a wait has, named in one place (each wait's [`Stage`] says which ran out).
pub mod timeouts {
    pub use crate::card::{OP_COND_TIMEOUT_US, POWER_UP_US, RECOVERY_TIMEOUT_US};
    pub use crate::engine::{
        COMMAND_TIMEOUT_US, INHIBIT_TIMEOUT_US, PER_BLOCK_US, READ_TIMEOUT_US, WRITE_TIMEOUT_US,
    };
    pub use crate::host::{CLOCK_STABLE_TIMEOUT_US, RESET_TIMEOUT_US, SIGNAL_SETTLE_US};
    pub use crate::k1::DLL_LOCK_TIMEOUT_US;
}

pub use card::{Card, Ceiling, InitFailure, Mode};
pub use disk::Disk;
pub use error::{Error, Stage};
pub use host::{Host, HostConfig, Layer, Timing};

/// What the core needs from where it runs: time, and a way to wait for the controller.
pub trait Platform {
    /// Monotonic microseconds.
    fn now_us(&self) -> u64;
    /// Let `us` microseconds pass (between the reads of a bounded poll, after power-up).
    fn delay_us(&mut self, us: u64);
    /// Wait until the controller's interrupt fires or `deadline_us` passes; return at once
    /// when one is already pending. A platform without interrupts (a boot loader) lets a
    /// short time pass and returns — the core re-reads the status either way. A platform
    /// that holds an interrupt claim completes it before it blocks again (by then the core
    /// has cleared the status that raised it).
    fn wait_irq(&mut self, deadline_us: u64);
    /// True when [`Platform::wait_irq`] sleeps on the controller's interrupt line: the core
    /// then signals its status bits on the line; false for a polling platform.
    fn irq(&self) -> bool;
}
