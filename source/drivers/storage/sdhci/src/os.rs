// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(all(nexus_env = "os", feature = "os-lite"))]

//! CONTEXT: The SDHCI core on the OS (TASK-0246 P4b). [`open`] turns the device capability
//! the owner was granted into a [`Disk`]: the controller's window mapped from the capability,
//! its interrupt line bound to the owner's notify endpoint, every wait a kernel one-shot on the
//! owner's device-watchdog pair beside that endpoint in one waitset (never a receive
//! deadline, never a clock compare), and the DMA memory made FOR the device — inside its
//! reach, in its bus addresses — and maintained with Zicbom when the device does not snoop.
//! The only `unsafe` in the crate is the volatile register access here.
//! OWNERS: @runtime @drivers
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the core under it is host-proven (`tests/sdhci`); this glue by the QEMU
//!   lane `ci-os-sdhci` (TASK-0246 P5) and the board (P6)

use nexus_abi::DmaVmo;
use nexus_driverkit::{DmaBuffer, Zicbom};
use nexus_hal::Bus;
use nexus_ipc::timer::{NotifyTimer, Waitset};
use nexus_service_topology::SlotPair;

use crate::{Card, Disk, Error, Host, HostConfig, Platform};

/// The ADMA descriptor table: one page, one run.
const TABLE_BYTES: usize = 4096;
/// The bounce buffer a transfer crosses: 128 sectors per chunk.
const BOUNCE_BYTES: usize = 64 * 1024;
/// Between two reads of a state that raises no interrupt, when no line is bound.
const POLL_US: u64 = crate::host::POLL_US;
/// Interrupt notifications drained per wake (the line stays claimed until completed, so one
/// is the norm; the bound keeps a misbehaving source from holding the owner).
const DRAIN_MAX: usize = 8;

/// A [`Disk`] on the OS.
pub type OsDisk = Disk<MmioBus, OsPlatform, DmaVmo, Zicbom>;

/// Why a granted device did not become a disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenError {
    /// The capability is not a device window, or its window could not be mapped.
    Window,
    /// No one-shot could be bound on the watchdog pair, or no waitset made.
    Timer,
    /// The DMA memory could not be made for the device (out of its reach, or the device
    /// does not snoop and the harts cannot maintain the cache).
    DmaMemory,
    /// The controller or the card refused.
    Core(Error),
}

/// The controller's registers, mapped from its device capability.
pub struct MmioBus {
    base: usize,
    len: usize,
}

// The crate's one `unsafe`: volatile access to the mapped registers.
#[allow(unsafe_code)]
impl Bus for MmioBus {
    fn read(&self, addr: usize) -> u32 {
        if addr + 4 > self.len {
            return u32::MAX;
        }
        // SAFETY: `base..base + len` is the window `mmio_map_auto` mapped for the device
        // capability (USER|RW, never unmapped while the disk lives); `addr` is 4-byte aligned
        // (the core's register offsets) and the word lies inside the window (checked above).
        unsafe { core::ptr::read_volatile((self.base + addr) as *const u32) }
    }

    fn write(&self, addr: usize, value: u32) {
        if addr + 4 > self.len {
            return;
        }
        // SAFETY: as in `read`.
        unsafe { core::ptr::write_volatile((self.base + addr) as *mut u32, value) }
    }
}

/// Time and the interrupt: a kernel one-shot on the owner's watchdog pair and the owner's
/// notify endpoint, one waitset over both.
pub struct OsPlatform {
    irq_num: u32,
    irq_ep: u32,
    /// The waitset member index of the notify endpoint; `None` when no line is bound.
    irq_member: Option<u32>,
    timer: NotifyTimer,
    waitset: Waitset,
    /// An interrupt was taken and its PLIC claim not yet completed.
    claimed: bool,
}

impl OsPlatform {
    /// The core has cleared the status that raised the last interrupt: complete the claim so
    /// the line can raise the next one.
    fn complete_claim(&mut self) {
        if core::mem::take(&mut self.claimed) {
            let _ = nexus_abi::irq_complete(self.irq_num);
        }
    }

