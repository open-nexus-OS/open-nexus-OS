// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the boot-disk record (RFC-0098 C5, TASK-0246 P4b) on the two trees a boot is
//! handed — QEMU virt and the board — and a fixture for what they cannot show (a disabled
//! disk, a disabled ECAM host): every kind resolves from the record the loader writes, every
//! record the writer produces resolves back to the same device, and every malformed record
//! is refused by name.
//! OWNERS: @runtime

use nexus_fdt::{ChosenWriter, Fdt};
use storage::boot_disk::{self, BootDisk, Kind, Place, Reject, CHOSEN_KEY, MAX_RECORD};

const VIRT: &[u8] = include_bytes!("../../../source/libs/nexus-fdt/tests/goldens/virt.dtb");
const BOARD: &[u8] = include_bytes!("../../../source/libs/nexus-fdt/tests/goldens/bpi-f3.dtb");
const FIXTURE: &[u8] = include_bytes!("fixtures/boot-disk.dtb");

/// `tree` with `/chosen/nexus,boot-disk` set to `raw` (the loader's bytes, terminator included).
fn with_record(tree: &[u8], raw: &[u8]) -> Vec<u8> {
    let mut buf = tree.to_vec();
    buf.resize(buf.len() + 1024, 0);
    ChosenWriter::new(&mut buf).unwrap().set_nexus_bytes(CHOSEN_KEY, raw).unwrap();
    buf
}

fn resolve(tree: &[u8], record: &str) -> Result<(Kind, String), Reject> {
    let mut raw = record.as_bytes().to_vec();
    raw.push(0);
    let buf = with_record(tree, &raw);
    let fdt = Fdt::new(&buf).unwrap();
    let disk = boot_disk::resolve(&fdt)?.expect("a record");
    Ok((disk.kind, describe(&disk)))
}

fn describe(disk: &BootDisk<'_>) -> String {
    let mut buf = [0u8; 128];
    match disk.place {
        Place::Node(node) => node.path_into(&mut buf).unwrap().to_string(),
        Place::Pci { host, dev, func } => {
            format!("{} {dev},{func}", host.path_into(&mut buf).unwrap())
        }
    }
}

#[test]
fn every_kind_resolves_from_its_record() {
    assert_eq!(
        resolve(VIRT, "/soc/virtio_mmio@10008000"),
        Ok((Kind::VirtioBlk, "/soc/virtio_mmio@10008000".into()))
    );
    assert_eq!(
        resolve(VIRT, "/soc/pci@30000000/mmc@1,0"),
        Ok((Kind::SdhciPci, "/soc/pci@30000000 1,0".into()))
    );
    assert_eq!(
        resolve(BOARD, "/soc/storage-bus/mmc@d4281000"),
        Ok((Kind::SdhciK1, "/soc/storage-bus/mmc@d4281000".into()))
    );
    assert_eq!(Kind::VirtioBlk.policy_class(), "device.mmio.blk");
    assert_eq!(Kind::SdhciK1.policy_class(), "device.mmio.mmc");
    assert_eq!(Kind::SdhciPci.policy_class(), "device.mmio.mmc");
}

#[test]
fn a_tree_without_a_record_has_no_boot_disk() {
    for tree in [VIRT, BOARD, FIXTURE] {
        assert!(boot_disk::resolve(&Fdt::new(tree).unwrap()).unwrap().is_none());
    }
}

#[test]
fn every_record_the_writer_produces_resolves_to_the_same_device() {
    let virt = Fdt::new(VIRT).unwrap();
    let mut out = [0u8; MAX_RECORD];
    for node in virt.find_compatible(&["virtio,mmio"]) {
        let record = boot_disk::record_for_node(&node, &mut out).unwrap().to_string();
        let disk = boot_disk::parse(&virt, &record).unwrap();
        let Place::Node(found) = disk.place else { panic!("{record}") };
        assert_eq!(found.reg(0).unwrap(), node.reg(0).unwrap(), "{record}");
        assert_eq!(disk.kind, Kind::VirtioBlk);
    }
    let host = virt.find_compatible(&["pci-host-ecam-generic"]).next().unwrap();
    for dev in 0..32u8 {
        for func in 0..8u8 {
            let record =
                boot_disk::record_for_pci_sd_host(&host, dev, func, &mut out).unwrap().to_string();
            let disk = boot_disk::parse(&virt, &record).unwrap();
            let Place::Pci { dev: d, func: f, .. } = disk.place else { panic!("{record}") };
            assert_eq!((d, f, disk.kind), (dev, func, Kind::SdhciPci), "{record}");
        }
    }
    // On the board the writer records only a host that may hold an eMMC (no `no-mmc`): one.
    let board = Fdt::new(BOARD).unwrap();
    let emmc_hosts: Vec<_> = board
        .find_compatible(&["spacemit,k1-sdhci"])
        .filter(|node| node.prop("no-mmc").is_none())
        .collect();
    assert_eq!(emmc_hosts.len(), 1);
    for node in emmc_hosts {
        let record = boot_disk::record_for_node(&node, &mut out).unwrap().to_string();
        assert_eq!(boot_disk::parse(&board, &record).unwrap().kind, Kind::SdhciK1, "{record}");
    }
}

