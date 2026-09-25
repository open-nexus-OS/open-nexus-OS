// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The boot loader's path (TASK-0246B P1): PIO in both directions on a polling host —
//! no DMA memory, no interrupt — the way nxboot drives the core. Written sectors arrive where
//! they were written (a single sector, the BSB's shape, and a run) and read back by PIO; the
//! write is programmed and its status read when it returns; every refusal a PIO write can meet
//! is named, and the card serves the next request.
//! OWNERS: @runtime @drivers

use storage_sdhci::proto::status;
use storage_sdhci::regs::ERR_DATA_TIMEOUT;
use storage_sdhci::{Card, Ceiling, Error};

use crate::model::card::pattern;
use crate::model::{self, commands, Shared};

/// A card on a host that polls (no interrupt line), as the loader drives it.
fn polling(config: model::Config) -> (Shared, crate::ModelCard) {
    let m = model::machine(config);
    let card =
        Card::init(model::host(&m, false), Ceiling::Hs52).map_err(|f| f.error).expect("init");
    (m, card)
}

fn data(seed: u8, sectors: usize) -> Vec<u8> {
    (0..sectors * 512).map(|i| (i as u8).wrapping_mul(13).wrapping_add(seed)).collect()
}

/// The card writes and reads again at `lba` after a refusal.
fn serves_again(card: &mut crate::ModelCard, lba: u32) {
    let written = data(0x77, 1);
    card.write_pio(lba, &written).expect("write after the refusal");
    let mut back = vec![0u8; 512];
    card.read_pio(lba, &mut back).expect("read after the refusal");
    assert_eq!(back, written);
}

#[test]
fn pio_writes_arrive_where_they_were_written() {
    for config in [model::qemu(), model::qemu_8bit(), model::k1()] {
        let (m, mut card) = polling(config);
        let one = data(1, 1);
        card.write_pio(40, &one).expect("one sector");
        let run = data(2, 3);
        card.write_pio(100, &run).expect("a run");
        let mut back = vec![0u8; 512];
        card.read_pio(40, &mut back).expect("read the sector");
        assert_eq!(back, one);
        let mut back = vec![0u8; 3 * 512];
        card.read_pio(100, &mut back).expect("read the run");
        assert_eq!(back, run);
        // The neighbours keep the card's own data; no DMA memory was touched.
        let mut next = vec![0u8; 512];
        card.read_pio(41, &mut next).expect("a neighbour");
        assert!(next.iter().enumerate().all(|(i, b)| *b == pattern(41, i)));
        assert!(m.borrow().mem.ops.is_empty());
    }
}

#[test]
fn a_pio_write_is_programmed_and_its_status_read_when_it_returns() {
    let (m, mut card) = polling(model::qemu_8bit());
    let before = commands(&m).len();
    card.write_pio(7, &data(3, 2)).expect("write");
    let issued: Vec<u8> = commands(&m)[before..].iter().map(|c| c.0).collect();
    assert_eq!(issued, [23, 25, 13], "the count, the write, the status after programming");
}

#[test]
fn test_reject_a_pio_write_that_times_out_and_recover() {
    let (m, mut card) = polling(model::qemu_8bit());
    m.borrow_mut().faults.data_timeout = true;
    assert_eq!(
        card.write_pio(7, &data(4, 1)),
        Err(Error::Controller { cmd: 25, status: ERR_DATA_TIMEOUT })
    );
    serves_again(&mut card, 7);
}

#[test]
fn test_reject_r1_error_bits_on_a_pio_write() {
    let (m, mut card) = polling(model::qemu_8bit());
    m.borrow_mut().faults.status_bits = Some((25, status::WP_VIOLATION));
    match card.write_pio(7, &data(5, 1)) {
        Err(Error::CardStatus { cmd: 25, status }) => assert!(status & status::WP_VIOLATION != 0),
        other => panic!("{other:?}"),
    }
    m.borrow_mut().faults.status_bits = None;
    serves_again(&mut card, 7);
}

#[test]
fn test_reject_a_short_pio_write_and_recover() {
    let (m, mut card) = polling(model::qemu_8bit());
    m.borrow_mut().faults.short_by = 2;
    let before = m.borrow().now;
    assert_eq!(card.write_pio(7, &data(6, 4)), Err(Error::ShortTransfer { remaining: 2 }));
    assert!(m.borrow().now - before < storage_sdhci::timeouts::WRITE_TIMEOUT_US);
    serves_again(&mut card, 7);
}

#[test]
fn test_reject_a_short_pio_read_and_recover() {
    // The controller completes after two of four blocks: named at once, never waited out.
    let (m, mut card) = polling(model::qemu_8bit());
    m.borrow_mut().faults.short_by = 2;
    let before = m.borrow().now;
    assert_eq!(card.read_pio(9, &mut [0u8; 4 * 512]), Err(Error::ShortTransfer { remaining: 2 }));
    assert!(m.borrow().now - before < storage_sdhci::timeouts::READ_TIMEOUT_US);
    let mut back = vec![0u8; 512];
    card.read_pio(9, &mut back).expect("read after the refusal");
    assert!(back.iter().enumerate().all(|(i, b)| *b == pattern(9, i)));
}

#[test]
fn test_reject_pio_writes_outside_the_card_or_not_whole_sectors() {
    let (m, mut card) = polling(model::qemu_8bit());
    let end = card.sectors();
    let issued = commands(&m).len();
    assert_eq!(card.write_pio(end, &[0u8; 512]), Err(Error::Range));
    assert_eq!(card.write_pio(end - 1, &[0u8; 1024]), Err(Error::Range));
    assert_eq!(card.write_pio(0, &[0u8; 100]), Err(Error::Range));
    assert_eq!(card.write_pio(0, &[]), Err(Error::Range));
    assert_eq!(commands(&m).len(), issued, "nothing reached the card");
    card.write_pio(end - 1, &data(8, 1)).expect("the last sector is inside");
}
