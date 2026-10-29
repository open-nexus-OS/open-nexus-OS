// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! TASK-0333 6f-2 (ADR-0088) host proofs: the pager's books and its place in the policy. At
//! `critical` a frozen app is squeezed, then — with a pager that can swap — swapped out, and only
//! then is anyone killed; without a pager (no key) the swap step is skipped; a band change makes
//! the app a candidate again (squeezed first). The slot table hands out the lowest free slot,
//! frees exactly what it held and never more than the partition has; a slot's bytes carry the
//! sealed record and its length, and a slot that holds none is refused (`test_reject_*`).

use memd::answer::{Memd, Outcome, Peers};
use memd::order::{encode_seed_swap_key, seed_swap_key_status, Recording, SwappedOut};
use memd::swap::{decode_slot, encode_slot, SlotUse, Slots, SLOT_BYTES, SLOT_HEAD};
use nexus_abi::syscall::memory::{MEM_PRESSURE_CRITICAL, MEM_PRESSURE_EMERGENCY};
use nexus_abi::SWAP_MAX_SEALED;
use nexus_wire::memd as wire;

const ABILITYMGR: u64 = 0xAB11;
const HARNESS: u64 = 0x5E1F;
const PEERS: Peers = Peers { abilitymgr: ABILITYMGR, harness: HARNESS, gpud: 0x6B0D };

fn push(m: &mut Memd, pid: u32, band: u8) {
    let mut out = [0u8; wire::REPLY_MAX_BYTES];
    let mut orders = Recording::accepting();
    let (n, outcome) = m.answer(&wire::encode_band(pid, band), ABILITYMGR, &mut out, &mut orders);
    assert_eq!(n, 0);
    assert!(matches!(outcome, Outcome::Band(_)));
}

fn drill(m: &mut Memd, level: u8, orders: &mut Recording) -> (u8, u32, u8) {
    let mut out = [0u8; wire::REPLY_MAX_BYTES];
    let (n, _) = m.answer(&wire::encode_drill(level), HARNESS, &mut out, orders);
    wire::decode_drill_reply(wire::OP_DRILL, &out[..n]).expect("drill reply")
}

fn frozen_pair() -> Memd {
    let mut m = Memd::new(PEERS, true);
    push(&mut m, 10, wire::BAND_FOREGROUND);
    push(&mut m, 20, wire::BAND_SUSPENDED);
    push(&mut m, 21, wire::BAND_SUSPENDED);
    m
}

#[test]
fn at_critical_a_frozen_app_is_squeezed_then_swapped_out_then_killed() {
    let mut m = frozen_pair();
    let mut orders = Recording::paging();
    let suspended = wire::BAND_SUSPENDED;
    // Both squeezed first, the older first …
    assert_eq!(
        drill(&mut m, MEM_PRESSURE_CRITICAL, &mut orders),
        (wire::STATUS_SQUEEZED, 20, suspended)
    );
    assert_eq!(
        drill(&mut m, MEM_PRESSURE_CRITICAL, &mut orders),
        (wire::STATUS_SQUEEZED, 21, suspended)
    );
    // … then both swapped out, the older first …
    assert_eq!(
        drill(&mut m, MEM_PRESSURE_CRITICAL, &mut orders),
        (wire::STATUS_SWAPPED, 20, suspended)
    );
    assert_eq!(
        drill(&mut m, MEM_PRESSURE_CRITICAL, &mut orders),
        (wire::STATUS_SWAPPED, 21, suspended)
    );
    assert_eq!(orders.swap_orders(), &[20, 21]);
    assert!(orders.orders().is_empty(), "no kill while a squeezed app was not swapped out");
    assert_eq!(m.authority().swap_outs(), 2);
    // … and only then the kill, of the older.
    assert_eq!(drill(&mut m, MEM_PRESSURE_CRITICAL, &mut orders), (wire::STATUS_OK, 20, suspended));
    assert_eq!(orders.orders(), &[20]);
}

#[test]
fn without_a_pager_the_swap_step_is_skipped() {
    let mut m = frozen_pair();
    let mut orders = Recording::accepting();
    assert_eq!(drill(&mut m, MEM_PRESSURE_CRITICAL, &mut orders).0, wire::STATUS_SQUEEZED);
    assert_eq!(drill(&mut m, MEM_PRESSURE_CRITICAL, &mut orders).0, wire::STATUS_SQUEEZED);
    assert_eq!(drill(&mut m, MEM_PRESSURE_CRITICAL, &mut orders), (wire::STATUS_OK, 20, 4));
    assert!(orders.swap_orders().is_empty(), "no key, no swap-out asked");
}

