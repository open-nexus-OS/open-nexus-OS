// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the SoC glue against the board as measured. The register file is
//! seeded from `docs/board/measurements/2026-09-22-stock-system/regmap-apmu.txt`
//! (the APMU window as the stock system runs it) and the tree is the golden
//! `bpi-f3.dtb`; every expectation below is a number read on the desk board.

use std::cell::RefCell;
use std::collections::HashMap;

use nexus_fdt::Fdt;
use nexus_hal::Bus;
use nexus_soc::table::k1;
use nexus_soc::{
    bring_up, clock_rate, plan, BringUp, BringUpError, Executor, Fault, Pause, PlanError,
    ProviderKind, Providers, RateError,
};

const BOARD: &[u8] = include_bytes!("../../nexus-fdt/tests/goldens/bpi-f3.dtb");
const VIRT: &[u8] = include_bytes!("../../nexus-fdt/tests/goldens/virt.dtb");
/// What the planner refuses (TASK-0328 U3): hub lines and glue words off the tables.
const SOC_REJECTS: &[u8] = include_bytes!("../../nexus-fdt/tests/goldens/soc-rejects.dtb");

/// The display set and the encoder's pads (TASK-0245B P3), on the same fixtures.
#[path = "k1/display.rs"]
mod display;
/// The USB host and its hub (TASK-0328 U3), on this file's fixtures (one test target; a
/// `tests/*.rs` of its own would be a second copy of them).
#[path = "k1/usb.rs"]
mod usb;
const APMU_STOCK: &str =
    include_str!("../../../../docs/board/measurements/2026-09-22-stock-system/regmap-apmu.txt");
const APMU_BASE: usize = 0xd428_2800;

/// A register file: absolute address → word, plus a write log.
struct MockBus {
    regs: RefCell<HashMap<usize, u32>>,
    writes: RefCell<Vec<(usize, u32)>>,
    /// An FC bit that never clears, if set.
    stuck_fc: Option<(usize, u32)>,
    /// Bits the hardware clears by itself once written (the FC bits): the mock
    /// clears them on write, as the SoC does after the switch.
    self_clearing: Vec<(usize, u32)>,
    /// A power sequencer: once the control word at `.0` holds all of `.1`, the status word
    /// at `.2` reports `.3` — the domain came up.
    sequencer: Option<(usize, u32, usize, u32)>,
    /// GPIO bank register bases (TASK-0328 U3): a write to a bank's set-direction, set or
    /// clear register lands in its direction or level word, as the block does.
    gpio_banks: Vec<usize>,
    /// Reads of the level word before a set/clear shows in it (board cycle 5: the word
    /// follows the pin, a read right after the write saw the old level); 0 = at once.
    gpio_lag: std::cell::Cell<usize>,
    /// A level change waiting out its lag: the word, the bits, set or clear, reads left.
    gpio_pending: RefCell<Vec<(usize, u32, bool, usize)>>,
}

/// The settles a bring-up asked for, in order.
#[derive(Default)]
struct Pauses(RefCell<Vec<u32>>);

impl Pause for Pauses {
    fn pause_ms(&self, ms: u32) {
        self.0.borrow_mut().push(ms);
    }
}

/// The GPIO block's window and bank 3 inside it (lines 96..=127).
const GPIO_BASE: usize = 0xd401_9000;
const GPIO_BANK3: usize = GPIO_BASE + 0x100;
const USB: &str = "/soc/storage-bus/usb@c0a00000";
const HUB: &str = "/usb-hub";

impl MockBus {
    fn cold() -> Self {
        MockBus {
            regs: RefCell::new(HashMap::new()),
            writes: RefCell::new(Vec::new()),
            stuck_fc: None,
            self_clearing: Vec::new(),
            sequencer: None,
            gpio_banks: Vec::new(),
            gpio_lag: std::cell::Cell::new(0),
            gpio_pending: RefCell::new(Vec::new()),
        }
    }

    /// The level word shows a set/clear only after `reads` reads of it.
    fn with_gpio_lag(self, reads: usize) -> Self {
        self.gpio_lag.set(reads);
        self
    }

