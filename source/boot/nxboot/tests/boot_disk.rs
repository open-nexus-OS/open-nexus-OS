// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The boot disk under whole loader decisions (TASK-0246B P1/P2) against the shared
//! fixture disk (`common/`): the candidate rule, the clean boot and the trial ladder's BSB writes
//! over the SDHCI reader against the core's behavioural machine; and the board path — the tree's
//! SD hosts, their glue, their clock — from the measured stock state and from cold.
//! OWNERS: @security
//! ADR: docs/adr/0059-first-stage-boot-chain-nxboot-handoff.md

mod common;

use bootfmt::bsb::Slot;
use common::*;
use nxboot::flow::{self, Event, SECTOR};
use storage::gpt::write_gpt;
use storage::layout::{plan, NEXUS_DISK_BYTES};
use storage::{BlockDevice, MemBlockDevice};

// ---- The boot disk (TASK-0246B P1): the rule, and the SDHCI reader under whole decisions ----

use nxboot::disk::{self, sdhci::SdhciDisk, Skip};
use storage_sdhci_model as model;

type SdDisk = SdhciDisk<model::ModelBus, model::SimPlatform>;

/// The eMMC behind `m`'s host, opened from power-up and polled, as the loader drives it.
fn sd_open(m: &model::Shared) -> Result<SdDisk, storage_sdhci::Error> {
    let platform = model::SimPlatform { m: m.clone(), irq: false };
    SdhciDisk::open(model::ModelBus(m.clone()), platform, model::host_config(m))
}

#[test]
fn a_whole_boot_decision_runs_over_the_sdhci_reader() {
    let m = model::machine(model::qemu_8bit());
    let mut fx = factory_fixture_on(sd_open(&m).expect("card"));
    let (got, events, dest) = run(&mut fx);
    let loaded = got.expect("clean boot over sdhci");
    assert_eq!(loaded.slot, Slot::A);
    assert_placed(&dest, &fx.kernel_a, &loaded);
    // The descriptor the reader handed the flow is boot-a's first sector, byte for byte.
    let mut sector = [0u8; SECTOR];
    fx.dev.read_blocks(fx.boot_a.first_lba, &mut sector).expect("nxbd sector over sdhci");
    assert_eq!(sector, signed_nxbd(&fx.kernel_a, "build-A", 1, &dev_seed()));
    assert_eq!(events[0], Event::BsbOk { slot: Slot::A, seq: 1 });
    assert!(m.borrow().mem.ops.is_empty(), "no DMA memory: the loader reads by PIO");
}

#[test]
fn the_trial_ladder_writes_the_bsb_through_the_sdhci_reader() {
    let m = model::machine(model::qemu_8bit());
    let mut fx = factory_fixture_on(sd_open(&m).expect("card"));
    let kernel_b = kernel_fixture(0xB7);
    let nxbd_b = signed_nxbd(&kernel_b, "build-B", 2, &dev_seed());
    write_slot(&mut fx.dev, &fx.boot_b.clone(), &kernel_b, &nxbd_b);
    schedule_trial(&mut fx, 1, 2);
    // Each decision re-reads the BSB from the card: the next `from` is the last one's write.
    let (got, events, dest) = run(&mut fx);
    let loaded = got.expect("trial 1");
    assert_eq!(loaded.slot, Slot::B);
    assert_eq!(events[1], Event::Tries { slot: Slot::B, from: 2, to: 1 });
    assert_placed(&dest, &kernel_b, &loaded);
    let (got, events, _) = run(&mut fx);
    assert_eq!(got.expect("trial 2").slot, Slot::B);
    assert_eq!(events[1], Event::Tries { slot: Slot::B, from: 1, to: 0 });
    let (got, events, _) = run(&mut fx);
    assert_eq!(got.expect("exhausted").slot, Slot::A);
    assert_eq!(events[1], Event::Exhausted { attempted: Slot::B, to: Slot::A });
}

#[test]
fn the_first_candidate_carrying_a_valid_bsb_is_the_boot_disk() {
    let blank = || MemBlockDevice::new(SECTOR, NEXUS_DISK_BYTES / SECTOR as u64);
    let open = |c: &u8| -> Result<MemBlockDevice, &'static str> {
        match c {
            0 => Err("silent"),
            1 => Ok(blank()),
            2 => {
                // Our layout, both BSB blocks zeroed.
                let mut dev = blank();
                write_gpt(&mut dev, &plan().expect("plan")).expect("gpt");
                Ok(dev)
            }
            _ => Ok(factory_fixture().dev),
        }
    };
    let mut skipped = Vec::new();
    let (picked, _) =
        disk::pick([0u8, 1, 2, 3, 4], open, |c, why| skipped.push((*c, why))).expect("a disk");
    assert_eq!(picked, 3, "the first with a valid BSB; the one after it is never opened");
    assert_eq!(
        skipped,
        [(0, Skip::NoDevice("silent")), (1, Skip::NoLayout), (2, Skip::NoValidBsb)]
    );
    assert!(disk::pick([0u8, 1, 2], open, |_, _| {}).is_none());
}

