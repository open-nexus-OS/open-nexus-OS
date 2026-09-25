// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The machine the SDHCI core runs against on the host (TASK-0246): a controller
//! (`ctrl`), an eMMC (`card`), DMA memory behind a non-coherent cache (`mem`), simulated time,
//! a log of what the driver did, and faults a test can inject. Time moves only when the
//! driver lets it (`delay_us`, `wait_irq`); a wait with nothing pending jumps to its deadline,
//! so a fault that never completes costs no real time and every run is identical. One machine
//! for every consumer of the core — the core's own tests (P2), the block owner's adapter
//! (P4b), the boot loader's reader (TASK-0246B) — so each is proven against the same
//! behaviour. Host only: no OS crate depends on it.
//! OWNERS: @runtime @drivers
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the core's `tests/sdhci` (init goldens, ADMA2 under the cache protocol, the
//!   reject matrix) prove the machine against the driver and each other

#![forbid(unsafe_code)]
// A test machine, host only: a fixture that cannot be built is a failed test, reported where it
// broke — its inputs are the tests' own constants, never untrusted data.
#![allow(clippy::unwrap_used, clippy::expect_used)]

pub mod card;
pub mod ctrl;
pub mod mem;

use std::cell::RefCell;
use std::rc::Rc;

use nexus_abi::DmaCoherence;
use nexus_driverkit::DmaBuffer;
use nexus_hal::Bus;
use storage_sdhci::{Card, Ceiling, Disk, Host, HostConfig, Layer, Platform};

use mem::{ModelCache, ModelMem};

/// What the driver did, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// A command was issued: index, argument.
    Cmd(u8, u32),
    /// A K1 vendor register was written: offset, value.
    Vendor(usize, u32),
}

/// Faults a test injects. The per-command ones persist; the data ones fire once.
#[derive(Default)]
pub struct Faults {
    /// The card never answers this command.
    pub silent: Option<u8>,
    /// The response to this command fails its CRC.
    pub crc: Option<u8>,
    /// The controller never raises anything for this command.
    pub never_complete: Option<u8>,
    /// These R1 bits ride on the response to this command.
    pub status_bits: Option<(u8, u32)>,
    /// The response to this command reports this state.
    pub stuck_state: Option<(u8, u8)>,
    /// The next data phase times out.
    pub data_timeout: bool,
    /// The next ADMA walk finds no valid descriptor.
    pub adma_error: bool,
    /// The next transfer ends with this many blocks left.
    pub short_by: u16,
    /// Reads on a bus wider than 1 bit arrive corrupted (no CRC error).
    pub corrupt_wide: bool,
    pub clock_never_stable: bool,
    pub reset_never_clears: bool,
    pub dll_never_locks: bool,
}

pub struct Machine {
    pub ctrl: ctrl::Ctrl,
    pub card: card::Emmc,
    pub mem: mem::Dram,
    pub now: u64,
    pub log: Vec<Event>,
    pub faults: Faults,
}

pub type Shared = Rc<RefCell<Machine>>;

/// A controller and card pair.
pub struct Config {
    pub version: u8,
    pub caps: u32,
    pub base_hz: u32,
    pub k1: bool,
    pub ext_csd: [u8; 512],
}

/// QEMU's `sdhci-pci` defaults (`capareg` 0x057834b4: spec 2.00, 52 MHz, ADMA2, 3.3 V and
/// 1.8 V, no 8-bit bus) with its `emmc` card's EXT_CSD (revision 5, HS26 + HS52, no switch
/// time) at 4 GiB.
pub fn qemu() -> Config {
    let mut ext = [0u8; 512];
    ext[196] = 0b11;
    ext[194] = 2;
    ext[192] = 5;
    ext[212..216].copy_from_slice(&0x0080_0000u32.to_le_bytes());
    Config { version: 1, caps: 0x0578_34B4, base_hz: 52_000_000, k1: false, ext_csd: ext }
}

/// The lane's host: `sdhci-pci` declared spec 3.00 with an 8-bit bus.
pub fn qemu_8bit() -> Config {
    Config { version: 2, caps: 0x0578_34B4 | (1 << 18), ..qemu() }
}

/// The board: the K1 eMMC host (spec 3.00, no usable base clock in its capabilities, the
/// `io` clock at 375 MHz) with the board's card as the stock kernel read it.
pub fn k1() -> Config {
    Config {
        version: 2,
        caps: 0x0578_00B4 | (1 << 18),
        base_hz: 375_000_000,
        k1: true,
        ext_csd: measured(),
    }
}