#[test]
fn the_owner_finds_its_disk_by_window_when_no_record_exists() {
    let virt = Fdt::new(VIRT).unwrap();
    let disk = boot_disk::node_at_window(&virt, 0x1000_8000).unwrap();
    assert_eq!(
        (disk.kind, describe(&disk).as_str()),
        (Kind::VirtioBlk, "/soc/virtio_mmio@10008000")
    );
    let board = Fdt::new(BOARD).unwrap();
    let disk = boot_disk::node_at_window(&board, 0xd428_1000).unwrap();
    assert_eq!(disk.kind, Kind::SdhciK1);
    // Not a disk's window: the UART, a PCI BAR, nothing at all.
    assert!(boot_disk::node_at_window(&virt, 0x1000_0000).is_none());
    assert!(boot_disk::node_at_window(&virt, 0x4000_0000).is_none());
    assert!(boot_disk::node_at_window(&board, 0).is_none());
    // A disabled disk is not found either.
    assert!(boot_disk::node_at_window(&Fdt::new(FIXTURE).unwrap(), 0x1000_1000).is_none());
}

/// The board's microSD slot and SDIO function carry `no-mmc` (measured 2026-09-26): neither is
/// a disk, whether a record names it or the owner looks it up by its window.
#[test]
fn test_reject_a_record_naming_a_host_that_holds_no_emmc() {
    for host in ["/soc/storage-bus/mmc@d4280000", "/soc/storage-bus/mmc@d4280800"] {
        assert_eq!(resolve(BOARD, host).err(), Some(Reject::NotADisk), "{host}");
    }
    let board = Fdt::new(BOARD).unwrap();
    for window in [0xd428_0000, 0xd428_0800] {
        assert!(boot_disk::node_at_window(&board, window).is_none(), "{window:#x}");
    }
    assert_eq!(boot_disk::node_at_window(&board, 0xd428_1000).unwrap().kind, Kind::SdhciK1);
}

#[test]
fn test_reject_malformed_records() {
    let long = format!("/soc/{}", "a".repeat(MAX_RECORD));
    let cases: &[(&[u8], &str, Reject)] = &[
        (VIRT, "", Reject::Empty),
        (VIRT, &long, Reject::TooLong),
        (VIRT, "soc/virtio_mmio@10008000", Reject::NotAbsolute),
        (VIRT, "virtio0", Reject::NotAbsolute),
        (VIRT, "/", Reject::Syntax),
        (VIRT, "/soc/", Reject::Syntax),
        (VIRT, "/soc//virtio_mmio@10008000", Reject::Syntax),
        (VIRT, "/soc/virtio_mmio@10008000:opts", Reject::Syntax),
        (VIRT, "/soc/virtio_mmio@10008001", Reject::NoSuchNode),
        (VIRT, "/nope/mmc@1,0", Reject::NoSuchNode),
        (VIRT, "/soc/virtio_mmio@10008000/mmc@1,0", Reject::NoSuchNode),
        (VIRT, "/soc", Reject::NotADisk),
        (VIRT, "/cpus/cpu@0", Reject::NotADisk),
        (VIRT, "/soc/serial@10000000", Reject::NotADisk),
        (VIRT, "/soc/pci@30000000/nvme@0,0", Reject::NotADisk),
        (VIRT, "/soc/pci@30000000/mmc", Reject::PciAddress),
        (VIRT, "/soc/pci@30000000/mmc@1", Reject::PciAddress),
        (VIRT, "/soc/pci@30000000/mmc@01,0", Reject::PciAddress),
        (VIRT, "/soc/pci@30000000/mmc@1F,0", Reject::PciAddress),
        (VIRT, "/soc/pci@30000000/mmc@20,0", Reject::PciAddress),
        (VIRT, "/soc/pci@30000000/mmc@1,8", Reject::PciAddress),
        (VIRT, "/soc/pci@30000000/mmc@1,0,0", Reject::PciAddress),
        (FIXTURE, "/soc/virtio_mmio@10001000", Reject::Disabled),
        (FIXTURE, "/soc/pci@30000000/mmc@1,0", Reject::Disabled),
    ];
    for (tree, record, want) in cases {
        assert_eq!(resolve(tree, record).err(), Some(*want), "{record:?}");
    }
    // The bytes themselves: no terminator, a NUL inside, not UTF-8.
    for raw in [&b"/soc/virtio_mmio@10008000"[..], b"/soc\0/x\0", b"/soc/\xff\0"] {
        let buf = with_record(VIRT, raw);
        let got = boot_disk::resolve(&Fdt::new(&buf).unwrap()).err();
        assert_eq!(got, Some(Reject::Encoding), "{raw:?}");
    }
}

#[test]
fn test_reject_a_record_the_writer_cannot_spell() {
    let virt = Fdt::new(VIRT).unwrap();
    let host = virt.find_compatible(&["pci-host-ecam-generic"]).next().unwrap();
    let mut out = [0u8; MAX_RECORD];
    assert!(boot_disk::record_for_pci_sd_host(&host, 32, 0, &mut out).is_none());
    assert!(boot_disk::record_for_pci_sd_host(&host, 0, 8, &mut out).is_none());
    // A buffer too small is refused whole.
    let mut small = [0u8; 20];
    assert!(boot_disk::record_for_pci_sd_host(&host, 1, 0, &mut small).is_none());
    let node = virt.node_at_path("/soc/virtio_mmio@10008000").unwrap();
    assert!(boot_disk::record_for_node(&node, &mut small).is_none());
}
