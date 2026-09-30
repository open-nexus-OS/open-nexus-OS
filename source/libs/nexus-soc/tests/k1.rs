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
    bring_up, clock_rate, plan, BringUp, BringUpError, Executor, Fault, PlanError, ProviderKind,
    Providers, RateError, Step,
};

const BOARD: &[u8] = include_bytes!("../../nexus-fdt/tests/goldens/bpi-f3.dtb");
const VIRT: &[u8] = include_bytes!("../../nexus-fdt/tests/goldens/virt.dtb");
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
}

impl MockBus {
    fn cold() -> Self {
        MockBus {
            regs: RefCell::new(HashMap::new()),
            writes: RefCell::new(Vec::new()),
            stuck_fc: None,
            self_clearing: Vec::new(),
            sequencer: None,
        }
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
        let v = self.regs.borrow().get(&addr).copied().unwrap_or(0);
        match self.stuck_fc {
            Some((a, bit)) if a == addr => v | bit,
            _ => v,
        }
    }
    fn write(&self, addr: usize, value: u32) {
        self.writes.borrow_mut().push((addr, value));
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
    assert_eq!(providers.count(), 6);
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
        let report = Executor::new(&bus).execute(&plan).unwrap();
        assert_eq!(report.writes, 0, "{path} glue already on");
    }
}

// ---- The display set (TASK-0245B P3): domain 7, hdmi_reset, hmclk at its demanded rate ----

const DPU: &str = "/soc/multimedia-bus/display@c0440000";
const HDMI: &str = "/soc/hdmi@c0400500";
const HDMI_PWR_CTRL: usize = APMU_BASE + 0x3f4;
const PWR_STATUS: usize = APMU_BASE + 0x0f0;
const HDMI_CLK_RES: usize = APMU_BASE + 0x1b8;

/// A cold register file whose power sequencer brings domain 7 up once its control word
/// holds the mode bit and the request, and whose hmclk frequency-change bit clears itself.
fn cold_with_sequencer() -> MockBus {
    let mut bus = MockBus::cold();
    bus.sequencer = Some((HDMI_PWR_CTRL, (1 << 4) | (1 << 0), PWR_STATUS, 1 << 15));
    bus.self_clearing.push((HDMI_CLK_RES, 1 << 29));
    bus
}

#[test]
fn the_display_pipeline_names_what_the_stock_tree_names() {
    let (fdt, providers) = board_providers();
    for path in [DPU, HDMI] {
        let p = plan(fdt.node_at_path(path).unwrap(), &providers).unwrap();
        let steps: Vec<Step> = p.steps().copied().collect();
        let hmclk = nexus_soc::table::clock(ProviderKind::Apmu, k1::CLK_HDMI).unwrap();
        assert_eq!(
            steps,
            vec![
                Step::DomainOn {
                    id: k1::PD_HDMI,
                    ctrl: HDMI_PWR_CTRL,
                    mode: 1 << 4,
                    request: 1 << 0,
                    status: PWR_STATUS,
                    on: 1 << 15
                },
                Step::ReleaseReset { addr: HDMI_CLK_RES, mask: 1 << 9, assert_sets: false },
                Step::GateOn { addr: HDMI_CLK_RES, mask: 1 << 0 },
                // 491.52 MHz = pll1_d5 (mux 1) / 1 (divider 0).
                Step::SetRate { clock: hmclk, window: APMU_BASE, mux: 1, div: 0 },
            ],
            "{path}: domain 7, hdmi_reset, hmclk — none of the five DSI clocks"
        );
        let regs: Vec<usize> = p.registers().iter().collect();
        assert_eq!(regs, vec![HDMI_PWR_CTRL, PWR_STATUS, HDMI_CLK_RES], "each register once");
    }
}

#[test]
fn the_display_pipeline_on_the_stock_board_is_up_without_a_write() {
    let (fdt, providers) = board_providers();
    let bus = MockBus::stock();
    for path in [DPU, HDMI] {
        let Ok(BringUp::Up(report)) = bring_up(fdt.node_at_path(path).unwrap(), &providers, &bus)
        else {
            panic!("{path} is up on the stock board")
        };
        let counts = (report.domains, report.resets_released, report.clocks_on, report.rates_set);
        assert_eq!(counts, (1, 1, 1, 1));
        assert_eq!(report.writes, 0, "{path}: the stock desktop is on HDMI: {:?}", bus.writes());
    }
    let hmclk = nexus_soc::table::clock(ProviderKind::Apmu, k1::CLK_HDMI).unwrap();
    assert_eq!(Executor::new(&bus).rate(hmclk, APMU_BASE), 491_520_000);
}