#[test]
fn a_silent_sd_host_is_skipped_and_a_card_left_initialised_is_initialised_again() {
    let silent = model::machine(model::qemu_8bit());
    silent.borrow_mut().faults.silent = Some(1);
    // The boot medium: written by one reader, then opened again from power-up — the card is still
    // in the transfer state its last user left (the board's SPL does the same).
    let emmc = model::machine(model::qemu_8bit());
    let fx = factory_fixture_on(sd_open(&emmc).expect("card"));
    drop(fx);
    let hosts = [&silent, &emmc];
    let mut skipped = Vec::new();
    let (picked, mut dev) = disk::pick(
        0..hosts.len(),
        |i| sd_open(hosts[*i]).map_err(|_| "no card"),
        |i, why| skipped.push((*i, why)),
    )
    .expect("the eMMC");
    assert_eq!((picked, skipped), (1, vec![(0, Skip::NoDevice("no card"))]));
    let mut dest = vec![0u8; 4 * 1024 * 1024];
    let loaded = flow::run(&mut dev, &mut dest, &mut |_| {}).expect("boot");
    assert_eq!(loaded.slot, Slot::A);
}

// ---- The board path (TASK-0246B P2): the tree's SD hosts, their glue, their clock ----

use std::cell::RefCell;
use std::collections::HashMap;

use nexus_fdt::Fdt;
use nexus_hal::Bus;
use nexus_soc::{Fault, PlanError};
use nxboot::disk::node::{self as sd_node, Refusal};
use storage_sdhci::Layer;

const BOARD: &[u8] = include_bytes!("../../../libs/nexus-fdt/tests/goldens/bpi-f3.dtb");
const VIRT: &[u8] = include_bytes!("../../../libs/nexus-fdt/tests/goldens/virt.dtb");
/// SD hosts the real trees cannot show: one binding no glue, one with a bus no host has.
const FIXTURE: &[u8] = include_bytes!("../../../../userspace/storage/tests/fixtures/boot-disk.dtb");
/// The APMU window as the stock system runs it (measured 2026-09-22): what the SPL leaves.
const APMU_STOCK: &str =
    include_str!("../../../../docs/board/measurements/2026-09-22-stock-system/regmap-apmu.txt");
const APMU_BASE: usize = 0xd428_2800;
const EMMC: &str = "/soc/storage-bus/mmc@d4281000";

/// The provider windows as a register file: absolute address → word, every write logged.
#[derive(Default)]
struct Regs {
    words: RefCell<HashMap<usize, u32>>,
    writes: RefCell<Vec<(usize, u32)>>,
}

impl Regs {
    fn stock() -> Self {
        let regs = Regs::default();
        for line in APMU_STOCK.lines() {
            let Some((off, val)) = line.split_once(": ") else { continue };
            let off = usize::from_str_radix(off.trim(), 16).expect("offset");
            let val = u32::from_str_radix(val.trim(), 16).expect("value");
            regs.words.borrow_mut().insert(APMU_BASE + off, val);
        }
        regs
    }
}

impl Bus for Regs {
    fn read(&self, addr: usize) -> u32 {
        self.words.borrow().get(&addr).copied().unwrap_or(0)
    }
    fn write(&self, addr: usize, value: u32) {
        self.writes.borrow_mut().push((addr, value));
        self.words.borrow_mut().insert(addr, value);
    }
}

/// Nothing answers at the windows.
struct DeadBus;

impl Bus for DeadBus {
    fn read(&self, _addr: usize) -> u32 {
        0
    }
    fn write(&self, _addr: usize, _value: u32) {}
}

#[test]
fn the_rule_takes_the_tree_hosts_lowest_address_first() {
    let fixture = Fdt::new(FIXTURE).unwrap();
    let bases: Vec<usize> = sd_node::emmc_hosts(&fixture).into_iter().map(|(b, _)| b).collect();
    assert_eq!(bases, [0xd428_1000, 0xd428_2000, 0xd428_3000, 0xd428_4000], "not the tree's order");
    assert!(sd_node::emmc_hosts(&Fdt::new(VIRT).unwrap()).is_empty(), "QEMU lists none");
}

#[test]
fn test_reject_a_host_marked_no_mmc_so_the_board_offers_only_its_emmc() {
    let board = Fdt::new(BOARD).unwrap();
    for path in ["/soc/storage-bus/mmc@d4280000", "/soc/storage-bus/mmc@d4280800"] {
        let host = board.node_at_path(path).unwrap();
        assert!(host.prop("no-mmc").is_some(), "{path}: the live tree's flag");
    }
    let hosts: Vec<(usize, &str)> =
        sd_node::emmc_hosts(&board).into_iter().map(|(b, n)| (b, n.name())).collect();
    assert_eq!(hosts, [(0xd428_1000, "mmc@d4281000")]);
}

