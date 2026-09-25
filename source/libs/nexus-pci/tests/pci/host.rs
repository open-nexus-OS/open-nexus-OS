// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the host as the tree describes it.
//! OWNERS: @runtime @drivers

use nexus_fdt::{DmaReach, Fdt};
use nexus_pci::{Bdf, HostError, PciHost, Window, WindowKind};

pub const VIRT: &[u8] = include_bytes!("../../../nexus-fdt/tests/goldens/virt.dtb");
const BOARD: &[u8] = include_bytes!("../../../nexus-fdt/tests/goldens/bpi-f3.dtb");
const HOSTS: &[u8] = include_bytes!("../goldens/pci-hosts.dtb");

pub fn virt_host() -> PciHost {
    let fdt = Fdt::new(VIRT).unwrap();
    let node = fdt.find_compatible(&["pci-host-ecam-generic"]).next().unwrap();
    PciHost::from_node(&fdt, node).unwrap()
}

fn fixture(tag: &str) -> Result<PciHost, HostError> {
    let fdt = Fdt::new(HOSTS).unwrap();
    let node = fdt.all_nodes().find(|n| n.is_compatible(tag)).unwrap();
    PciHost::from_node(&fdt, node)
}

pub fn translated_host() -> PciHost {
    fixture("test,translated").unwrap()
}

fn bdf(dev: u8, func: u8) -> Bdf {
    Bdf { bus: 0, dev, func }
}

#[test]
fn qemu_virts_host_is_its_ecam_three_windows_coherence_and_routes() {
    let host = virt_host();
    assert_eq!((host.ecam, host.first_bus, host.last_bus), (0x3000_0000, 0, 255));
    assert_eq!(host.root_config(), (0x3000_0000, 1 << 20));
    let window = |kind, pci, cpu, size| Window { kind, prefetchable: false, pci, cpu, size };
    assert_eq!(
        host.windows(),
        [
            window(WindowKind::Io, 0, 0x0300_0000, 0x1_0000),
            window(WindowKind::Mem32, 0x4000_0000, 0x4000_0000, 0x4000_0000),
            window(WindowKind::Mem64, 0x4_0000_0000, 0x4_0000_0000, 0x4_0000_0000),
        ]
    );
    assert!(host.coherent);
    assert_eq!(host.reach, DmaReach::All);
}

#[test]
fn intx_routes_follow_the_swizzled_interrupt_map() {
    let host = virt_host();
    // Slot 1 INTA — the SD host QEMU puts at 00:01.0 — is PLIC line 33 (measured, P0).
    assert_eq!(host.route(bdf(1, 0), 1), Some(33));
    assert_eq!(host.route(bdf(0, 0), 1), Some(32));
    assert_eq!(host.route(bdf(2, 0), 2), Some(35));
    assert_eq!(host.route(bdf(3, 0), 4), Some(34));
    // The mask keeps the slot's low two bits: slot 5 routes like slot 1; the function
    // number is masked away.
    assert_eq!(host.route(bdf(5, 3), 1), Some(33));
    // No pin, or a pin that does not exist.
    assert_eq!(host.route(bdf(1, 0), 0), None);
    assert_eq!(host.route(bdf(1, 0), 5), None);
}

#[test]
fn the_boards_tree_has_no_generic_ecam_host() {
    let board = Fdt::new(BOARD).unwrap();
    assert!(board.find_compatible(&["pci-host-ecam-generic"]).next().is_none());
}

#[test]
fn a_host_behind_a_translating_bus_answers_in_cpu_addresses() {
    let host = translated_host();
    assert_eq!(host.ecam, 0xa000_0000);
    let mem = host.window(WindowKind::Mem32).unwrap();
    assert_eq!((mem.pci, mem.cpu, mem.size), (0x100_0000, 0xa100_0000, 0x100_0000));
    // No interrupt map: no pin routes anywhere.
    assert_eq!(host.route(bdf(1, 0), 1), None);
}

#[test]
fn test_reject_malformed_hosts_by_name() {
    assert_eq!(fixture("test,bad-cells"), Err(HostError::Binding));
    assert_eq!(fixture("test,no-reg"), Err(HostError::Ecam));
    assert_eq!(fixture("test,small-ecam"), Err(HostError::BusRange));
    assert_eq!(fixture("test,config-window"), Err(HostError::Ranges));
    assert_eq!(fixture("test,short-map"), Err(HostError::InterruptMap));
    assert_eq!(fixture("test,orphan-map"), Err(HostError::InterruptParent));
    assert_eq!(fixture("test,parent-without-cells"), Err(HostError::InterruptParent));
    assert_eq!(fixture("test,mem32-above-4g"), Err(HostError::Ranges));
}