    /// Lagged level changes: one read passed; those due land in their word.
    fn settle_gpio(&self, addr: usize) {
        let mut pending = self.gpio_pending.borrow_mut();
        let mut i = 0;
        while i < pending.len() {
            let (word, bits, set, left) = pending[i];
            if word != addr {
                i += 1;
                continue;
            }
            if left == 0 {
                let cur = self.regs.borrow().get(&word).copied().unwrap_or(0);
                self.regs.borrow_mut().insert(word, if set { cur | bits } else { cur & !bits });
                pending.remove(i);
            } else {
                pending[i].3 = left - 1;
                i += 1;
            }
        }
    }

    /// With bank 3 of the GPIO block behaving like one.
    fn with_gpio_bank3(mut self) -> Self {
        self.gpio_banks.push(GPIO_BANK3);
        self
    }

    fn seed(&self, addr: usize, word: u32) {
        self.regs.borrow_mut().insert(addr, word);
    }

    /// The hub's lines and pads as the stock system runs them (2026-10-04).
    fn with_stock_hub(self, providers: &Providers) -> Self {
        let bus = self.with_gpio_bank3();
        bus.seed(GPIO_BANK3, 0x3818_4002);
        bus.seed(GPIO_BANK3 + 0x0c, 0x981a_c003);
        let pinctrl = providers.get(ProviderKind::Pinctrl).unwrap().base;
        for (off, live) in [(0x1e4usize, 0xa041u32), (0x23c, 0xa040), (0x240, 0xd040)] {
            bus.seed(pinctrl + off, live);
        }
        bus
    }

    /// Seeded with the measured APMU window.
    fn stock() -> Self {
        let bus = MockBus::cold();
        for line in APMU_STOCK.lines() {
            let Some((off, val)) = line.split_once(": ") else { continue };
            let off = usize::from_str_radix(off.trim(), 16).expect("offset");
            let val = u32::from_str_radix(val.trim(), 16).expect("value");
            bus.regs.borrow_mut().insert(APMU_BASE + off, val);
        }
        bus
    }

    fn writes(&self) -> Vec<(usize, u32)> {
        self.writes.borrow().clone()
    }
}

impl Bus for MockBus {
    fn read(&self, addr: usize) -> u32 {
        self.settle_gpio(addr);
        let v = self.regs.borrow().get(&addr).copied().unwrap_or(0);
        match self.stuck_fc {
            Some((a, bit)) if a == addr => v | bit,
            _ => v,
        }
    }
    fn write(&self, addr: usize, value: u32) {
        self.writes.borrow_mut().push((addr, value));
        for &bank in &self.gpio_banks {
            let (level, direction) = (bank, bank + 0x0c);
            let target = match addr - bank {
                0x54 => Some((direction, true)),
                0x18 => Some((level, true)),
                0x24 => Some((level, false)),
                _ => None,
            };
            if let Some((word, set)) = target {
                let lag = self.gpio_lag.get();
                if word == level && lag > 0 {
                    self.gpio_pending.borrow_mut().push((word, value, set, lag));
                    return;
                }
                let cur = self.regs.borrow().get(&word).copied().unwrap_or(0);
                self.regs.borrow_mut().insert(word, if set { cur | value } else { cur & !value });
                return;
            }
        }
        let mut stored = value;
        for &(a, bits) in &self.self_clearing {
            if a == addr {
                stored &= !bits;
            }
        }
        self.regs.borrow_mut().insert(addr, stored);
        if let Some((ctrl, bits, status, on)) = self.sequencer {
            if addr == ctrl && stored & bits == bits {
                let now = self.read(status);
                self.regs.borrow_mut().insert(status, now | on);
            }
        }
    }
}

fn board_providers() -> (Fdt<'static>, Providers) {
    let fdt = Fdt::new(BOARD).unwrap();
    let providers =
        Providers::from_tree(&fdt, |n| n.reg(0).ok().flatten().map(|r| r.addr as usize));
    (fdt, providers)
}

