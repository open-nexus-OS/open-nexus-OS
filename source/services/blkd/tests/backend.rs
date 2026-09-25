// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: which backend the block owner runs (TASK-0246 P4b) — on QEMU virt's and the
//! board's trees with the loader's record written in: every kind selects its backend when the
//! granted window is the recorded disk's, a grant that is not refuses, a record that does not
//! resolve refuses by name, and without a record only a disk node at the window stands in.
//! OWNERS: @runtime

use blkd::backend::{self, Backend, Refusal};
use nexus_fdt::{ChosenWriter, Fdt};
use storage::boot_disk::{Kind, Reject, CHOSEN_KEY};
use storage_sdhci::Layer;

const VIRT: &[u8] = include_bytes!("../../../libs/nexus-fdt/tests/goldens/virt.dtb");
const BOARD: &[u8] = include_bytes!("../../../libs/nexus-fdt/tests/goldens/bpi-f3.dtb");
const FIXTURE: &[u8] = include_bytes!("../../../../userspace/storage/tests/fixtures/boot-disk.dtb");

fn select(tree: &[u8], record: Option<&str>, window: u64) -> Result<backend::Selected, Refusal> {
    let mut buf = tree.to_vec();
    buf.resize(buf.len() + 1024, 0);
    if let Some(record) = record {
        ChosenWriter::new(&mut buf).unwrap().set_nexus_str(CHOSEN_KEY, record).unwrap();
    }
    backend::select(&Fdt::new(&buf).unwrap(), window)
}

#[test]
fn every_kind_selects_its_backend() {
    let virtio = select(VIRT, Some("/soc/virtio_mmio@10008000"), 0x1000_8000).unwrap();
    assert_eq!(
        (virtio.kind, virtio.backend, virtio.recorded),
        (Kind::VirtioBlk, Backend::VirtioBlk, true)
    );
    // QEMU's SD host behind PCI: its BAR sits in the host's 32-bit memory window.
    assert_eq!(
        virtio.node.map(|n| n.as_str().to_string()).as_deref(),
        Some("/soc/virtio_mmio@10008000")
    );
    let pci = select(VIRT, Some("/soc/pci@30000000/mmc@1,0"), 0x4000_0000).unwrap();
    let Backend::Sdhci(config) = pci.backend else { panic!("{pci:?}") };
    assert_eq!((pci.kind, config.layer, config.bus_width), (Kind::SdhciPci, Layer::Standard, 8));
    // A function behind PCI has no node of its own to bring up.
    assert_eq!(pci.node, None);
    // The board's eMMC: the K1 layer, its node's 8 bits.
    let emmc = select(BOARD, Some("/soc/storage-bus/mmc@d4281000"), 0xd428_1000).unwrap();
    let Backend::Sdhci(config) = emmc.backend else { panic!("{emmc:?}") };
    assert_eq!((emmc.kind, config.layer, config.bus_width), (Kind::SdhciK1, Layer::K1, 8));
    assert_eq!(
        emmc.node.map(|n| n.as_str().to_string()).as_deref(),
        Some("/soc/storage-bus/mmc@d4281000")
    );
}

#[test]
fn test_reject_a_grant_that_is_not_the_recorded_disk() {
    // Another transport, a window outside the host's memory, another SD host of the board.
    assert_eq!(
        select(VIRT, Some("/soc/virtio_mmio@10008000"), 0x1000_7000),
        Err(Refusal::WindowMismatch)
    );
    assert_eq!(
        select(VIRT, Some("/soc/pci@30000000/mmc@1,0"), 0x1000_8000),
        Err(Refusal::WindowMismatch)
    );
    assert_eq!(
        select(BOARD, Some("/soc/storage-bus/mmc@d4281000"), 0xd428_0000),
        Err(Refusal::WindowMismatch)
    );
    // The host's I/O window forwards ports, not a BAR; the 32-bit memory window ends at 2 GiB.
    let pci = Some("/soc/pci@30000000/mmc@1,0");
    assert_eq!(select(VIRT, pci, 0x0300_0000), Err(Refusal::WindowMismatch));
    assert_eq!(select(VIRT, pci, 0x8000_0000), Err(Refusal::WindowMismatch));
    assert!(select(VIRT, pci, 0x7fff_f000).is_ok());
}

#[test]
fn test_reject_a_record_that_does_not_resolve() {
    assert_eq!(
        select(VIRT, Some("/soc/virtio_mmio@10008001"), 0x1000_8000),
        Err(Refusal::Record(Reject::NoSuchNode))
    );
    assert_eq!(
        select(VIRT, Some("/soc/pci@30000000/nvme@0,0"), 0x4000_0000),
        Err(Refusal::Record(Reject::NotADisk))
    );
    assert_eq!(Refusal::Record(Reject::NotADisk).name(), "not-a-disk");
}

#[test]
fn test_reject_an_sd_host_its_node_cannot_configure() {
    assert_eq!(select(FIXTURE, Some("/soc/mmc@d4283000"), 0xd428_3000), Err(Refusal::HostConfig));
}

#[test]
fn without_a_record_only_a_disk_node_at_the_window_stands_in() {
    let blk = select(VIRT, None, 0x1000_8000).unwrap();
    assert_eq!((blk.kind, blk.backend, blk.recorded), (Kind::VirtioBlk, Backend::VirtioBlk, false));
    assert_eq!(
        blk.node.map(|n| n.as_str().to_string()).as_deref(),
        Some("/soc/virtio_mmio@10008000")
    );
    // A PCI function has no node: without a record nothing names it.
    assert_eq!(select(VIRT, None, 0x4000_0000), Err(Refusal::NoDisk));
    assert_eq!(select(VIRT, None, 0x1000_0000), Err(Refusal::NoDisk));
}