#[test]
fn from_the_measured_state_the_emmc_host_needs_no_write_and_runs_at_375_mhz() {
    let board = Fdt::new(BOARD).unwrap();
    let emmc = board.node_at_path(EMMC).unwrap();
    let regs = Regs::stock();
    let config = sd_node::open_config(&board, emmc, &regs).expect("the eMMC host");
    assert_eq!(config.base_clock_hz, Some(375_000_000), "sdh2_clk: pll2_d8, divider 1");
    assert_eq!((config.bus_width, config.hs400es, config.layer), (8, true, Layer::K1));
    assert!(regs.writes.borrow().is_empty(), "the SPL left the eMMC glue on");
}

#[test]
fn from_cold_the_loader_writes_the_documented_glue_bits_and_reads_what_they_select() {
    let board = Fdt::new(BOARD).unwrap();
    let emmc = board.node_at_path(EMMC).unwrap();
    let regs = Regs::default();
    let config = sd_node::open_config(&board, emmc, &regs).expect("up from cold");
    let (sdh0, sdh2) = (APMU_BASE + 0x054, APMU_BASE + 0x0e0);
    assert_eq!(
        *regs.writes.borrow(),
        [(sdh0, 1 << 0), (sdh2, 1 << 1), (sdh0, (1 << 0) | (1 << 3)), (sdh2, (1 << 1) | (1 << 4))],
        "sdh_axi released, sdh2 released, sdh_axi gated on, sdh2 gated on"
    );
    assert_eq!(config.base_clock_hz, Some(409_600_000), "zeroed fields: mux 0 = pll1_d6");
}

#[test]
fn a_whole_boot_decision_runs_over_the_board_emmc_host() {
    let board = Fdt::new(BOARD).unwrap();
    let emmc = board.node_at_path(EMMC).unwrap();
    let config = sd_node::open_config(&board, emmc, &Regs::stock()).expect("config");
    // The K1 machine's host runs its `io` clock at the 375 MHz the registers select.
    let m = model::machine(model::k1());
    let platform = model::SimPlatform { m: m.clone(), irq: false };
    let disk = SdhciDisk::open(model::ModelBus(m.clone()), platform, config).expect("the eMMC");
    let mut fx = factory_fixture_on(disk);
    let (got, events, dest) = run(&mut fx);
    let loaded = got.expect("a clean boot over the K1 host");
    assert_eq!(loaded.slot, Slot::A);
    assert_placed(&dest, &fx.kernel_a, &loaded);
    assert_eq!(events[0], Event::BsbOk { slot: Slot::A, seq: 1 });
    assert!(m.borrow().mem.ops.is_empty(), "no DMA memory: the loader reads by PIO");
}

#[test]
fn test_reject_glue_the_tables_do_not_cover_or_that_does_not_read_back() {
    let board = Fdt::new(BOARD).unwrap();
    let emmc = board.node_at_path(EMMC).unwrap();
    let fault = Fault::ReadBack { addr: APMU_BASE + 0x054, value: 0 };
    assert_eq!(sd_node::open_config(&board, emmc, &DeadBus), Err(Refusal::Glue(fault)));
    assert_eq!(Refusal::Glue(fault).name(), "soc glue failed");
    // A node whose glue the loader cannot run is refused before any bus access: the GPU's
    // power domain 2 is software-sequenced (TASK-0329 G1) and needs the executor's pause for
    // its settles — the loader has none (its boot disk never did), so the node is refused at
    // the first settle, with nothing written. A domain the tables do not hold at all (the
    // VPU's, id 1) has no protocol: `DomainUnsupported` stays the refusal for such a node.
    let gpu = board.node_at_path("/soc/multimedia-bus/gpu@cac00000").unwrap();
    let regs = Regs::default();
    assert_eq!(sd_node::open_config(&board, gpu, &regs), Err(Refusal::Glue(Fault::NoPause)));
    assert!(regs.writes.borrow().is_empty());
    assert_eq!(nexus_soc::table::domain(nexus_soc::ProviderKind::Apmu, 1), None);
    let _ = PlanError::DomainUnsupported(1);
}

#[test]
fn test_reject_a_k1_host_without_an_io_clock_or_a_bus() {
    let fixture = Fdt::new(FIXTURE).unwrap();
    // It binds no glue: nothing to bring up, and no clock to run the host at — never a guess.
    let bare = fixture.node_at_path("/soc/mmc@d4281000").unwrap();
    assert_eq!(sd_node::open_config(&fixture, bare, &Regs::default()), Err(Refusal::NoIoClock));
    let odd = fixture.node_at_path("/soc/mmc@d4283000").unwrap();
    assert_eq!(sd_node::open_config(&fixture, odd, &Regs::default()), Err(Refusal::HostConfig));
}
