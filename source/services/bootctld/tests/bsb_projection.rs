// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: TASK-0036-B host proofs — the BSB runtime projection is a pure
//! function of the record (golden bytes), the idempotency predicate holds,
//! and the startup reconciliation classifies actuator effects vs. genuine
//! drift exactly per ADR-0058 (a trial decrement must NEVER be undone by a
//! resync — that would hand a broken image its tries back).
//! OWNERS: @reliability
//! ADR: docs/adr/0058-boot-selection-block-dual-actor-discipline.md

use bootctld::bsb::{self, ResyncVerdict};
use bootctld::machine::{BootCtrl, Slot};
use bootfmt::bsb::{encode, Bsb, Slot as BsbSlot};

fn switched_record() -> BootCtrl {
    let mut boot = BootCtrl::new(Slot::A);
    boot.stage();
    boot.switch(2, 1_000).expect("switch");
    boot
}

#[test]
fn test_bsb_projection_golden() {
    let boot = switched_record();
    let projected = bsb::project(&boot);
    assert_eq!(projected.active_slot, BsbSlot::A);
    assert_eq!(projected.next_slot, Some(BsbSlot::B));
    assert_eq!(projected.tries_left, 2);
    assert!(!projected.health_committed);
    assert_eq!(projected.boot_target, 0, "normal target byte");
    assert_eq!(projected.rollback_min_index, 0);

    // Golden block bytes at seq 2 (the write after the factory block).
    let sector = encode(&Bsb { seq: 2, ..projected });
    assert_eq!(&sector[0..8], b"NXBSB1\0\0");
    assert_eq!(&sector[8..16], &2u64.to_le_bytes());
    assert_eq!(sector[18], 0, "active a");
    assert_eq!(sector[19], 1, "next b");
    assert_eq!(sector[20], 2, "tries");
    assert_eq!(sector[21], 0, "health uncommitted");
}

#[test]
fn projection_is_idempotent_under_fields_equal() {
    let boot = switched_record();
    let a = bsb::project(&boot);
    let b = Bsb { seq: 99, ..bsb::project(&boot) };
    assert!(bsb::fields_equal(&a, &b), "seq never participates in the predicate");
    assert_eq!(bsb::resync_verdict(Some(&b), &a), ResyncVerdict::Equal);
}

#[test]
fn test_actuator_absorption() {
    let boot = switched_record();
    let desired = bsb::project(&boot); // tries=2, next=b

    // Loader trial decrement on disk: tries 2 -> 1. Must NOT re-project.
    let decremented = Bsb { seq: 2, tries_left: 1, ..desired };
    assert_eq!(bsb::resync_verdict(Some(&decremented), &desired), ResyncVerdict::ActuatorPending);

    // Exhaustion clear: next dropped, tries zeroed. Must NOT re-project.
    let exhausted = Bsb { seq: 4, next_slot: None, tries_left: 0, ..desired };
    assert_eq!(bsb::resync_verdict(Some(&exhausted), &desired), ResyncVerdict::ActuatorPending);
}

#[test]
fn test_bsb_resync_after_crash_window() {
    let boot = switched_record();
    let desired = bsb::project(&boot);

    // Crash window: the record committed the switch but the projection
    // never landed — the disk still shows the pre-switch factory state.
    let stale = Bsb {
        seq: 1,
        active_slot: BsbSlot::A,
        next_slot: None,
        tries_left: 0,
        health_committed: true,
        boot_target: 0,
        rollback_min_index: 0,
    };
    assert_eq!(bsb::resync_verdict(Some(&stale), &desired), ResyncVerdict::Drift);

    // Tries INCREASED on disk is never an actuator effect — drift.
    let inflated = Bsb { seq: 2, tries_left: 3, ..desired };
    assert_eq!(bsb::resync_verdict(Some(&inflated), &desired), ResyncVerdict::Drift);

    // Both blocks invalid: always a re-seed.
    assert_eq!(bsb::resync_verdict(None, &desired), ResyncVerdict::Drift);
}

#[test]
fn actuator_masks_never_hide_other_field_drift() {
    let boot = switched_record();
    let desired = bsb::project(&boot);
    // A tries decrement COMBINED with a flipped health bit is not a pure
    // actuator effect — classification must fail closed to Drift.
    let mixed = Bsb { seq: 2, tries_left: 1, health_committed: true, ..desired };
    assert_eq!(bsb::resync_verdict(Some(&mixed), &desired), ResyncVerdict::Drift);
    // Exhaustion shape but the active slot changed too: Drift.
    let mixed = Bsb { seq: 2, next_slot: None, tries_left: 0, active_slot: BsbSlot::B, ..desired };
    assert_eq!(bsb::resync_verdict(Some(&mixed), &desired), ResyncVerdict::Drift);
}
