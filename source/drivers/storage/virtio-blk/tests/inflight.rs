// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: TASK-0333 6f-3 (ADR-0089) — the virtio-blk in-flight table and what the device's
//! interrupts say. A slot whose waiter failed comes back with its late completion, a slot the
//! device still owns is never handed out or released, a stray completion is ignored; a device
//! failure is the device's own event (a configuration change with `DEVICE_NEEDS_RESET`), and
//! a configuration change without it is none.
//! OWNERS: @storage @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: this file (host)

use storage_virtio_blk::inflight::{
    interrupt, Completion, DeviceSays, InFlight, ISR_CONFIG, ISR_USED, STATUS_NEEDS_RESET,
};

#[test]
fn a_request_is_claimed_submitted_completed_and_released() {
    let mut t = InFlight::<4>::new();
    let idx = t.claim().expect("a free slot");
    assert!(t.submitted(idx, 7));
    assert!(!t.is_done(idx));
    assert_eq!(t.completed(7), Completion::Waiter(idx));
    assert!(t.is_done(idx));
    assert!(t.release(idx));
    assert_eq!(t.claim(), Some(idx), "the slot is the driver's again");
}

#[test]
fn a_late_completion_reclaims_an_abandoned_slot() {
    let mut t = InFlight::<4>::new();
    // Four waiters fail with their requests still out: before the table, the device was dead
    // from here on.
    for head in 0..4u16 {
        let idx = t.claim().expect("free");
        assert!(t.submitted(idx, head * 3));
        t.abandon(idx);
    }
    assert_eq!(t.abandoned(), 4);
    assert_eq!(t.claim(), None, "every slot is the device's while it holds the chains");
    // The stall ends: the late completions come back and free their slots.
    assert_eq!(t.completed(6), Completion::Reclaimed(2));
    assert_eq!(t.claim(), Some(2));
    for head in [0u16, 3, 9] {
        assert!(matches!(t.completed(head), Completion::Reclaimed(_)));
    }
    assert_eq!(t.abandoned(), 0);
    assert!(t.claim().is_some());
}

#[test]
fn a_request_that_completed_before_its_waiter_gave_up_is_simply_free() {
    let mut t = InFlight::<2>::new();
    let idx = t.claim().expect("free");
    assert!(t.submitted(idx, 1));
    assert_eq!(t.completed(1), Completion::Waiter(idx));
    t.abandon(idx);
    assert_eq!(t.abandoned(), 0);
    assert_eq!(t.claim(), Some(idx));
}

#[test]
fn test_reject_an_unknown_or_duplicate_completion() {
    let mut t = InFlight::<2>::new();
    assert_eq!(t.completed(5), Completion::Unknown, "nobody waits for head 5");
    let idx = t.claim().expect("free");
    assert!(t.submitted(idx, 5));
    assert_eq!(t.completed(5), Completion::Waiter(idx));
    assert_eq!(t.completed(5), Completion::Unknown, "a head comes back once");
    assert!(t.is_done(idx));
}

#[test]
fn test_reject_a_release_while_the_device_owns_the_slot() {
    let mut t = InFlight::<2>::new();
    let idx = t.claim().expect("free");
    assert!(t.submitted(idx, 4));
    assert!(!t.release(idx), "still the device's");
    t.abandon(idx);
    assert!(!t.release(idx), "abandoned is still the device's");
    assert!(!t.release(9), "no such slot");
    assert_ne!(t.claim(), Some(idx));
}

#[test]
fn test_reject_a_reuse_before_the_late_completion() {
    let mut t = InFlight::<1>::new();
    let idx = t.claim().expect("free");
    assert!(t.submitted(idx, 2));
    t.abandon(idx);
    assert_eq!(t.claim(), None);
    assert!(!t.submitted(idx, 3), "a slot the device holds is never submitted twice");
    assert_eq!(t.completed(3), Completion::Unknown);
    assert_eq!(t.completed(2), Completion::Reclaimed(idx));
    assert!(t.submitted(idx, 3));
}

#[test]
fn an_interrupt_says_completions_or_a_failure() {
    assert_eq!(interrupt(ISR_USED, 0), DeviceSays::Used);
    // The device status is read on a configuration change only; a failure outranks the
    // completions that came with it.
    assert_eq!(interrupt(ISR_CONFIG, STATUS_NEEDS_RESET), DeviceSays::Failed);
    assert_eq!(interrupt(ISR_USED | ISR_CONFIG, STATUS_NEEDS_RESET | 0x0f), DeviceSays::Failed);
    assert_eq!(interrupt(0, 0), DeviceSays::Nothing, "a wake without a cause");
}

#[test]
fn test_reject_a_configuration_change_that_is_no_failure() {
    // A resize or any other configuration change is not the device failing.
    assert_eq!(interrupt(ISR_CONFIG, 0x0f), DeviceSays::Nothing);
    assert_eq!(interrupt(ISR_CONFIG | ISR_USED, 0x0f), DeviceSays::Used);
    // `DEVICE_NEEDS_RESET` without the configuration interrupt is not read as one either.
    assert_eq!(interrupt(ISR_USED, STATUS_NEEDS_RESET), DeviceSays::Used);
}
