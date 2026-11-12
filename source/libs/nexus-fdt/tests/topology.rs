// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the logical CPUs of the two trees a kernel is handed (RFC-0118): the reference
//! board (8 harts in 2 clusters of 4) and QEMU `virt` (4 harts, no `cpu-map`). CPU 0 is the
//! boot hart whatever its hart id; a hart's PLIC context is read, never computed.

use nexus_fdt::{Error, Fdt, LogicalCpu, Topology};

const VIRT: &[u8] = include_bytes!("goldens/virt.dtb");
const BOARD: &[u8] = include_bytes!("goldens/bpi-f3.dtb");

/// The ceiling a kernel reads with (the width of its CPU set).
const BITS: usize = 64;

/// The forms the two real trees do not use (`goldens/cpus-topology.dts`).
const FORMS: &[u8] = include_bytes!("goldens/cpus-topology.dtb");

fn cpu(hart: u64, cluster: u32, context: u32) -> LogicalCpu {
    LogicalCpu { hart, cluster, plic_context: Some(context) }
}

#[test]
fn a_machine_never_seen_is_read_by_the_same_rules() {
    let fdt = Fdt::new(FORMS).unwrap();
    // No timebase on `/cpus`: the cpu nodes state it.
    assert_eq!(fdt.cpus().unwrap().timebase_hz, 10_000_000);
    // Six enabled cpu nodes; the monitor core and the `riscv,none` one have no MMU.
    let topo = Topology::<BITS>::read(&fdt, 0x200).unwrap();
    assert_eq!((topo.named(), topo.runnable(), topo.count(), topo.clusters()), (6, 4, 4, 3));
    let cpus: Vec<_> = topo.cpus().collect();
    assert_eq!(
        cpus,
        [
            // The boot hart first; then the tree's order. Hart ids are two cells wide.
            cpu(0x200, 1, 3),
            cpu(0x100, 0, 5),
            cpu(0x101, 0, 6),
            cpu(0x1_0000_0000, 2, 1),
        ]
    );
    // Two threads of one core share the innermost cluster; the socket above is not one.
    assert_eq!(topo.cluster_bits(0), Some(0b0110));
    assert_eq!(topo.cluster_bits(1), Some(0b0001));
    assert_eq!(topo.cluster_bits(2), Some(0b1000));
    assert_eq!(topo.cluster_bits(3), None);
    assert_eq!(topo.cpu_of(0x1_0000_0000), Some(3));
    // The monitor core is no CPU, the disabled one neither.
    assert_eq!((topo.cpu_of(0), topo.cpu_of(0x300), topo.cpu_of(0x400)), (None, None, None));
}

#[test]
fn test_reject_a_boot_hart_without_an_mmu() {
    let fdt = Fdt::new(FORMS).unwrap();
    assert_eq!(Topology::<BITS>::read(&fdt, 0).err(), Some(Error::NotFound));
    assert_eq!(Topology::<BITS>::read(&fdt, 0x400).err(), Some(Error::NotFound));
    assert_eq!(Topology::<BITS>::read(&fdt, 0x300).err(), Some(Error::NotFound), "disabled");
}

#[test]
fn the_board_is_eight_cpus_in_two_clusters_of_four() {
    let topo = Topology::<BITS>::read(&Fdt::new(BOARD).unwrap(), 0).unwrap();
    assert_eq!((topo.count(), topo.named(), topo.clusters()), (8, 8, 2));
    let cpus: Vec<_> = topo.cpus().collect();
    assert_eq!(
        cpus,
        [
            cpu(0, 0, 1),
            cpu(1, 0, 3),
            cpu(2, 0, 5),
            cpu(3, 0, 7),
            cpu(4, 1, 9),
            cpu(5, 1, 11),
            cpu(6, 1, 13),
            cpu(7, 1, 15),
        ]
    );
    assert_eq!(topo.cluster_bits(0), Some(0x0f));
    assert_eq!(topo.cluster_bits(1), Some(0xf0));
}

#[test]
fn virt_is_four_cpus_in_the_one_cluster_a_tree_without_a_map_has() {
    let topo = Topology::<BITS>::read(&Fdt::new(VIRT).unwrap(), 0).unwrap();
    assert_eq!((topo.count(), topo.named(), topo.clusters()), (4, 4, 1));
    let cpus: Vec<_> = topo.cpus().collect();
    assert_eq!(cpus, [cpu(0, 0, 1), cpu(1, 0, 3), cpu(2, 0, 5), cpu(3, 0, 7)]);
    assert_eq!(topo.cluster_bits(0), Some(0x0f));
}

#[test]
fn cpu_zero_is_the_boot_hart_whatever_its_hart_id() {
    let topo = Topology::<BITS>::read(&Fdt::new(BOARD).unwrap(), 5).unwrap();
    let harts: Vec<u64> = topo.cpus().map(|c| c.hart).collect();
    assert_eq!(harts, [5, 0, 1, 2, 3, 4, 6, 7]);
    assert_eq!((topo.hart_of(0), topo.cpu_of(5), topo.cpu_of(0)), (Some(5), Some(0), Some(1)));
    // The boot hart keeps its own cluster and its own PLIC context.
    assert_eq!(topo.cpu(0), Some(cpu(5, 1, 11)));
    // Cluster 1 = harts 4..7 = CPUs 0 (hart 5), 5 (hart 4), 6, 7.
    assert_eq!(topo.cluster_bits(1), Some(0b1110_0001));
    assert_eq!(topo.cluster_bits(0), Some(0b0001_1110));
}

#[test]
fn a_tree_naming_more_harts_than_the_ceiling_yields_the_first_and_says_how_many() {
    let topo = Topology::<4>::read(&Fdt::new(BOARD).unwrap(), 6).unwrap();
    assert_eq!((topo.count(), topo.named()), (4, 8));
    let harts: Vec<u64> = topo.cpus().map(|c| c.hart).collect();
    assert_eq!(harts, [6, 0, 1, 2]);
    assert_eq!(topo.cpu_of(7), None);
}

#[test]
fn test_reject_a_boot_hart_the_tree_does_not_name() {
    let fdt = Fdt::new(BOARD).unwrap();
    assert_eq!(Topology::<BITS>::read(&fdt, 8).err(), Some(Error::NotFound));
}

#[test]
fn test_reject_topology_index_beyond_count() {
    let topo = Topology::<BITS>::read(&Fdt::new(VIRT).unwrap(), 0).unwrap();
    assert_eq!(topo.cpu(4), None);
    assert_eq!(topo.hart_of(4), None);
    assert_eq!(topo.cluster_bits(1), None);
    assert_eq!(topo.cpu_of(4), None);
}

#[test]
fn a_machine_without_a_readable_tree_is_its_boot_hart_alone() {
    let topo = Topology::<BITS>::single(3);
    assert_eq!((topo.count(), topo.named(), topo.clusters()), (1, 1, 1));
    assert_eq!(topo.cpu(0), Some(LogicalCpu { hart: 3, cluster: 0, plic_context: None }));
    assert_eq!(topo.cluster_bits(0), Some(1));
}
