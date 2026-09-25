// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the SDHCI backend's block face (TASK-0246 P4b) over the behavioural controller +
//! eMMC the core itself is proven against (`storage-sdhci-model`): runs and single sectors
//! arrive where they were written, every range the card cannot hold is refused before the
//! core sees it, a device error is an I/O error and the disk serves again after it; and the
//! host configuration the boot disk's tree node gives.
//! OWNERS: @runtime

use nexus_fdt::{ChosenWriter, Fdt};
use storage::boot_disk::{self, CHOSEN_KEY};
use storage::sdhci::{host_config, SdhciDevice, SECTOR};
use storage::{BlockDevice, BlockError};
use storage_sdhci::{Ceiling, Layer};
use storage_sdhci_model as model;

const VIRT: &[u8] = include_bytes!("../../../source/libs/nexus-fdt/tests/goldens/virt.dtb");
const BOARD: &[u8] = include_bytes!("../../../source/libs/nexus-fdt/tests/goldens/bpi-f3.dtb");
const FIXTURE: &[u8] = include_bytes!("fixtures/boot-disk.dtb");

type Device =
    SdhciDevice<model::ModelBus, model::SimPlatform, model::mem::ModelMem, model::mem::ModelCache>;

fn device(config: model::Config) -> (model::Shared, Device) {
    let m = model::machine(config);
    let disk = model::disk(&m, model::card(&m, Ceiling::Hs400es));
    (m, SdhciDevice::new(disk))
}

fn pattern(seed: u8, len: usize) -> Vec<u8> {
    (0..len).map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed)).collect()
}

#[test]
fn runs_and_sectors_arrive_where_they_were_written() {
    for config in [model::qemu_8bit(), model::k1()] {
        let (_m, mut dev) = device(config);
        assert_eq!(dev.block_size(), SECTOR);
        let sectors = dev.block_count();
        assert!(sectors > 0);
        // A run larger than one bounce-buffer chunk (64 KiB), at an odd start.
        let run = pattern(7, 300 * SECTOR);
        dev.write_blocks(1001, &run).unwrap();
        let mut back = vec![0u8; run.len()];
        dev.read_blocks(1001, &mut back).unwrap();
        assert_eq!(back, run);
        // Single sectors, the last one of the card included; a longer buffer uses its head.
        let one = pattern(9, SECTOR);
        dev.write_block(sectors - 1, &one).unwrap();
        let mut head = vec![0u8; SECTOR + 17];
        dev.read_block(sectors - 1, &mut head).unwrap();
        assert_eq!(&head[..SECTOR], &one[..]);
        dev.sync().unwrap();
        // The run is still there after the single writes.
        dev.read_blocks(1001, &mut back).unwrap();
        assert_eq!(back, run);
    }
}

#[test]
fn test_reject_ranges_the_card_cannot_hold() {
    let (m, mut dev) = device(model::qemu_8bit());
    let sectors = dev.block_count();
    let issued = model::commands(&m).len();
    let mut buf = vec![0u8; 2 * SECTOR];
    assert_eq!(dev.read_blocks(sectors - 1, &mut buf), Err(BlockError::OutOfRange));
    assert_eq!(dev.write_blocks(sectors, &buf[..SECTOR]), Err(BlockError::OutOfRange));
    assert_eq!(dev.read_blocks(u64::from(u32::MAX) + 1, &mut buf), Err(BlockError::OutOfRange));
    assert_eq!(dev.read_blocks(u64::MAX, &mut buf), Err(BlockError::OutOfRange));
    assert_eq!(dev.read_blocks(0, &mut buf[..SECTOR + 1]), Err(BlockError::OutOfRange));
    assert_eq!(dev.read_blocks(0, &mut []), Err(BlockError::OutOfRange));
    assert_eq!(dev.write_blocks(0, &buf[..100]), Err(BlockError::OutOfRange));
    assert_eq!(dev.read_block(0, &mut buf[..SECTOR - 1]), Err(BlockError::IoError));
    assert_eq!(dev.write_block(0, &buf[..SECTOR - 1]), Err(BlockError::IoError));
    // None of them reached the card.
    assert_eq!(model::commands(&m).len(), issued);
}

#[test]
fn test_reject_a_device_error_and_serve_again() {
    let (m, mut dev) = device(model::qemu_8bit());
    let data = pattern(3, 8 * SECTOR);
    dev.write_blocks(64, &data).unwrap();
    m.borrow_mut().faults.data_timeout = true;
    let mut back = vec![0u8; data.len()];
    assert_eq!(dev.read_blocks(64, &mut back), Err(BlockError::IoError));
    dev.read_blocks(64, &mut back).unwrap();
    assert_eq!(back, data);
}

#[test]
fn the_host_configuration_comes_from_the_boot_disks_node() {
    // The board's eMMC: 8 bits, the K1 layer; its node does not (yet) allow HS400ES.
    let board = Fdt::new(BOARD).unwrap();
    let emmc = boot_disk::parse(&board, "/soc/storage-bus/mmc@d4281000").unwrap();
    let config = host_config(&emmc).unwrap();
    assert_eq!((config.bus_width, config.hs400es, config.layer), (8, false, Layer::K1));
    assert_eq!(config.base_clock_hz, None);
    // QEMU's SD host behind PCI: the standard core, its capabilities say the rest.
    let virt = Fdt::new(VIRT).unwrap();
    let pci = boot_disk::parse(&virt, "/soc/pci@30000000/mmc@1,0").unwrap();
    let config = host_config(&pci).unwrap();
    assert_eq!((config.bus_width, config.hs400es, config.layer), (8, false, Layer::Standard));
    // A virtio disk has no SD host.
    let blk = boot_disk::parse(&virt, "/soc/virtio_mmio@10008000").unwrap();
    assert!(host_config(&blk).is_none());
    // HS400ES needs the strobe AND 1.8 V signalling; `bus-width` defaults to 1.
    let fixture = Fdt::new(FIXTURE).unwrap();
    let both = host_config(&boot_disk::parse(&fixture, "/soc/mmc@d4281000").unwrap()).unwrap();
    assert!(both.hs400es);
    let strobe = host_config(&boot_disk::parse(&fixture, "/soc/mmc@d4282000").unwrap()).unwrap();
    assert_eq!((strobe.hs400es, strobe.bus_width), (false, 1));
    let volts = host_config(&boot_disk::parse(&fixture, "/soc/mmc@d4284000").unwrap()).unwrap();
    assert_eq!((volts.hs400es, volts.bus_width), (false, 8));
}

#[test]
fn test_reject_a_bus_width_no_bus_has() {
    let fixture = Fdt::new(FIXTURE).unwrap();
    let odd = boot_disk::parse(&fixture, "/soc/mmc@d4283000").unwrap();
    assert!(host_config(&odd).is_none());
}

#[test]
fn a_record_written_into_the_tree_configures_the_same_host() {
    let mut buf = BOARD.to_vec();
    buf.resize(buf.len() + 1024, 0);
    ChosenWriter::new(&mut buf)
        .unwrap()
        .set_nexus_str(CHOSEN_KEY, "/soc/storage-bus/mmc@d4281000")
        .unwrap();
    let fdt = Fdt::new(&buf).unwrap();
    let disk = boot_disk::resolve(&fdt).unwrap().unwrap();
    assert_eq!(host_config(&disk).map(|c| c.layer), Some(Layer::K1));
}