pub fn measured() -> [u8; 512] {
    let hex =
        include_str!("../../../../../../docs/board/measurements/2026-09-24-emmc-sdhci/ext_csd.hex");
    let digits: Vec<u8> = hex.bytes().filter(u8::is_ascii_hexdigit).collect();
    let mut raw = [0u8; 512];
    for (i, pair) in digits.chunks_exact(2).take(512).enumerate() {
        raw[i] = u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap();
    }
    raw
}

pub fn machine(config: Config) -> Shared {
    Rc::new(RefCell::new(Machine {
        ctrl: ctrl::Ctrl::new(config.version, config.caps, config.base_hz, config.k1),
        card: card::Emmc::new(config.ext_csd),
        mem: mem::Dram::default(),
        now: 0,
        log: Vec::new(),
        faults: Faults::default(),
    }))
}

pub struct ModelBus(pub Shared);

impl Bus for ModelBus {
    fn read(&self, addr: usize) -> u32 {
        assert_eq!(addr % 4, 0, "aligned words only");
        self.0.borrow_mut().read(addr)
    }
    fn write(&self, addr: usize, value: u32) {
        assert_eq!(addr % 4, 0, "aligned words only");
        self.0.borrow_mut().write(addr, value);
    }
}

pub struct SimPlatform {
    pub m: Shared,
    pub irq: bool,
}

impl Platform for SimPlatform {
    fn now_us(&self) -> u64 {
        self.m.borrow().now
    }
    fn delay_us(&mut self, us: u64) {
        self.m.borrow_mut().now += us;
    }
    fn wait_irq(&mut self, deadline_us: u64) {
        let mut m = self.m.borrow_mut();
        if m.ctrl.pending() {
            // A pending interrupt returns at once, but time still passes: a driver waiting for a
            // bit that never comes spins to its deadline, as on hardware, instead of forever.
            m.now += 1;
        } else {
            m.now = m.now.max(deadline_us);
        }
    }
    fn irq(&self) -> bool {
        self.irq
    }
}

/// The host config the board's tree gives each machine.
pub fn host_config(m: &Shared) -> HostConfig {
    let k1 = m.borrow().ctrl.k1;
    HostConfig {
        base_clock_hz: if k1 { Some(375_000_000) } else { None },
        bus_width: 8,
        hs400es: k1,
        layer: if k1 { Layer::K1 } else { Layer::Standard },
    }
}

pub type ModelHost = Host<ModelBus, SimPlatform>;

pub fn host(m: &Shared, irq: bool) -> ModelHost {
    let platform = SimPlatform { m: m.clone(), irq };
    Host::new(ModelBus(m.clone()), platform, host_config(m)).expect("host")
}

/// The commands the driver issued, in order.
pub fn commands(m: &Shared) -> Vec<(u8, u32)> {
    m.borrow()
        .log
        .iter()
        .filter_map(|e| if let Event::Cmd(i, a) = *e { Some((i, a)) } else { None })
        .collect()
}

pub type ModelCard = Card<ModelBus, SimPlatform>;
pub type ModelDisk = Disk<ModelBus, SimPlatform, ModelMem, ModelCache>;

/// The harts' cache block: every buffer is maintained (the K1's `soc` bus is non-coherent).
pub const NONCOHERENT: DmaCoherence = DmaCoherence::Maintained { block: 64 };

/// A card initialised up to `ceiling` on an interrupt-driven host.
pub fn card(m: &Shared, ceiling: Ceiling) -> ModelCard {
    Card::init(host(m, true), ceiling).map_err(|f| f.error).expect("init")
}

/// A DMA buffer over model memory, maintained like the K1's.
pub fn buffer(m: &Shared, mem: ModelMem) -> DmaBuffer<ModelMem, ModelCache> {
    DmaBuffer::new(mem, NONCOHERENT, ModelCache(m.clone())).expect("buffer")
}

/// A disk with a one-page table at 1 GiB and a 64 KiB bounce buffer in four scattered runs.
pub fn disk(m: &Shared, card: ModelCard) -> ModelDisk {
    let table = buffer(m, ModelMem::new(m, 4096, 0x4000_0000, 1, 0));
    let bounce = buffer(m, ModelMem::new(m, 64 * 1024, 0x4800_0000, 4, 1));
    Disk::new(card, table, bounce).expect("disk")
}
