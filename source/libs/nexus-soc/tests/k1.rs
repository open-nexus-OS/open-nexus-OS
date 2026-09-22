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
use nexus_soc::{plan, Executor, Fault, PlanError, ProviderKind, Providers, Step};

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
}

impl MockBus {
    fn cold() -> Self {
        MockBus {
            regs: RefCell::new(HashMap::new()),
            writes: RefCell::new(Vec::new()),
            stuck_fc: None,
            self_clearing: Vec::new(),
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
    let emmc = fdt.node_at_path("/soc/mmc@d4281000").unwrap();
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
    let emmc = fdt.node_at_path("/soc/mmc@d4281000").unwrap();
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
    for path in
        ["/soc/usb@c0a00000", "/soc/usb@c0980100", "/soc/usb@c0900100", "/soc/ethernet@cac80000"]
    {
        let node = fdt.node_at_path(path).unwrap();
        let plan = plan(node, &providers).unwrap();
        let report = Executor::new(&bus).execute(&plan).unwrap();
        assert_eq!(report.writes, 0, "{path} glue already on");
    }
}

#[test]
fn the_display_controller_is_off_on_the_stock_board_and_hdmi_is_on() {
    let (fdt, providers) = board_providers();
    let bus = MockBus::stock();
    let hdmi = fdt.node_at_path("/soc/hdmi@c0400500").unwrap();
    // HDMI lives in domain 7: the planner refuses until P3 measures the protocol.
    assert_eq!(plan(hdmi, &providers), Err(PlanError::DomainUnsupported(7)));
    let ex = Executor::new(&bus);
    let hmclk = nexus_soc::table::clock(ProviderKind::Apmu, k1::CLK_HDMI).unwrap();
    assert!(ex.is_on(hmclk, APMU_BASE), "the stock desktop is on HDMI");
    let dpu_hclk = nexus_soc::table::clock(ProviderKind::Apmu, k1::CLK_DPU_HCLK).unwrap();
    assert!(!ex.is_on(dpu_hclk, APMU_BASE), "clk_summary: dpu_hclk off");
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
    struct DeadBus;
    impl Bus for DeadBus {
        fn read(&self, _addr: usize) -> u32 {
            0
        }
        fn write(&self, _addr: usize, _value: u32) {}
    }
    let (fdt, providers) = board_providers();
    let emmc = fdt.node_at_path("/soc/mmc@d4281000").unwrap();
    let plan = plan(emmc, &providers).unwrap();
    let err = Executor::new(&DeadBus).execute(&plan).unwrap_err();
    assert_eq!(err, Fault::ReadBack { addr: APMU_BASE + 0x054, value: 0 });
}

#[test]
fn test_reject_an_id_the_tables_do_not_know() {
    let (fdt, providers) = board_providers();
    // A pad group binds no clocks; a node with clocks of an unknown provider kind
    // is refused by kind, an unknown id by id — both before any bus access.
    let sd = fdt.node_at_path("/soc/mmc@d4280000").unwrap();
    let p = plan(sd, &providers).unwrap();
    assert!(p.steps().any(|s| matches!(s, Step::GateOn { .. })));
    let mut no_apmu = Providers::new();
    no_apmu.set(providers.get(ProviderKind::Apbc).unwrap());
    assert_eq!(plan(sd, &no_apmu), Err(PlanError::ProviderUnknown(ProviderKind::Apmu)));
}