#[test]
fn the_display_pipeline_from_cold_raises_the_domain_request_then_sets_the_rate() {
    let (fdt, providers) = board_providers();
    let bus = cold_with_sequencer();
    let dpu = fdt.node_at_path(DPU).unwrap();
    let Ok(BringUp::Up(report)) = bring_up(dpu, &providers, &bus) else { panic!("up from cold") };
    assert_eq!(
        bus.writes(),
        vec![
            (HDMI_PWR_CTRL, 1 << 4),              // the sequencer's mode, the request low
            (HDMI_PWR_CTRL, (1 << 4) | (1 << 0)), // the request raised: 0x11, the stock word
            (HDMI_CLK_RES, 1 << 9),               // hdmi_reset released
            (HDMI_CLK_RES, (1 << 9) | (1 << 0)),  // hmclk gated on
            (HDMI_CLK_RES, 0x221),                // mux 1 = pll1_d5, divider 0
            (HDMI_CLK_RES, 0x221 | (1 << 29)),    // the frequency change, cleared by the SoC
        ],
        "one write at a time, in RFC-0106 order"
    );
    assert_eq!(report.writes, 6);
    assert_eq!(bus.read(PWR_STATUS) & (1 << 15), 1 << 15, "domain 7 reports on");
    let hmclk = nexus_soc::table::clock(ProviderKind::Apmu, k1::CLK_HDMI).unwrap();
    assert_eq!(Executor::new(&bus).rate(hmclk, APMU_BASE), 491_520_000);
    // The encoder shares all three: its bring-up after the controller's writes nothing.
    let hdmi = fdt.node_at_path(HDMI).unwrap();
    let Ok(BringUp::Up(again)) = bring_up(hdmi, &providers, &bus) else { panic!("hdmi up") };
    assert_eq!(again.writes, 0);
}

#[test]
fn a_domain_left_requested_but_off_gets_a_fresh_rising_request() {
    let (fdt, providers) = board_providers();
    let bus = cold_with_sequencer();
    // The control word already reads 0x11 while the status says off (a request that never
    // landed): the executor drops the request and raises it again.
    bus.regs.borrow_mut().insert(HDMI_PWR_CTRL, 0x11);
    let p = plan(fdt.node_at_path(DPU).unwrap(), &providers).unwrap();
    Executor::new(&bus).execute(&p).unwrap();
    assert_eq!(bus.writes()[..2], [(HDMI_PWR_CTRL, 0x10), (HDMI_PWR_CTRL, 0x11)]);
}

#[test]
fn test_reject_a_domain_that_never_reports_on() {
    let (fdt, providers) = board_providers();
    let mut bus = cold_with_sequencer();
    bus.sequencer = None; // the request lands nowhere
    let p = plan(fdt.node_at_path(DPU).unwrap(), &providers).unwrap();
    let err = Executor::new(&bus).execute(&p).unwrap_err();
    assert_eq!(err, Fault::DomainStuck { addr: PWR_STATUS, value: 0 });
    assert_eq!((err.step(), err.register()), ("domain", Some((PWR_STATUS, 0))));
    assert_eq!(bus.writes().len(), 2, "no reset or clock touched in a domain that is off");
}

#[test]
fn test_reject_a_domain_without_a_measured_protocol() {
    let (fdt, providers) = board_providers();
    let gpu = fdt.node_at_path("/soc/multimedia-bus/gpu@cac00000").unwrap();
    // The GPU's domain is software-sequenced and unmeasured: refused before any bus access.
    assert_eq!(plan(gpu, &providers), Err(PlanError::DomainUnsupported(k1::PD_GPU)));
}

