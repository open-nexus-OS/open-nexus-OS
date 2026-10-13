// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: socd's verdict on the host — the same function the service loop
//! runs, over the golden trees and a register file seeded from the board's
//! measured APMU state. QEMU virt → NotNeeded; the board's eMMC and display
//! pipeline → OK with the documented steps; denied / unknown node / malformed /
//! an unmeasured domain → their statuses; the marker the loop prints, with every
//! register a bring-up touched before and after.

use std::cell::RefCell;
use std::collections::HashMap;

use nexus_fdt::Fdt;
use nexus_hal::Bus;
use nexus_soc::Providers;
use nexus_wire::soc;
use socd::verdict::{
    answer, bring_up_marker, required_capability, Access, Outcome, Word, MARKER_MAX, REPLY_MAX,
};

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
    bring_up_with_outcome(frame_path, access, tree, bus).0
}

fn bring_up_with_outcome(
    frame_path: &str,
    access: Access,
    tree: &[u8],
    bus: &MockBus,
) -> (soc::BringUpReply, Outcome) {
    let fdt = Fdt::new(tree).unwrap();
    let providers = providers_of(&fdt);
    let mut req = [0u8; 128];
    let n = soc::encode_bring_up_req(&mut req, 0xabcd, frame_path).unwrap();
    let mut out = [0u8; REPLY_MAX];
    let (len, outcome) = answer(&req[..n], access, Some(&fdt), &providers, bus, None, &mut out);
    (soc::decode_bring_up_rsp(&out[..len]).expect("a bring-up reply"), outcome)
}

const EMMC: &str = "/soc/storage-bus/mmc@d4281000";
const DPU: &str = "/soc/multimedia-bus/display@c0440000";

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
        answer(b"RG\x01\x01", Access::Allowed, Some(&fdt), &providers, &bus, None, &mut out);
    assert_eq!(outcome.status, soc::STATUS_MALFORMED);
    assert!(len > 0);
}

/// The executor's pause for a software-sequenced domain: the test needs no time.
struct NoWait;
impl nexus_soc::Pause for NoWait {
    fn pause_ms(&self, _ms: u32) {}
    fn pause_us(&self, _us: u32) {}
}

#[test]
fn the_gpu_on_the_stock_board_is_ok_without_a_write_and_names_its_class() {
    // TASK-0329 G1: the GPU's domain is measured (software-sequenced); on the stock words
    // (domain on, clock gated and reset released at 614.4 MHz) nothing is written.
    let fdt = Fdt::new(BOARD).unwrap();
    let providers = providers_of(&fdt);
    let bus = MockBus::stock();
    let gpu = "/soc/multimedia-bus/gpu@cac00000";
    let mut req = [0u8; 128];
    let n = soc::encode_bring_up_req(&mut req, 0xabcd, gpu).unwrap();
    assert_eq!(required_capability(&req[..n], Some(&fdt)), Some(&b"soc.glue.gpu"[..]));
    let mut out = [0u8; REPLY_MAX];
    let (len, outcome) =
        answer(&req[..n], Access::Allowed, Some(&fdt), &providers, &bus, Some(&NoWait), &mut out);
    let rsp = soc::decode_bring_up_rsp(&out[..len]).unwrap();
    assert_eq!((rsp.status, rsp.domains, rsp.clocks, rsp.resets), (soc::STATUS_OK, 1, 1, 1));
    assert_eq!(*bus.1.borrow(), 0, "the stock words need no write");
    assert!(bring_up_marker(gpu, &outcome).as_str().contains("ok"));
}

#[test]
fn test_reject_soc_glue_class_mismatch() {
    // RFC-0106's per-class floor: the capability a node needs is its class's, and a requester
    // policyd denied for it gets DENIED before a register is read — for a node that would
    // touch one. The disk's node is storage, the display's display, the USB host's usb.
    let fdt = Fdt::new(BOARD).unwrap();
    let mut req = [0u8; 160];
    for (path, cap) in [
        ("/soc/storage-bus/mmc@d4281000", &b"soc.glue.storage"[..]),
        ("/soc/multimedia-bus/display@c0440000", &b"soc.glue.display"[..]),
        ("/soc/hdmi@c0400500", &b"soc.glue.display"[..]),
        ("/soc/multimedia-bus/gpu@cac00000", &b"soc.glue.gpu"[..]),
    ] {
        let n = soc::encode_bring_up_req(&mut req, 1, path).unwrap();
        assert_eq!(required_capability(&req[..n], Some(&fdt)), Some(cap), "{path}");
        let n = soc::encode_clock_rate_req(&mut req, 1, path, "io").unwrap();
        assert_eq!(required_capability(&req[..n], Some(&fdt)), Some(cap), "{path} rate");
    }
    let bus = MockBus::stock();
    let rsp = bring_up("/soc/multimedia-bus/gpu@cac00000", Access::Denied, BOARD, &bus);
    assert_eq!(rsp.status, soc::STATUS_DENIED);
    assert_eq!(*bus.1.borrow(), 0);
    // A node of no class, or no node: no capability (the caller denies without a policy trip).
    let n = soc::encode_bring_up_req(&mut req, 1, "/soc/nothing@0").unwrap();
    assert_eq!(required_capability(&req[..n], Some(&fdt)), None);
    assert_eq!(required_capability(b"RG\x01\x01", Some(&fdt)), None);
    assert_eq!(required_capability(&req[..n], None), None);
}

