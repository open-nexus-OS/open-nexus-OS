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

/// The boot LED (TASK-0260B P3): the board names its user LED in `/chosen` — bank 3 line 0 of
/// the GPIO block at 0xd4019000, its pad's mux register and value — and QEMU names none.
#[test]
fn the_board_names_its_boot_led_and_qemu_names_none() {
    let board = Fdt::new(BOARD).unwrap();
    let chosen = board.chosen().unwrap();
    let led = chosen.boot_led().expect("the board's boot LED");
    assert_eq!((led.bank, led.line, led.flags), (3, 0, 0));
    assert!(led.gpio.is_compatible("spacemit,k1-gpio"));
    assert_eq!(led.gpio.reg(0).unwrap().unwrap().addr, 0xd401_9000);
    assert_eq!(chosen.boot_led_pad(), Some((0xd401_e180, 0x440)));
    let virt = Fdt::new(VIRT).unwrap();
    assert!(virt.chosen().unwrap().boot_led().is_none());
    assert!(virt.chosen().unwrap().boot_led_pad().is_none());
}

/// On the board the SPL hands this very tree to the pinned OpenSBI (TASK-0260B), so it carries
/// what the firmware reads. OpenSBI's ACLINT drivers serve exactly the harts the CLINT's
/// `interrupts-extended` names — machine software (3) for its IPIs, machine timer (7) for its
/// timer — and without them its timer serves no hart and it stops before our loader (the first
/// board attempt). Its console driver matches `spacemit,pxa-uart`. QEMU brings its own tree.
#[test]
fn the_board_firmware_finds_every_hart_on_the_clint_and_its_console() {
    const M_SOFT: u32 = 3;
    const M_TIMER: u32 = 7;
    let board = Fdt::new(BOARD).unwrap();
    let clint = board.find_compatible(&["sifive,clint0"]).next().expect("the clint");
    let cells = clint.prop("interrupts-extended").expect("the harts it serves");
    let mut served = std::collections::BTreeMap::<u32, Vec<u32>>::new();
    for pair in cells.chunks_exact(8) {
        let intc = u32::from_be_bytes([pair[0], pair[1], pair[2], pair[3]]);
        let cause = u32::from_be_bytes([pair[4], pair[5], pair[6], pair[7]]);
        let hart = board
            .node_by_phandle(intc)
            .and_then(|n| n.parent())
            .and_then(|cpu| cpu.prop_cell("reg", 0))
            .expect("a hart's interrupt controller");
        served.entry(hart).or_default().push(cause);
    }
    let harts: Vec<u32> = served.keys().copied().collect();
    assert_eq!(harts, (0..board.cpus().unwrap().count() as u32).collect::<Vec<_>>());
    for (hart, causes) in &served {
        assert!(causes.contains(&M_SOFT) && causes.contains(&M_TIMER), "hart {hart}: {causes:?}");
    }
    assert!(board.stdout().expect("console").is_compatible("spacemit,pxa-uart"));
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

const NESTED: &[u8] = include_bytes!("goldens/dma-nested.dtb");

fn window(bus: u64, cpu: u64, size: u64) -> nexus_fdt::DmaWindow {
    nexus_fdt::DmaWindow { bus, cpu, size }
}

#[test]
fn the_board_buses_give_each_master_its_measured_dma_reach() {
    let board = Fdt::new(BOARD).unwrap();
    // Storage (SD hosts, DWC3, EHCI, UDC): ONLY the first 2 GiB, identity.
    for path in ["/soc/storage-bus/mmc@d4281000", "/soc/storage-bus/usb@c0a00000"] {
        let reach = board.node_at_path(path).unwrap().dma_reach().unwrap();
        assert_eq!(reach.windows(), [window(0, 0, 0x8000_0000)], "{path}");
    }
    // Network: the first 2 GiB identity plus bus 2 GiB.. → CPU 4 GiB.. (2 GiB).
    let emac = board.node_at_path("/soc/network-bus/ethernet@cac80000").unwrap();
    assert_eq!(
        emac.dma_reach().unwrap().windows(),
        [window(0, 0, 0x8000_0000), window(0x8000_0000, 0x1_0000_0000, 0x8000_0000)]
    );
    // Multimedia (display controller, GPU): the upper bank through a translated window.
    let dpu = board.node_at_path("/soc/multimedia-bus/display@c0440000").unwrap();
    assert_eq!(
        dpu.dma_reach().unwrap().windows(),
        [window(0, 0, 0x8000_0000), window(0x8000_0000, 0x1_0000_0000, 0x3_8000_0000)]
    );
    // Registers are unaffected by the extra level (identity `ranges`).
    let emmc = board.node_at_path("/soc/storage-bus/mmc@d4281000").unwrap();
    assert_eq!(emmc.reg(0).unwrap().unwrap().addr, 0xd428_1000);
    // Not a DMA master and on no constraining bus: every address.
    let hdmi = board.node_at_path("/soc/hdmi@c0400500").unwrap();
    assert_eq!(hdmi.dma_reach().unwrap(), nexus_fdt::DmaReach::All);
}

#[test]
fn qemu_virt_constrains_no_master() {
    let virt = Fdt::new(VIRT).unwrap();
    for node in virt.find_compatible(&["virtio,mmio"]) {
        assert_eq!(node.dma_reach().unwrap(), nexus_fdt::DmaReach::All);
    }
}

#[test]
fn dma_ranges_compose_over_levels_and_an_absent_level_ends_the_walk() {
    let fdt = Fdt::new(NESTED).unwrap();
    // bus-b: device 0..64 MiB → bus-a 128 MiB.., and 1 GiB.. (16 MiB) → bus-a 768 MiB..;
    // bus-a carries only its child 0..256 MiB → CPU 256 MiB+: the first window composes
    // to CPU 384 MiB, the second lies outside bus-a's window and is out of reach.
    let nested = fdt.find_compatible(&["test,dma-nested"]).next().unwrap();
    assert_eq!(nested.dma_reach().unwrap().windows(), [window(0, 0x1800_0000, 0x0400_0000)]);
    // Empty `dma-ranges` at every level: identity, every address.
    let identity = fdt.find_compatible(&["test,dma-identity"]).next().unwrap();
    assert_eq!(identity.dma_reach().unwrap(), nexus_fdt::DmaReach::All);
    // The device's own bus has none: the walk ends — the 1 GiB limit above is not its.
    let open = fdt.find_compatible(&["test,dma-unconstrained"]).next().unwrap();
    assert_eq!(open.dma_reach().unwrap(), nexus_fdt::DmaReach::All);
}

#[test]
fn test_reject_more_dma_windows_than_a_reach_carries() {
    let fdt = Fdt::new(NESTED).unwrap();
    let crowded = fdt.find_compatible(&["test,dma-crowded"]).next().unwrap();
    assert_eq!(crowded.dma_reach(), Err(Error::DmaRanges));
}

#[test]
fn reg_is_translated_through_every_level() {
    let fdt = Fdt::new(NESTED).unwrap();
    let deep = fdt.find_compatible(&["test,xlate-deep"]).next().unwrap();
    assert_eq!(deep.reg(0).unwrap().unwrap(), nexus_fdt::Reg { addr: 0x4100_1000, size: 0x100 });
    // The same translation for any address in the node's parent space.
    assert_eq!(deep.cpu_address(0x1000).unwrap(), 0x4100_1000);
}

#[test]
fn a_device_below_a_bus_without_a_node_reaches_what_the_bus_passes_down() {
    // A PCI function behind QEMU's ECAM host (TASK-0246 P3): no `dma-ranges` on the host,
    // coherent, and its windows are identity in the CPU's space.
    let virt = Fdt::new(VIRT).unwrap();
    let pci = virt.find_compatible(&["pci-host-ecam-generic"]).next().unwrap();
    assert_eq!(pci.child_dma_reach().unwrap(), nexus_fdt::DmaReach::All);
    assert!(pci.dma_coherent());
    assert_eq!(pci.cpu_address(0x4000_0000).unwrap(), 0x4000_0000);
    // On the board a bus's children reach what the bus's own ranges say: the same answer
    // a device node on it gets.
    let board = Fdt::new(BOARD).unwrap();
    let storage = board.node_at_path("/soc/storage-bus").unwrap();
    let emmc = board.node_at_path("/soc/storage-bus/mmc@d4281000").unwrap();
    assert_eq!(storage.child_dma_reach().unwrap(), emmc.dma_reach().unwrap());
    assert_eq!(storage.child_dma_reach().unwrap().windows(), [window(0, 0, 0x8000_0000)]);
}

#[test]
fn coherence_is_the_default_and_the_board_soc_bus_marks_its_masters_non_coherent() {
    // QEMU virt's tree carries neither property: every virtio master is coherent.
    let virt = Fdt::new(VIRT).unwrap();
    assert!(virt.find_compatible(&["virtio,mmio"]).all(|n| n.dma_coherent()));
    // The board's `soc` bus carries `dma-noncoherent` (measured R4: swiotlb, the
    // stock kernel bounces; the mainline K1 tree marks the bus): every master on
    // it inherits the property.
    let board = Fdt::new(BOARD).unwrap();
    const MASTERS: [&[&str]; 5] = [
        &["spacemit,k1-sdhci"],
        &["spacemit,dpu-online2"],
        &["snps,dwc3"],
        &["spacemit,k1-emac"],
        &["img,rgx"],
    ];
    for compatible in MASTERS {
        let masters: Vec<_> = board.find_compatible(compatible).collect();
        assert!(!masters.is_empty(), "{compatible:?} in the board tree");
        assert!(masters.iter().all(|n| !n.dma_coherent()), "{compatible:?} is non-coherent");
    }
    // Outside the bus nothing is marked: the cpus node stays coherent.
    assert!(board.node_at_path("/cpus").unwrap().dma_coherent());
}

#[test]
fn nxboot_writes_chosen_in_headroom_and_the_kernel_reads_it_back() {
    let mut buf = BOARD.to_vec();
    let mut w = ChosenWriter::new(&mut buf).unwrap();
    assert!(w.headroom() >= 400, "dtc -p 512 headroom expected, got {}", w.headroom());

    w.set_nexus_str("boot-profile", "headless").unwrap();
    w.set_nexus_str("boot-slot", "a").unwrap();
    w.set_nexus_u64("boot-count", 0x8000_1234).unwrap();
    // The measured boot record travels as bytes (ADR-0059 v1: 60 bytes incl. CRC).
    let record: [u8; 60] = core::array::from_fn(|i| i as u8 ^ 0x5a);
    w.set_nexus_bytes("boot-record", &record).unwrap();
    // Overwrite with a different length: remove + insert.
    w.set_nexus_str("boot-profile", "visible").unwrap();
    // Same length: in-place.
    w.set_nexus_str("boot-slot", "b").unwrap();

    let fdt = Fdt::new(&buf).unwrap();
    let chosen = fdt.chosen().unwrap();
    assert_eq!(chosen.nexus_str("boot-profile"), Some("visible"));
    assert_eq!(chosen.nexus_str("boot-slot"), Some("b"));
    assert_eq!(chosen.nexus_u64("boot-count"), Some(0x8000_1234));
    assert_eq!(chosen.nexus_bytes("boot-record"), Some(&record[..]));
    assert_eq!(chosen.nexus_bytes("boot-count").map(<[u8]>::len), Some(8));
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

/// RFC-0106: consumers bind to providers the standard way; the ids are the
/// binding header's (config/board/include/dt-bindings), the providers the syscons.
#[test]
fn consumers_resolve_their_clocks_resets_domains_and_pads_to_providers() {
    let board = Fdt::new(BOARD).unwrap();
    let emmc = board.node_at_path("/soc/storage-bus/mmc@d4281000").unwrap();
    let clocks: Vec<_> = emmc.specifiers("clocks", "#clock-cells").collect();
    assert_eq!(clocks.len(), 2);
    assert_eq!(clocks[0].provider.name(), "syscon@d4282800");
    assert_eq!((clocks[0].args(), clocks[0].arg(0)), (1, Some(10)), "CLK_SDH_AXI");
    assert_eq!(clocks[1].arg(0), Some(13), "CLK_SDH2");
    let io = emmc.specifier_named("clocks", "#clock-cells", "clock-names", "io").unwrap();
    assert_eq!(io.arg(0), Some(13));
    let resets: Vec<_> = emmc.specifiers("resets", "#reset-cells").map(|s| s.arg(0)).collect();
    assert_eq!(resets, vec![Some(2), Some(5)], "RESET_SDH_AXI, RESET_SDH2");
    let pd = emmc.specifiers("power-domains", "#power-domain-cells").next().unwrap();
    assert_eq!((pd.provider.name(), pd.arg(0)), ("syscon@d4282800", Some(0)), "K1_PD_BUS");
    // A pad group is a phandle with no cells.
    let sd = board.node_at_path("/soc/storage-bus/mmc@d4280000").unwrap();
    let pads: Vec<_> = sd.specifiers("pinctrl-0", "#pinctrl-cells").collect();
    assert_eq!(pads.len(), 1);
    assert_eq!((pads[0].provider.name(), pads[0].args()), ("mmc1-cfg", 0));
    assert!(emmc.specifiers("pinctrl-0", "#pinctrl-cells").next().is_none(), "eMMC pads are fixed");
    // The display controller names what the stock tree names — hmclk alone, at the rate it
    // was measured running (assigned-clock-rates, index-aligned) — and domain 7; the GPU its
    // domain.
    let dpu = board.find_compatible(&["spacemit,dpu-online2"]).next().unwrap();
    let dpu_clocks: Vec<_> = dpu.specifiers("clocks", "#clock-cells").map(|s| s.arg(0)).collect();
    assert_eq!(dpu_clocks, vec![Some(26)], "CLK_HDMI");
    let assigned = dpu.specifiers("assigned-clocks", "#clock-cells").next().unwrap();
    assert_eq!(assigned.arg(0), Some(26));
    assert_eq!(dpu.prop_cell("assigned-clock-rates", 0), Some(491_520_000));
    let dpu_pd = dpu.specifiers("power-domains", "#power-domain-cells").next().unwrap();
    assert_eq!(dpu_pd.arg(0), Some(7), "K1_PD_HDMI");
    let gpu = board.find_compatible(&["img,rgx"]).next().unwrap();
    let gpu_pd = gpu.specifiers("power-domains", "#power-domain-cells").next().unwrap();
    assert_eq!(gpu_pd.arg(0), Some(2), "K1_PD_GPU");
    // QEMU virt binds nothing.
    let virt = Fdt::new(VIRT).unwrap();
    let blk = virt.find_compatible(&["virtio,mmio"]).next().unwrap();
    assert!(blk.specifiers("clocks", "#clock-cells").next().is_none());
}

#[test]
fn every_node_names_its_own_path_and_the_path_finds_it_again() {
    for tree in [VIRT, BOARD] {
        let fdt = Fdt::new(tree).unwrap();
        let mut seen = 0;
        for node in fdt.all_nodes() {
            let mut buf = [0u8; 128];
            let path = node.path_into(&mut buf).unwrap();
            let found = fdt.node_at_path(path).unwrap();
            assert_eq!(found.name(), node.name(), "{path}");
            assert_eq!(found.reg(0).ok().flatten(), node.reg(0).ok().flatten(), "{path}");
            seen += 1;
        }
        assert!(seen > 20);
    }
    let virt = Fdt::new(VIRT).unwrap();
    let mut buf = [0u8; 64];
    assert_eq!(virt.root().unwrap().path_into(&mut buf), Some("/"));
    let blk = virt.node_at_path("/soc/virtio_mmio@10008000").unwrap();
    assert_eq!(blk.path_into(&mut buf), Some("/soc/virtio_mmio@10008000"));
    let board = Fdt::new(BOARD).unwrap();
    let emmc = board.find_compatible(&["spacemit,k1-sdhci"]).last().unwrap();
    assert_eq!(emmc.path_into(&mut buf), Some("/soc/storage-bus/mmc@d4281000"));
}

/// TASK-0260B P3: a cpu node with `status = "disabled"` is not a hart the OS may start —
/// `harts()` skips it, the count drops, and the enabled ones keep their ids (the canonical
/// way a board tree pins a boot to fewer harts than the SoC has).
#[test]
fn a_disabled_cpu_node_is_not_a_hart() {
    const DISABLED: &[u8] = include_bytes!("goldens/cpus-disabled.dtb");
    let cpus = Fdt::new(DISABLED).unwrap().cpus().unwrap();
    assert_eq!(cpus.timebase_hz, 24_000_000);
    assert_eq!(cpus.count(), 2);
    let harts: Vec<u32> = cpus.harts().map(|c| c.hart).collect();
    assert_eq!(harts, [0, 2], "hart 1 is disabled, hart 2 has no status (enabled)");
    assert!(cpus.harts().all(|c| c.is_enabled() && c.has_extension("sstc")));
    // The node itself is still in the tree (a cpu-map may name it).
    let fdt = Fdt::new(DISABLED).unwrap();
    assert!(fdt.node_at_path("/cpus/cpu@1").is_some());
    // The real trees enable every hart they list: nothing changes for them.
    assert_eq!(Fdt::new(VIRT).unwrap().cpus().unwrap().count(), 4);
    assert_eq!(Fdt::new(BOARD).unwrap().cpus().unwrap().count(), 8);
}

#[test]
fn test_reject_a_path_that_does_not_fit() {
    let virt = Fdt::new(VIRT).unwrap();
    let blk = virt.node_at_path("/soc/virtio_mmio@10008000").unwrap();
    // 25 bytes: every shorter buffer is refused whole, never truncated.
    for len in 0..25 {
        let mut buf = vec![0u8; len];
        assert_eq!(blk.path_into(&mut buf), None, "len={len}");
    }
    let mut buf = [0u8; 25];
    assert_eq!(blk.path_into(&mut buf), Some("/soc/virtio_mmio@10008000"));
    let mut empty = [0u8; 0];
    assert_eq!(virt.root().unwrap().path_into(&mut empty), None);
}
