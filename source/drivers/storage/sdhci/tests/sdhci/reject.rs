// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The reject matrix: every failure is named, bounded, and — for a data error — followed by
//! a card that is back in the transfer state and serves the next request.
//! OWNERS: @runtime @drivers

use storage_sdhci::adma::AdmaError;
use storage_sdhci::proto::{ext, status, ExtCsdError};
use storage_sdhci::regs::{ERR_CMD_CRC, ERR_CMD_TIMEOUT, ERR_DATA_TIMEOUT};
use storage_sdhci::{Card, Ceiling, Disk, Error, Host, HostConfig, Layer, Stage};

use crate::model::card::pattern;
use crate::model::mem::ModelMem;
use crate::model::{self, commands, ModelBus, Shared, SimPlatform};

fn init_error(m: &Shared) -> Error {
    match Card::init(model::host(m, true), Ceiling::Hs400es) {
        Ok(_) => panic!("init succeeded"),
        Err(failure) => failure.error,
    }
}

/// A read after an error is served, and the sector is the right one.
fn serves_again(disk: &mut crate::ModelDisk) {
    let mut buf = [0u8; 512];
    disk.read(9, &mut buf).expect("read after recovery");
    assert!(buf.iter().enumerate().all(|(i, b)| *b == pattern(9, i)));
}

#[test]
fn test_reject_a_command_the_card_never_answers() {
    let m = model::machine(model::qemu());
    m.borrow_mut().faults.silent = Some(1);
    assert_eq!(init_error(&m), Error::Controller { cmd: 1, status: ERR_CMD_TIMEOUT });
}

#[test]
fn test_reject_a_response_that_fails_its_crc() {
    let m = model::machine(model::qemu());
    m.borrow_mut().faults.crc = Some(2);
    assert_eq!(init_error(&m), Error::Controller { cmd: 2, status: ERR_CMD_CRC });
}

#[test]
fn test_reject_a_controller_that_never_completes_within_the_command_deadline() {
    let m = model::machine(model::qemu());
    m.borrow_mut().faults.never_complete = Some(9);
    let before = m.borrow().now;
    assert_eq!(init_error(&m), Error::Timeout(Stage::Command));
    assert!(m.borrow().now - before >= storage_sdhci::timeouts::COMMAND_TIMEOUT_US);
}

#[test]
fn test_reject_a_data_timeout_and_recover() {
    let m = model::machine(model::k1());
    let mut disk = crate::disk(&m, crate::card(&m, Ceiling::Hs400es));
    m.borrow_mut().faults.data_timeout = true;
    let issued = commands(&m).len();
    assert_eq!(
        disk.read(9, &mut [0u8; 512]),
        Err(Error::Controller { cmd: 18, status: ERR_DATA_TIMEOUT })
    );
    let after: Vec<u8> = commands(&m)[issued..].iter().map(|c| c.0).collect();
    assert_eq!(after, [23, 18, 12, 13], "the card is stopped and its state read back");
    serves_again(&mut disk);
}

#[test]
fn test_reject_an_adma_error_and_recover() {
    let m = model::machine(model::k1());
    let mut disk = crate::disk(&m, crate::card(&m, Ceiling::Hs400es));
    m.borrow_mut().faults.adma_error = true;
    assert_eq!(disk.read(9, &mut [0u8; 1024]), Err(Error::Adma { cmd: 18, status: 1 }));
    serves_again(&mut disk);
}

#[test]
fn test_reject_a_short_transfer_and_recover() {
    let m = model::machine(model::k1());
    let mut disk = crate::disk(&m, crate::card(&m, Ceiling::Hs400es));
    m.borrow_mut().faults.short_by = 3;
    assert_eq!(disk.read(9, &mut [0u8; 8 * 512]), Err(Error::ShortTransfer { remaining: 3 }));
    serves_again(&mut disk);
}

#[test]
fn test_reject_r1_error_bits() {
    let m = model::machine(model::k1());
    let mut disk = crate::disk(&m, crate::card(&m, Ceiling::Hs400es));
    m.borrow_mut().faults.status_bits = Some((18, status::CARD_ECC_FAILED));
    match disk.read(9, &mut [0u8; 512]) {
        Err(Error::CardStatus { cmd: 18, status }) => {
            assert!(status & status::CARD_ECC_FAILED != 0)
        }
        other => panic!("{other:?}"),
    }
    m.borrow_mut().faults.status_bits = None;
    serves_again(&mut disk);
}