#[test]
fn emergency_kills_at_once_and_a_band_change_makes_an_app_a_candidate_again() {
    let mut m = frozen_pair();
    let mut orders = Recording::paging();
    assert_eq!(drill(&mut m, MEM_PRESSURE_EMERGENCY, &mut orders), (wire::STATUS_OK, 20, 4));
    assert!(orders.squeeze_orders().is_empty() && orders.swap_orders().is_empty());
    // App 21: squeezed and swapped, thawed (foreground), frozen again: squeezed first again.
    let mut m = frozen_pair();
    let mut orders = Recording::paging();
    push(&mut m, 20, wire::BAND_GONE);
    assert_eq!(drill(&mut m, MEM_PRESSURE_CRITICAL, &mut orders).0, wire::STATUS_SQUEEZED);
    assert_eq!(drill(&mut m, MEM_PRESSURE_CRITICAL, &mut orders).0, wire::STATUS_SWAPPED);
    push(&mut m, 21, wire::BAND_FOREGROUND);
    push(&mut m, 21, wire::BAND_SUSPENDED);
    assert_eq!(drill(&mut m, MEM_PRESSURE_CRITICAL, &mut orders).0, wire::STATUS_SQUEEZED);
    assert_eq!(drill(&mut m, MEM_PRESSURE_CRITICAL, &mut orders).0, wire::STATUS_SWAPPED);
    assert_eq!(orders.swap_orders(), &[21, 21]);
}

/// A swap-out that moved nothing is no swap-out: the reply says it failed, the app is not
/// asked again until its band changes, the kill comes next.
#[test]
fn test_reject_a_swap_out_that_moved_nothing() {
    let mut m = frozen_pair();
    push(&mut m, 21, wire::BAND_GONE);
    let mut orders = Recording::paging();
    orders.swap = Some(SwappedOut { pages: 0, slots: 0 });
    assert_eq!(drill(&mut m, MEM_PRESSURE_CRITICAL, &mut orders).0, wire::STATUS_SQUEEZED);
    assert_eq!(drill(&mut m, MEM_PRESSURE_CRITICAL, &mut orders), (wire::STATUS_FAILED, 20, 4));
    assert_eq!(m.authority().swap_outs(), 0);
    assert_eq!(drill(&mut m, MEM_PRESSURE_CRITICAL, &mut orders), (wire::STATUS_OK, 20, 4));
}

#[test]
fn slots_hand_out_the_lowest_free_one_and_free_what_they_held() {
    let mut slots = Slots::new(130);
    assert_eq!(slots.take(7, 0x70), Some(0));
    assert_eq!(slots.take(7, 0x71), Some(1));
    assert_eq!(slots.take(8, 0x80), Some(2));
    assert_eq!(slots.used(), 3);
    assert_eq!(slots.owner(1), Some(SlotUse { slot: 1, pid: 7, token: 0x71 }));
    assert_eq!(slots.release(1).map(|u| u.token), Some(0x71));
    assert_eq!(slots.release(1), None, "freed once");
    assert_eq!(slots.take(9, 0x90), Some(1), "the lowest free slot again");
    let held: Vec<u32> = slots.held_by(7).iter().map(|u| u.slot).collect();
    assert_eq!(held, [0]);
    assert_eq!(slots.held_by(9).len(), 1);
}

#[test]
fn test_reject_a_slot_past_the_partition() {
    let mut slots = Slots::new(65);
    for i in 0..65 {
        assert_eq!(slots.take(1, i), Some(i as u32));
    }
    assert_eq!(slots.take(1, 99), None, "the partition is full");
    assert_eq!(slots.capacity(), 65);
    assert!(Slots::new(0).take(1, 1).is_none());
}

#[test]
fn a_slot_carries_the_sealed_record_and_test_reject_one_that_holds_none() {
    let sealed: Vec<u8> = (0..700u32).map(|i| (i * 7 % 256) as u8).collect();
    let mut slot = [0xAAu8; SLOT_BYTES];
    encode_slot(&sealed, &mut slot).expect("fits");
    assert_eq!(decode_slot(&slot), Some(&sealed[..]));
    assert!(slot[SLOT_HEAD + sealed.len()..].iter().all(|b| *b == 0), "zeroes after");
    // Longer than the seal's bound: never written, never read back.
    assert!(encode_slot(&vec![0u8; SWAP_MAX_SEALED + 1], &mut slot).is_none());
    let mut long = [0u8; SLOT_BYTES];
    long[..2].copy_from_slice(&((SWAP_MAX_SEALED + 1) as u16).to_le_bytes());
    assert_eq!(decode_slot(&long), None);
    assert_eq!(decode_slot(&[0u8; SLOT_BYTES]), None, "a slot never written");
    assert_eq!(decode_slot(&[5, 0, 1]), None, "cut short");
}

/// The pager's one request to the RNG service, byte for byte as `rngd::protocol` reads it; only
/// the answer to THIS request counts (the reply inbox is shared with the lifecycle authority's).
#[test]
fn the_seed_request_is_rngds_frame_and_test_reject_another_answer() {
    assert_eq!(encode_seed_swap_key(0x0102_0304), [b'R', b'G', 1, 2, 4, 3, 2, 1]);
    let answer = |status: u8, nonce: u32| {
        let n = nonce.to_le_bytes();
        [b'R', b'G', 1, 0x82, status, n[0], n[1], n[2], n[3]]
    };
    assert_eq!(seed_swap_key_status(&answer(0, 7), 7), Some(0));
    assert_eq!(seed_swap_key_status(&answer(3, 7), 7), Some(3), "unavailable is an answer");
    assert_eq!(seed_swap_key_status(&answer(0, 8), 7), None, "another request's answer");
    assert_eq!(seed_swap_key_status(&answer(0, 7)[..8], 7), None, "cut short");
    let mut foreign = answer(0, 7);
    foreign[3] = 0x81;
    assert_eq!(seed_swap_key_status(&foreign, 7), None, "an entropy answer");
}