    /// Consume the notifications a wake left on the endpoint (the claim stays until
    /// [`Self::complete_claim`]).
    fn drain_irq(&mut self) {
        let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 16];
        for _ in 0..DRAIN_MAX {
            if nexus_abi::ipc_recv_v1_nb(self.irq_ep, &mut hdr, &mut buf, true).is_err() {
                break;
            }
        }
        self.claimed = true;
    }
}

impl Platform for OsPlatform {
    fn now_us(&self) -> u64 {
        nexus_abi::nsec().unwrap_or(0) / 1000
    }

    fn delay_us(&mut self, us: u64) {
        self.complete_claim();
        // A frame left by an earlier one-shot would end this delay early: drop it first. Only
        // the timer is waited on — an interrupt raised meanwhile stays queued on its endpoint
        // for the next `wait_irq`.
        let _ = self.timer.drain();
        self.timer.arm_in(us.saturating_mul(1000));
        let _ = self.timer.wait_fired();
    }

    fn wait_irq(&mut self, deadline_us: u64) {
        self.complete_claim();
        let Some(irq_member) = self.irq_member else {
            self.delay_us(POLL_US);
            return;
        };
        if deadline_us <= self.now_us() {
            return;
        }
        let _ = self.timer.drain();
        self.timer.arm_at(deadline_us.saturating_mul(1000));
        if self.waitset.wait() == Ok(irq_member) {
            self.drain_irq();
        }
        // The one-shot may have fired between the wake and the disarm: its frame goes too, so
        // no later wait or delay ends on it.
        self.timer.disarm();
        let _ = self.timer.drain();
    }

    fn irq(&self) -> bool {
        self.irq_member.is_some()
    }
}

/// What bringing the disk up found, for the owner's marker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Facts {
    /// Why HS400 enhanced strobe fell back to HS52, when it did.
    pub fallback: Option<Error>,
    /// The controller's line is bound to the owner's endpoint (false: the core polls).
    pub irq_bound: bool,
}

/// The SD host behind `device` as a disk: its window mapped, its line bound to `irq_ep` (the
/// owner's notify endpoint; 0 = poll), every wait bounded by a one-shot on `watchdog`, the
/// card taken to the best mode `config` allows.
pub fn open(
    device: u32,
    irq_ep: u32,
    watchdog: SlotPair,
    config: HostConfig,
) -> Result<(OsDisk, Facts), OpenError> {
    let mut info = nexus_abi::CapQuery::default();
    nexus_abi::cap_query(device, &mut info).map_err(|_| OpenError::Window)?;
    if info.kind_tag != 2 {
        return Err(OpenError::Window);
    }
    let len = usize::try_from(info.len).map_err(|_| OpenError::Window)?;
    let base = nexus_abi::mmio_map_auto(device, 0, len).map_err(|_| OpenError::Window)?;

    let timer = NotifyTimer::bind(watchdog).map_err(|_| OpenError::Timer)?;
    let mut waitset = Waitset::new().map_err(|_| OpenError::Timer)?;
    let bound = info.irq != 0 && irq_ep != 0 && nexus_abi::irq_bind(info.irq, irq_ep).is_ok();
    let irq_member = if bound { waitset.add(irq_ep).ok() } else { None };
    waitset.add(timer.recv_slot()).map_err(|_| OpenError::Timer)?;
    let platform =
        OsPlatform { irq_num: info.irq, irq_ep, irq_member, timer, waitset, claimed: false };

    let host = Host::new(MmioBus { base, len }, platform, config).map_err(OpenError::Core)?;
    let (card, fallback) = Card::init_best(host).map_err(|f| OpenError::Core(f.error))?;

    let coherence = nexus_abi::device_dma_coherence(device).map_err(|_| OpenError::DmaMemory)?;
    let table = DmaVmo::contiguous(device, TABLE_BYTES).map_err(|_| OpenError::DmaMemory)?;
    let bounce = DmaVmo::anonymous(device, BOUNCE_BYTES).map_err(|_| OpenError::DmaMemory)?;
    let table = DmaBuffer::new(table, coherence, Zicbom).map_err(|_| OpenError::DmaMemory)?;
    let bounce = DmaBuffer::new(bounce, coherence, Zicbom).map_err(|_| OpenError::DmaMemory)?;
    let disk = Disk::new(card, table, bounce).map_err(OpenError::Core)?;
    Ok((disk, Facts { fallback, irq_bound: bound }))
}
