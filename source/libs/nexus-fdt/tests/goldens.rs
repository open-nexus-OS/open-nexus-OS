// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the two trees a boot stage will actually be handed — QEMU `virt`
//! (dumped with the launcher's machine options: `-machine virt,aclint=on -cpu max
//! -m 320M -smp 4`) and the reference board (compiled from
//! `config/board/bpi-f3/board.dts` with `dtc -p 512`). Every value asserted here
//! is a value TASK-0245's kernel platform will read; the numbers are the ones
//! measured on 2026-09-22 (`docs/board/measurements/2026-09-22-stock-system`).

use nexus_fdt::{ChosenWriter, Error, Fdt};

const VIRT: &[u8] = include_bytes!("goldens/virt.dtb");
const BOARD: &[u8] = include_bytes!("goldens/bpi-f3.dtb");

#[test]
fn virt_memory_is_one_bank_at_0x8000_0000() {
    let fdt = Fdt::new(VIRT).unwrap();
    let banks: Vec<_> = fdt.memory_banks().collect();
    assert_eq!(banks.len(), 1);
    assert_eq!(banks[0].base, 0x8000_0000);
    assert_eq!(banks[0].size, 320 * 1024 * 1024);
}

#[test]
fn board_memory_is_two_banks_with_a_hole_and_opensbi_is_reserved() {
    let fdt = Fdt::new(BOARD).unwrap();
    let banks: Vec<_> = fdt.memory_banks().collect();
    assert_eq!(banks.len(), 2);
    assert_eq!((banks[0].base, banks[0].size), (0x0, 0x8000_0000));
    assert_eq!((banks[1].base, banks[1].size), (0x1_0000_0000, 0x8000_0000));
    let reserved: Vec<_> = fdt.reserved_ranges().collect();
    assert!(reserved.iter().any(|r| r.base == 0 && r.size == 0x8_0000 && r.no_map));
}

#[test]
fn timebase_and_harts_differ_between_the_two_machines() {
    let virt = Fdt::new(VIRT).unwrap().cpus().unwrap();
    assert_eq!(virt.timebase_hz, 10_000_000);
    assert_eq!(virt.count(), 4);

    let board = Fdt::new(BOARD).unwrap().cpus().unwrap();
    assert_eq!(board.timebase_hz, 24_000_000);
    assert_eq!(board.count(), 8);
    let harts: Vec<u32> = board.harts().map(|c| c.hart).collect();
    assert_eq!(harts, [0, 1, 2, 3, 4, 5, 6, 7]);
    let map = board.cpu_map().expect("cpu-map");
    assert_eq!(map.clusters(), 2);
    let cpu5 = board.harts().find(|c| c.hart == 5).unwrap();
    assert_eq!(map.cluster_of(cpu5.phandle().unwrap()), Some(1));
}

#[test]
fn sstc_decides_the_timer_on_both_machines() {
    for (name, tree) in [("virt", VIRT), ("board", BOARD)] {
        let cpus = Fdt::new(tree).unwrap().cpus().unwrap();
        let hart0 = cpus.harts().next().unwrap();
        assert!(hart0.has_extension("sstc"), "{name}: sstc expected");
    }
    let board = Fdt::new(BOARD).unwrap().cpus().unwrap();
    let hart0 = board.harts().next().unwrap();
    assert!(hart0.has_extension("zicbom"));
    assert!(hart0.has_extension("svpbmt"));
    assert_eq!(hart0.mmu(), Some("riscv,sv39"));
}

#[test]
fn plic_base_ndev_and_s_contexts_come_from_the_tree() {
    let virt = Fdt::new(VIRT).unwrap();
    let plic = virt.plic().expect("virt plic");
    let reg = plic.reg(0).unwrap().unwrap();
    assert_eq!(reg.addr, 0x0c00_0000);
    assert_eq!(plic.plic_ndev(), Some(0x5f));
    let ctx: Vec<_> = plic.plic_s_contexts().collect();
    // Each hart's S-mode context is the odd slot of its `<intc 11>, <intc 9>` pair,
    // in the order QEMU lists them (hart 0 first): the numbers the kernel's PLIC
    // driver used to derive as `2 * hart + 1` are now READ.
    assert_eq!(ctx, [(0, 1), (1, 3), (2, 5), (3, 7)]);

    let board = Fdt::new(BOARD).unwrap();
    let plic = board.plic().expect("board plic");
    let reg = plic.reg(0).unwrap().unwrap();
    assert_eq!((reg.addr, reg.size), (0xe000_0000, 0x400_0000));
    assert_eq!(plic.plic_ndev(), Some(159));
    let ctx: Vec<_> = plic.plic_s_contexts().collect();
    assert_eq!(ctx, [(0, 1), (1, 3), (2, 5), (3, 7), (4, 9), (5, 11), (6, 13), (7, 15)]);
}

