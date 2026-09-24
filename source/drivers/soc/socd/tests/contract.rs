// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: socd's verdict on the host — the same function the service loop
//! runs, over the golden trees and a register file seeded from the board's
//! measured APMU state. QEMU virt → NotNeeded; the board's eMMC → OK with the
//! documented steps; denied / unknown node / malformed → their statuses.

use std::cell::RefCell;
use std::collections::HashMap;

use nexus_fdt::Fdt;
use nexus_hal::Bus;
use nexus_soc::Providers;
use nexus_wire::soc;
use socd::verdict::{answer, Access, REPLY_MAX};

const BOARD: &[u8] = include_bytes!("../../../../libs/nexus-fdt/tests/goldens/bpi-f3.dtb");
const VIRT: &[u8] = include_bytes!("../../../../libs/nexus-fdt/tests/goldens/virt.dtb");
const APMU_STOCK: &str =
    include_str!("../../../../../docs/board/measurements/2026-09-22-stock-system/regmap-apmu.txt");
const APMU_BASE: usize = 0xd428_2800;

struct MockBus(RefCell<HashMap<usize, u32>>, RefCell<usize>);

impl MockBus {
    fn stock() -> Self {
        let mut regs = HashMap::new();
        for line in APMU_STOCK.lines() {
            if let Some((off, val)) = line.split_once(": ") {
                regs.insert(
                    APMU_BASE + usize::from_str_radix(off.trim(), 16).unwrap(),
                    u32::from_str_radix(val.trim(), 16).unwrap(),
                );
            }
        }
        MockBus(RefCell::new(regs), RefCell::new(0))
    }
    fn empty() -> Self {
        MockBus(RefCell::new(HashMap::new()), RefCell::new(0))
    }
}

impl Bus for MockBus {
    fn read(&self, addr: usize) -> u32 {
        self.0.borrow().get(&addr).copied().unwrap_or(0)
    }
    fn write(&self, addr: usize, value: u32) {
        *self.1.borrow_mut() += 1;
        self.0.borrow_mut().insert(addr, value);
    }
}

fn providers_of(fdt: &Fdt<'_>) -> Providers {
    Providers::from_tree(fdt, |n| n.reg(0).ok().flatten().map(|r| r.addr as usize))
}

fn bring_up(frame_path: &str, access: Access, tree: &[u8], bus: &MockBus) -> soc::BringUpReply {
    let fdt = Fdt::new(tree).unwrap();
    let providers = providers_of(&fdt);
    let mut req = [0u8; 128];
    let n = soc::encode_bring_up_req(&mut req, 0xabcd, frame_path).unwrap();
    let mut out = [0u8; REPLY_MAX];
    let (len, _) = answer(&req[..n], access, Some(&fdt), &providers, bus, &mut out);
    soc::decode_bring_up_rsp(&out[..len]).expect("a bring-up reply")
}

#[test]
fn a_tree_without_soc_glue_answers_not_needed() {
    let bus = MockBus::empty();
    let rsp = bring_up("/soc/virtio_mmio@10001000", Access::Allowed, VIRT, &bus);
    assert_eq!((rsp.status, rsp.nonce), (soc::STATUS_NOT_NEEDED, 0xabcd));
    assert_eq!(*bus.1.borrow(), 0, "no register is ever touched");
}

#[test]
fn the_emmc_on_the_stock_board_is_ok_without_a_write() {
    let bus = MockBus::stock();
    let rsp = bring_up("/soc/storage-bus/mmc@d4281000", Access::Allowed, BOARD, &bus);
    assert_eq!(rsp.status, soc::STATUS_OK);
    assert_eq!((rsp.domains, rsp.resets, rsp.clocks), (1, 2, 2));
    assert_eq!(*bus.1.borrow(), 0, "the SPL left the eMMC glue on");
}

#[test]
fn the_emmc_from_cold_registers_is_ok_after_four_writes() {
    let bus = MockBus::empty();
    let rsp = bring_up("/soc/storage-bus/mmc@d4281000", Access::Allowed, BOARD, &bus);
    assert_eq!(rsp.status, soc::STATUS_OK);
    assert_eq!(*bus.1.borrow(), 4);
    assert_eq!(bus.read(APMU_BASE + 0x0e0), (1 << 1) | (1 << 4));
}

#[test]
fn test_reject_a_requester_without_soc_glue() {
    let bus = MockBus::stock();
    let rsp = bring_up("/soc/storage-bus/mmc@d4281000", Access::Denied, BOARD, &bus);
    assert_eq!((rsp.status, rsp.nonce), (soc::STATUS_DENIED, 0xabcd));
    assert_eq!(*bus.1.borrow(), 0);
}

#[test]
fn test_reject_an_unknown_node_and_a_malformed_frame() {
    let bus = MockBus::stock();
    let rsp = bring_up("/soc/nothing@0", Access::Allowed, BOARD, &bus);
    assert_eq!(rsp.status, soc::STATUS_NO_SUCH_NODE);
    let fdt = Fdt::new(BOARD).unwrap();
    let providers = providers_of(&fdt);
    let mut out = [0u8; REPLY_MAX];
    let (len, outcome) =
        answer(b"RG\x01\x01", Access::Allowed, Some(&fdt), &providers, &bus, &mut out);
    assert_eq!(outcome.status, soc::STATUS_MALFORMED);
    assert!(len > 0);
}

#[test]
fn a_domain_without_a_measured_protocol_is_unsupported_not_guessed() {
    let bus = MockBus::stock();
    let rsp = bring_up("/soc/hdmi@c0400500", Access::Allowed, BOARD, &bus);
    assert_eq!(rsp.status, soc::STATUS_UNSUPPORTED, "K1_PD_HDMI waits for P3");
    assert_eq!(*bus.1.borrow(), 0);
}

#[test]
fn clock_rate_reads_the_emmc_io_clock_from_the_stock_registers() {
    let fdt = Fdt::new(BOARD).unwrap();
    let providers = providers_of(&fdt);
    let bus = MockBus::stock();
    let mut req = [0u8; 160];
    let n = soc::encode_clock_rate_req(&mut req, 5, "/soc/storage-bus/mmc@d4281000", "io").unwrap();
    let mut out = [0u8; REPLY_MAX];
    let (len, _) = answer(&req[..n], Access::Allowed, Some(&fdt), &providers, &bus, &mut out);
    assert_eq!(soc::decode_clock_rate_rsp(&out[..len]), Some((soc::STATUS_OK, 5, 375_000_000)));
}