#[test]
fn test_reject_an_unexpected_card_state() {
    let m = model::machine(model::qemu());
    m.borrow_mut().faults.stuck_state = Some((7, 0));
    assert_eq!(init_error(&m), Error::CardState { cmd: 7, state: 0, want: 3 });
}

#[test]
fn test_reject_ext_csd_with_zero_or_out_of_range_sectors() {
    for (sectors, error) in
        [(0u32, ExtCsdError::ZeroCapacity), (0x40_0000, ExtCsdError::CapacityOutOfRange)]
    {
        let mut config = model::qemu();
        config.ext_csd[ext::SEC_COUNT..ext::SEC_COUNT + 4].copy_from_slice(&sectors.to_le_bytes());
        let m = model::machine(config);
        assert_eq!(init_error(&m), Error::ExtCsd(error));
    }
}

#[test]
fn test_reject_a_byte_addressed_card() {
    let m = model::machine(model::qemu());
    m.borrow_mut().card.sector_mode = false;
    assert_eq!(init_error(&m), Error::ByteAddressed);
}

#[test]
fn test_reject_a_switch_the_card_refuses() {
    let m = model::machine(model::qemu());
    m.borrow_mut().card.refuse = Some(ext::HS_TIMING);
    match init_error(&m) {
        Error::CardStatus { cmd: 13, status } => assert!(status & status::SWITCH_ERROR != 0),
        other => panic!("{other:?}"),
    }
}

#[test]
fn test_reject_a_bus_that_corrupts_wide_data() {
    let m = model::machine(model::qemu_8bit());
    m.borrow_mut().faults.corrupt_wide = true;
    assert_eq!(init_error(&m), Error::BusVerify);
}

#[test]
fn test_reject_controller_states_that_never_settle() {
    for (clock, stage) in [(true, Stage::ClockStable), (false, Stage::Reset)] {
        let m = model::machine(model::qemu());
        if clock {
            m.borrow_mut().faults.clock_never_stable = true;
        } else {
            m.borrow_mut().faults.reset_never_clears = true;
        }
        assert_eq!(init_error(&m), Error::Timeout(stage));
    }
}

#[test]
fn test_reject_requests_outside_the_card_or_not_whole_sectors() {
    let m = model::machine(model::k1());
    let mut disk = crate::disk(&m, crate::card(&m, Ceiling::Hs400es));
    let end = disk.sectors();
    assert_eq!(disk.read(end - 1, &mut [0u8; 1024]), Err(Error::Range));
    assert_eq!(disk.read(0, &mut [0u8; 100]), Err(Error::Range));
    assert_eq!(disk.read(0, &mut []), Err(Error::Range));
    assert_eq!(disk.write(end, &[0u8; 512]), Err(Error::Range));
    let issued = commands(&m).len();
    assert_eq!(disk.read(end - 1, &mut [0u8; 512]), Ok(()));
    assert_eq!(commands(&m).len(), issued + 2, "the last sector is inside");
}

#[test]
fn test_reject_dma_memory_the_controller_cannot_reach() {
    let m = model::machine(model::k1());
    let make = |table: ModelMem, bounce: ModelMem| {
        let card = crate::card(&m, Ceiling::Hs52);
        Disk::new(card, crate::buffer(&m, table), crate::buffer(&m, bounce)).err()
    };
    let above = make(
        ModelMem::new(&m, 4096, 0x4000_0000, 1, 0),
        ModelMem::new(&m, 8192, 0x1_0000_0000, 1, 0),
    );
    assert_eq!(above, Some(Error::DmaMemory(AdmaError::Above4G)));
    let split = make(
        ModelMem::new(&m, 8192, 0x4100_0000, 2, 1),
        ModelMem::new(&m, 8192, 0x4200_0000, 1, 0),
    );
    assert_eq!(split, Some(Error::DmaMemory(AdmaError::TableSplit)));
}

#[test]
fn test_reject_host_configs_without_a_base_clock_or_with_a_bad_width() {
    let m = model::machine(model::k1());
    let host = |base_clock_hz, bus_width| {
        let platform = SimPlatform { m: m.clone(), irq: true };
        let config = HostConfig { base_clock_hz, bus_width, hs400es: true, layer: Layer::K1 };
        Host::<ModelBus, SimPlatform>::new(ModelBus(m.clone()), platform, config).err()
    };
    assert_eq!(host(None, 8), Some(Error::NoBaseClock));
    assert_eq!(host(Some(375_000_000), 3), Some(Error::Config));
    assert_eq!(host(Some(375_000_000), 8), None);
}