#[test]
fn the_board_tree_names_every_provider_and_virt_names_none() {
    let (_, providers) = board_providers();
    assert_eq!(providers.count(), 7, "five syscon/pll windows, the pads, the GPIO block");
    assert_eq!(providers.get(ProviderKind::Gpio).unwrap().base, GPIO_BASE);
    assert_eq!(providers.get(ProviderKind::Apmu).unwrap().base, APMU_BASE);
    let virt = Fdt::new(VIRT).unwrap();
    let none = Providers::from_tree(&virt, |n| n.reg(0).ok().flatten().map(|r| r.addr as usize));
    assert_eq!(none.count(), 0, "QEMU virt has no SoC glue");
    let blk = virt.find_compatible(&["virtio,mmio"]).next().unwrap();
    assert!(plan(blk, &none).unwrap().is_empty(), "NotNeeded");
}

#[test]
fn emmc_bring_up_from_the_stock_state_writes_nothing() {
    let (fdt, providers) = board_providers();
    let emmc = fdt.node_at_path("/soc/storage-bus/mmc@d4281000").unwrap();
    let plan = plan(emmc, &providers).unwrap();
    assert_eq!(plan.len(), 5, "domain + 2 resets + 2 clocks");
    let bus = MockBus::stock();
    let report = Executor::new(&bus).execute(&plan).unwrap();
    assert_eq!((report.resets_released, report.clocks_on, report.domains), (2, 2, 1));
    assert_eq!(report.writes, 0, "the SPL left the eMMC glue on: {:?}", bus.writes());
}

#[test]
fn emmc_bring_up_from_cold_writes_exactly_the_documented_bits() {
    let (fdt, providers) = board_providers();
    let emmc = fdt.node_at_path("/soc/storage-bus/mmc@d4281000").unwrap();
    let plan = plan(emmc, &providers).unwrap();
    let bus = MockBus::cold();
    let report = Executor::new(&bus).execute(&plan).unwrap();
    assert_eq!(report.writes, 4);
    let sdh0 = APMU_BASE + 0x054;
    let sdh2 = APMU_BASE + 0x0e0;
    assert_eq!(
        bus.writes(),
        vec![
            (sdh0, 1 << 0),
            (sdh2, 1 << 1),
            (sdh0, (1 << 0) | (1 << 3)),
            (sdh2, (1 << 1) | (1 << 4))
        ],
        "sdh_axi released, sdh2 released, sdh_axi gated on, sdh2 gated on — in that order"
    );
}

#[test]
fn usb_and_ethernet_are_up_on_the_stock_board_too() {
    let (fdt, providers) = board_providers();
    let bus = MockBus::stock();
    for path in [
        "/soc/storage-bus/usb@c0a00000",
        "/soc/storage-bus/usb@c0980100",
        "/soc/storage-bus/usb@c0900100",
        "/soc/network-bus/ethernet@cac80000",
    ] {
        let node = fdt.node_at_path(path).unwrap();
        let plan = plan(node, &providers).unwrap();
        Executor::new(&bus).execute(&plan).unwrap();
    }
    // The APMU glue is on as the stock system left it; the register file holds no pad words
    // (the capture is the APMU's), so the only writes are pads.
    let pinctrl = providers.get(ProviderKind::Pinctrl).unwrap().base;
    let not_a_pad: Vec<_> = bus
        .writes()
        .into_iter()
        .filter(|(a, _)| !(pinctrl..pinctrl + 0x1000).contains(a))
        .collect();
    assert!(not_a_pad.is_empty(), "the USB and ethernet glue is already on: {not_a_pad:x?}");
}

// ---- The node operations (TASK-0246B P2): what `socd` and the loader both run ----

/// A bus that takes no write (nothing answers at the windows).
struct DeadBus;

impl Bus for DeadBus {
    fn read(&self, _addr: usize) -> u32 {
        0
    }
    fn write(&self, _addr: usize, _value: u32) {}
}