#[test]
fn test_reject_a_rate_no_parent_makes_exactly() {
    let hmclk = nexus_soc::table::clock(ProviderKind::Apmu, k1::CLK_HDMI).unwrap();
    assert_eq!(hmclk.select(491_520_000), Some((1, 0)), "pll1_d5 / 1");
    assert_eq!(hmclk.select(245_760_000), Some((1, 1)), "pll1_d5 / 2 — no rounding elsewhere");
    assert_eq!(hmclk.select(500_000_000), None, "met exactly or refused, never rounded");
    let uart = nexus_soc::table::clock(ProviderKind::Apbc, k1::CLK_UART0).unwrap();
    assert_eq!(uart.select(14_745_600), None, "a clock without a divider sets no rate");
}

#[test]
fn rates_read_back_from_the_stock_registers_match_the_clock_tree() {
    let bus = MockBus::stock();
    let ex = Executor::new(&bus);
    let sdh2 = nexus_soc::table::clock(ProviderKind::Apmu, k1::CLK_SDH2).unwrap();
    assert_eq!(ex.rate(sdh2, APMU_BASE), 375_000_000, "sdh2_clk from pll2_d8, div 0");
    let sdh1 = nexus_soc::table::clock(ProviderKind::Apmu, k1::CLK_SDH1).unwrap();
    assert_eq!(ex.rate(sdh1, APMU_BASE), 375_000_000);
    let aclk = nexus_soc::table::clock(ProviderKind::Apmu, k1::CLK_PMUA_ACLK).unwrap();
    assert_eq!(ex.rate(aclk, APMU_BASE), 307_200_000, "pmua_aclk: mux 1 = pll1_d8");
    let gpu = nexus_soc::table::clock(ProviderKind::Apmu, k1::CLK_GPU).unwrap();
    assert_eq!(ex.rate(gpu, APMU_BASE), 614_400_000, "gpu_clk: mux 0 = pll1_d4, div 0");
}

#[test]
fn a_mux_change_triggers_the_frequency_change_bit_and_reads_back() {
    let mut bus = MockBus::stock();
    bus.self_clearing.push((APMU_BASE + 0x0e0, 1 << 11));
    let ex = Executor::new(&bus);
    let sdh2 = nexus_soc::table::clock(ProviderKind::Apmu, k1::CLK_SDH2).unwrap();
    // pll1_d4 614.4 MHz / 2
    ex.set_mux_div(sdh2, APMU_BASE, 1, 1).unwrap();
    assert_eq!(ex.rate(sdh2, APMU_BASE), 307_200_000);
    let writes = bus.writes();
    assert_eq!(writes.len(), 2, "fields, then the FC bit");
    assert_eq!(writes[1].1 & (1 << 11), 1 << 11);
    assert_eq!(ex.set_mux_div(sdh2, APMU_BASE, 8, 0), Err(Fault::Range));
}

#[test]
fn test_reject_stuck_frequency_change_bit() {
    let mut bus = MockBus::stock();
    bus.stuck_fc = Some((APMU_BASE + 0x0e0, 1 << 11));
    let ex = Executor::new(&bus);
    let sdh2 = nexus_soc::table::clock(ProviderKind::Apmu, k1::CLK_SDH2).unwrap();
    assert!(matches!(ex.set_mux_div(sdh2, APMU_BASE, 1, 1), Err(Fault::FcStuck { .. })));
}

#[test]
fn test_reject_a_write_that_does_not_read_back() {
    let (fdt, providers) = board_providers();
    let emmc = fdt.node_at_path("/soc/storage-bus/mmc@d4281000").unwrap();
    let plan = plan(emmc, &providers).unwrap();
    let err = Executor::new(&DeadBus).execute(&plan).unwrap_err();
    assert_eq!(err, Fault::ReadBack { addr: APMU_BASE + 0x054, value: 0 });
}

#[test]
fn test_reject_an_id_the_tables_do_not_know() {
    let (fdt, providers) = board_providers();
    // A pad group binds no clocks; a node with clocks of an unknown provider kind
    // is refused by kind, an unknown id by id — both before any bus access.
    let sd = fdt.node_at_path("/soc/storage-bus/mmc@d4280000").unwrap();
    let p = plan(sd, &providers).unwrap();
    assert!(p.steps().any(|s| matches!(s, Step::GateOn { .. })));
    let mut no_apmu = Providers::new();
    no_apmu.set(providers.get(ProviderKind::Apbc).unwrap());
    assert_eq!(plan(sd, &no_apmu), Err(PlanError::ProviderUnknown(ProviderKind::Apmu)));
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