#[test]
fn the_console_is_resolved_through_stdout_path_and_aliases() {
    let virt = Fdt::new(VIRT).unwrap();
    let uart = virt.stdout().expect("virt console");
    assert!(uart.is_compatible("ns16550a"));
    assert_eq!(uart.reg(0).unwrap().unwrap().addr, 0x1000_0000);
    assert_eq!(uart.prop_u32("reg-shift"), None);

    let board = Fdt::new(BOARD).unwrap();
    let uart = board.stdout().expect("board console via serial0 alias");
    assert!(uart.is_compatible("spacemit,k1-uart"));
    assert_eq!(uart.reg(0).unwrap().unwrap().addr, 0xd401_7000);
    assert_eq!(uart.prop_u32("reg-shift"), Some(2));
    assert_eq!(uart.interrupts().next(), Some(42));
}

#[test]
fn devices_are_found_by_compatible_with_registers_and_interrupt_lines() {
    let virt = Fdt::new(VIRT).unwrap();
    let mmio: Vec<_> = virt.find_compatible(&["virtio,mmio"]).collect();
    assert_eq!(mmio.len(), 8);
    let lowest = mmio.iter().map(|n| n.reg(0).unwrap().unwrap().addr).min().unwrap();
    assert_eq!(lowest, 0x1000_1000);
    // The interrupt line is a property, never slot arithmetic.
    let at_1000 = mmio.iter().find(|n| n.reg(0).unwrap().unwrap().addr == 0x1000_1000).unwrap();
    assert_eq!(at_1000.interrupts().next(), Some(1));

    let board = Fdt::new(BOARD).unwrap();
    let sdh: Vec<_> = board.find_compatible(&["spacemit,k1-sdhci"]).collect();
    assert_eq!(sdh.len(), 3);
    let emmc = sdh.iter().find(|n| n.reg(0).unwrap().unwrap().addr == 0xd428_1000).unwrap();
    assert_eq!(emmc.interrupts().next(), Some(101));
    assert_eq!(emmc.prop_u32("bus-width"), Some(8));
    let dpu = board.find_compatible(&["spacemit,dpu-online2"]).next().unwrap();
    assert_eq!(dpu.interrupts().collect::<Vec<_>>(), [139, 138]);
    let gpu = board.find_compatible(&["img,rgx"]).next().unwrap();
    assert_eq!(gpu.reg(0).unwrap().unwrap(), nexus_fdt::Reg { addr: 0xcac0_0000, size: 0x8_0000 });
    assert!(!board.find_compatible(&["spacemit,k1-rtc"]).next().unwrap().is_enabled());
}

#[test]
fn nxboot_writes_chosen_in_headroom_and_the_kernel_reads_it_back() {
    let mut buf = BOARD.to_vec();
    let mut w = ChosenWriter::new(&mut buf).unwrap();
    assert!(w.headroom() >= 400, "dtc -p 512 headroom expected, got {}", w.headroom());

    w.set_nexus_str("boot-profile", "headless").unwrap();
    w.set_nexus_str("boot-slot", "a").unwrap();
    w.set_nexus_u64("boot-record", 0x8000_1234).unwrap();
    // Overwrite with a different length: remove + insert.
    w.set_nexus_str("boot-profile", "visible").unwrap();
    // Same length: in-place.
    w.set_nexus_str("boot-slot", "b").unwrap();

    let fdt = Fdt::new(&buf).unwrap();
    let chosen = fdt.chosen().unwrap();
    assert_eq!(chosen.nexus_str("boot-profile"), Some("visible"));
    assert_eq!(chosen.nexus_str("boot-slot"), Some("b"));
    assert_eq!(chosen.nexus_u64("boot-record"), Some(0x8000_1234));
    assert_eq!(chosen.stdout_path(), Some("serial0:115200n8"));
    // Nothing else moved: the rest of the tree still parses to the same values.
    assert_eq!(fdt.cpus().unwrap().count(), 8);
    assert_eq!(fdt.find_compatible(&["spacemit,k1-sdhci"]).count(), 3);
}

#[test]
fn a_write_without_headroom_is_refused_and_leaves_the_tree_intact() {
    let mut buf = VIRT.to_vec();
    let before = buf.clone();
    let mut w = ChosenWriter::new(&mut buf).unwrap();
    assert_eq!(w.set_nexus_str("boot-profile", "headless"), Err(Error::NoHeadroom));
    assert_eq!(buf, before);
}

#[test]
fn virt_chosen_writes_work_once_headroom_exists() {
    let mut buf = VIRT.to_vec();
    buf.resize(buf.len() + 512, 0);
    let mut w = ChosenWriter::new(&mut buf).unwrap();
    w.set_nexus_str("boot-profile", "smp1").unwrap();
    w.set_nexus_str("display-mode", "1280x800").unwrap();
    let fdt = Fdt::new(&buf).unwrap();
    assert_eq!(fdt.chosen().unwrap().nexus_str("boot-profile"), Some("smp1"));
    assert_eq!(fdt.chosen().unwrap().nexus_str("display-mode"), Some("1280x800"));
    assert_eq!(fdt.stdout().unwrap().reg(0).unwrap().unwrap().addr, 0x1000_0000);
}