#[test]
fn a_node_is_brought_up_when_the_tree_binds_glue_and_not_needed_when_it_binds_none() {
    let virt = Fdt::new(VIRT).unwrap();
    let none = Providers::from_tree(&virt, |n| n.reg(0).ok().flatten().map(|r| r.addr as usize));
    let blk = virt.find_compatible(&["virtio,mmio"]).next().unwrap();
    assert_eq!(bring_up(blk, &none, &MockBus::cold()), Ok(BringUp::NotNeeded));
    let (fdt, providers) = board_providers();
    let emmc = fdt.node_at_path("/soc/storage-bus/mmc@d4281000").unwrap();
    let bus = MockBus::stock();
    let Ok(BringUp::Up(report)) = bring_up(emmc, &providers, &bus) else {
        panic!("the eMMC is up")
    };
    let counts = (report.domains, report.resets_released, report.clocks_on, report.writes);
    assert_eq!(counts, (1, 2, 2, 0), "the stock state needs no write");
    let cold = MockBus::cold();
    let Ok(BringUp::Up(report)) = bring_up(emmc, &providers, &cold) else { panic!("up from cold") };
    assert_eq!(report.writes, 4, "the documented bits: {:?}", cold.writes());
}

#[test]
fn test_reject_a_bring_up_the_tables_do_not_cover_or_the_bus_does_not_take() {
    let (fdt, providers) = board_providers();
    let bus = MockBus::stock();
    let gpu = fdt.node_at_path("/soc/multimedia-bus/gpu@cac00000").unwrap();
    let refused = Err(BringUpError::Plan(PlanError::DomainUnsupported(k1::PD_GPU)));
    assert_eq!(bring_up(gpu, &providers, &bus), refused);
    assert!(bus.writes().is_empty(), "refused before any bus access");
    let emmc = fdt.node_at_path("/soc/storage-bus/mmc@d4281000").unwrap();
    let fault = Fault::ReadBack { addr: APMU_BASE + 0x054, value: 0 };
    assert_eq!(bring_up(emmc, &providers, &DeadBus), Err(BringUpError::Fault(fault)));
}

#[test]
fn a_named_clock_rate_is_what_the_registers_select() {
    let (fdt, providers) = board_providers();
    let bus = MockBus::stock();
    let emmc = fdt.node_at_path("/soc/storage-bus/mmc@d4281000").unwrap();
    let sd = fdt.node_at_path("/soc/storage-bus/mmc@d4280000").unwrap();
    // The live tree's `spacemit,sdh-freq` for the same hosts (measured 2026-09-26).
    assert_eq!(clock_rate(emmc, "io", &providers, &bus), Ok(375_000_000), "sdh2: pll2_d8");
    assert_eq!(clock_rate(sd, "io", &providers, &bus), Ok(204_800_000), "sdh0: pll1_d6 / 2");
    assert_eq!(clock_rate(emmc, "core", &providers, &bus), Ok(307_200_000), "sdh_axi: fixed");
    // From a cold register file the rate is what the zeroed fields select: mux 0, divider 1.
    let cold = MockBus::cold();
    assert_eq!(clock_rate(emmc, "io", &providers, &cold), Ok(409_600_000), "pll1_d6");
}

#[test]
fn test_reject_a_clock_rate_without_its_name_or_its_window() {
    let (fdt, providers) = board_providers();
    let bus = MockBus::stock();
    let emmc = fdt.node_at_path("/soc/storage-bus/mmc@d4281000").unwrap();
    assert_eq!(clock_rate(emmc, "strobe", &providers, &bus), Err(RateError::NoSuchClock));
    let no_apmu = Providers::new();
    let refused = Err(RateError::ProviderUnknown(ProviderKind::Apmu));
    assert_eq!(clock_rate(emmc, "io", &no_apmu, &bus), refused);
    let virt = Fdt::new(VIRT).unwrap();
    let blk = virt.find_compatible(&["virtio,mmio"]).next().unwrap();
    assert_eq!(clock_rate(blk, "io", &no_apmu, &bus), Err(RateError::NoSuchClock));
}