#[test]
fn a_node_that_touches_nothing_is_not_needed_for_any_requester() {
    // QEMU's virtio transport has no SoC glue: the floor has nothing to guard — `NotNeeded`
    // for a denied requester too (the harness holds no class for it and proves exactly this).
    let virt = Fdt::new(VIRT).unwrap();
    let net = virt.find_compatible(&["virtio,mmio"]).next().unwrap();
    let mut buf = [0u8; 128];
    let path = net.path_into(&mut buf).unwrap().to_string();
    let mut req = [0u8; 128];
    let n = soc::encode_bring_up_req(&mut req, 7, &path).unwrap();
    assert_eq!(required_capability(&req[..n], Some(&virt)), None, "no class, no glue");
    let bus = MockBus::empty();
    let rsp = bring_up(&path, Access::Denied, VIRT, &bus);
    assert_eq!(rsp.status, soc::STATUS_NOT_NEEDED);
    assert_eq!(*bus.1.borrow(), 0);
}

#[test]
fn the_display_pipeline_on_the_stock_board_is_ok_without_a_write() {
    let bus = MockBus::stock();
    let (rsp, outcome) = bring_up_with_outcome(DPU, Access::Allowed, BOARD, &bus);
    assert_eq!(rsp.status, soc::STATUS_OK);
    assert_eq!((rsp.domains, rsp.resets, rsp.clocks), (1, 1, 1));
    assert_eq!((outcome.rates, outcome.writes), (1, 0), "hmclk already at 491.52 MHz");
    assert_eq!(*bus.1.borrow(), 0, "the stock desktop is on HDMI");
}

#[test]
fn the_bring_up_marker_names_every_register_before_and_after() {
    let bus = MockBus::stock();
    let (_, outcome) = bring_up_with_outcome(EMMC, Access::Allowed, BOARD, &bus);
    assert_eq!(
        bring_up_marker(EMMC, &outcome).as_str(),
        "socd: bring-up /soc/storage-bus/mmc@d4281000 ok (domains=1 resets=2 clocks=2 rates=0 \
         pads=0 gpios=0 writes=0) apmu+54:411b>411b apmu+e0:52>52"
    );
    // From cold with no power sequencer answering: the domain step fails with the status word
    // it read, and the words show what the bring-up left — the request raised, nothing else.
    let cold = MockBus::empty();
    let (rsp, outcome) = bring_up_with_outcome(DPU, Access::Allowed, BOARD, &cold);
    assert_eq!(rsp.status, soc::STATUS_FAILED);
    assert_eq!(rsp.fault_value, 0);
    assert_eq!(
        bring_up_marker(DPU, &outcome).as_str(),
        "socd: bring-up /soc/multimedia-bus/display@c0440000 FAIL (step=domain reg=apmu+0xf0 \
         val=0x0) apmu+3f4:0>11 apmu+f0:0>0 apmu+1b8:0>0"
    );
}

#[test]
fn a_marker_too_long_for_a_console_line_counts_the_words_it_drops() {
    let mut outcome = {
        let bus = MockBus::stock();
        bring_up_with_outcome(EMMC, Access::Allowed, BOARD, &bus).1
    };
    let wide = Word { addr: 0, window: "apbc2", offset: 0x3fc, before: u32::MAX, after: u32::MAX };
    outcome.words = [wide; 8];
    outcome.nwords = 8;
    let path = "/soc/a-very-long-bus-name-for-this-test/another-long-bus/device@deadbeef";
    let marker = bring_up_marker(path, &outcome);
    let line = marker.as_str();
    assert!(line.len() <= MARKER_MAX, "{} bytes", line.len());
    assert!(line.ends_with(" more"), "the dropped words are counted: {line}");
    // Every word printed is whole: each token after the verdict is the full word, until the count.
    let words: Vec<&str> = line
        .split(") ")
        .nth(1)
        .unwrap_or("")
        .split(' ')
        .take_while(|t| !t.starts_with('+'))
        .collect();
    assert!(!words.is_empty(), "some words fit: {line}");
    assert!(words.iter().all(|t| *t == "apbc2+3fc:ffffffff>ffffffff"), "no word is cut: {line}");
}

#[test]
fn clock_rate_reads_the_emmc_io_clock_from_the_stock_registers() {
    let fdt = Fdt::new(BOARD).unwrap();
    let providers = providers_of(&fdt);
    let bus = MockBus::stock();
    let mut req = [0u8; 160];
    let n = soc::encode_clock_rate_req(&mut req, 5, "/soc/storage-bus/mmc@d4281000", "io").unwrap();
    let mut out = [0u8; REPLY_MAX];
    let (len, _) = answer(&req[..n], Access::Allowed, Some(&fdt), &providers, &bus, None, &mut out);
    assert_eq!(soc::decode_clock_rate_rsp(&out[..len]), Some((soc::STATUS_OK, 5, 375_000_000)));
}
