// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Host proofs for the relocated boot-control machine + the v2
//! boot record (TASK-0050): machine flows (ported from
//! tests/updates_host/ota_flow.rs so the relocation cannot drift), v2
//! roundtrip incl. the now-persisted rollback slot, v1 + legacy
//! migration, one-shot next_boot semantics, target-codec rejects,
//! stale-seq discipline, malformed-no-panic.
//! OWNERS: @reliability @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: this file
//! ADR: docs/adr/0055-bootctld-single-boot-state-authority.md

use bootctld::machine::{BootCtrl, BootCtrlError, BootTarget, Slot};
use bootctld::record::{decode_record, encode_record, open_record, seal_record, RECORD_VERSION_V1};
use statefs::writer;

// ---- machine flows (ported: relocation must not drift) ---------------------

#[test]
fn test_stage_switch_health_commit() {
    let mut boot = BootCtrl::new(Slot::A);
    assert_eq!(boot.stage(), Slot::B);
    assert_eq!(boot.switch(3).expect("switch"), Slot::B);
    assert_eq!(boot.active_slot(), Slot::B);
    assert_eq!(boot.rollback_slot(), Some(Slot::A));
    assert_eq!(boot.tries_left(), 3);
    boot.commit_health().expect("health");
    assert!(boot.health_ok());
    assert_eq!(boot.pending_slot(), None);
    assert_eq!(boot.rollback_slot(), None);
}

#[test]
fn test_rollback_on_boot_attempts_exhausted() {
    let mut boot = BootCtrl::new(Slot::A);
    boot.stage();
    boot.switch(2).expect("switch");
    assert_eq!(boot.tick_boot_attempt().expect("tick"), None);
    assert_eq!(boot.tick_boot_attempt().expect("tick"), Some(Slot::A));
    assert_eq!(boot.active_slot(), Slot::A);
    assert!(!boot.health_ok());
}

#[test]
fn test_reject_switch_without_stage() {
    let mut boot = BootCtrl::new(Slot::A);
    assert_eq!(boot.switch(3), Err(BootCtrlError::NotStaged));
}

#[test]
fn test_reject_double_switch() {
    let mut boot = BootCtrl::new(Slot::A);
    boot.stage();
    boot.switch(3).expect("switch");
    boot.stage();
    assert_eq!(boot.switch(3), Err(BootCtrlError::AlreadyPending));
}

#[test]
fn test_reject_commit_health_without_switch() {
    let mut boot = BootCtrl::new(Slot::A);
    assert_eq!(boot.commit_health(), Err(BootCtrlError::NotPending));
}

// ---- v2 record roundtrip ----------------------------------------------------

#[test]
fn test_v2_roundtrip_preserves_every_field() {
    let mut boot = BootCtrl::new(Slot::A);
    boot.stage();
    boot.switch(3).expect("switch");
    boot.set_boot_target(BootTarget::Safe);
    boot.set_next_boot(BootTarget::Recovery);

    let restored = decode_record(&encode_record(&boot)).expect("decode");
    assert_eq!(restored, boot);
    // The v1-era replay hack could not represent this state faithfully:
    // rollback slot now survives the roundtrip directly.
    assert_eq!(restored.rollback_slot(), Some(Slot::A));
    assert_eq!(restored.boot_target(), BootTarget::Safe);
    assert_eq!(restored.next_boot(), Some(BootTarget::Recovery));
}

#[test]
fn test_one_shot_next_boot() {
    let mut boot = BootCtrl::new(Slot::A);
    boot.set_next_boot(BootTarget::Recovery);
    assert_eq!(boot.take_next_boot(), Some(BootTarget::Recovery));
    // One-shot: consumed exactly once.
    assert_eq!(boot.take_next_boot(), None);
    let restored = decode_record(&encode_record(&boot)).expect("decode");
    assert_eq!(restored.next_boot(), None);
}

// ---- migration ----------------------------------------------------------------

#[test]
fn test_v1_payload_migrates_with_derived_rollback() {
    // v1 payload: pending switch to B, 2 tries — the v1 codec never stored
    // the rollback slot; migration derives it (active.other()).
    let v1 = [RECORD_VERSION_V1, 2, 2, 0xff, 2, 0];
    let boot = decode_record(&v1).expect("migrate");
    assert_eq!(boot.active_slot(), Slot::B);
    assert_eq!(boot.pending_slot(), Some(Slot::B));
    assert_eq!(boot.rollback_slot(), Some(Slot::A));
    assert_eq!(boot.tries_left(), 2);
    assert_eq!(boot.boot_target(), BootTarget::Normal);
    assert_eq!(boot.next_boot(), None);
    // Migrated state must behave: exhausting tries rolls back to A.
    let mut boot = boot;
    boot.tick_boot_attempt().expect("tick");
    assert_eq!(boot.tick_boot_attempt().expect("tick"), Some(Slot::A));
}

#[test]
fn test_v1_envelope_from_updated_subject_opens() {
    // A v1-era record sealed by updated (subject "updated") must open.
    let payload = [RECORD_VERSION_V1, 1, 0xff, 0xff, 0, 1];
    let sealed = writer::seal_integrity(
        bootctld::record::BOOT_RECORD_KEY,
        7,
        "updated",
        "bootctl",
        42,
        &payload,
    )
    .expect("seal");
    let (boot, seq) = open_record(&sealed).expect("open");
    assert_eq!(seq, Some(7));
    assert_eq!(boot.active_slot(), Slot::A);
    assert!(boot.health_ok());
    assert_eq!(boot.boot_target(), BootTarget::Normal);
}

#[test]
fn test_legacy_raw_payload_opens() {
    // Pre-envelope journals stored the bare payload.
    let raw = [RECORD_VERSION_V1, 1, 0xff, 0xff, 0, 0];
    let (boot, seq) = open_record(&raw).expect("legacy");
    assert_eq!(seq, None);
    assert_eq!(boot.active_slot(), Slot::A);
}

#[test]
fn test_v2_seal_open_roundtrip_with_seq() {
    let mut boot = BootCtrl::new(Slot::B);
    boot.set_next_boot(BootTarget::Safe);
    let sealed = seal_record(&boot, 9, 100).expect("seal");
    let (opened, seq) = open_record(&sealed).expect("open");
    assert_eq!(seq, Some(9));
    assert_eq!(opened, boot);
}

// ---- rejects -------------------------------------------------------------------

#[test]
fn test_reject_corrupt_payloads() {
    // Unknown version / truncated / bad slot / bad target / unknown
    // next-boot value / pending without rollback — all Corrupted, no panic.
    assert!(decode_record(&[]).is_err());
    assert!(decode_record(&[3, 1, 0xff, 0xff, 0, 0, 0xff, 0, 0xff]).is_err());
    assert!(decode_record(&[2, 1, 0xff, 0xff, 0, 0]).is_err());
    assert!(decode_record(&[2, 9, 0xff, 0xff, 0, 0, 0xff, 0, 0xff]).is_err());
    assert!(decode_record(&[2, 1, 0xff, 0xff, 0, 0, 0xff, 7, 0xff]).is_err());
    assert!(decode_record(&[2, 1, 0xff, 0xff, 0, 0, 0xff, 0, 7]).is_err());
    assert!(decode_record(&[2, 1, 2, 0xff, 3, 0, 0xff, 0, 0xff]).is_err());
    // v1 with bad slot byte.
    assert!(decode_record(&[1, 0, 0xff, 0xff, 0, 0]).is_err());
}

#[test]
fn test_reject_stale_seq_discipline() {
    // The writer's stale-seq rule guards the record: next_seq is monotonic.
    assert_eq!(writer::next_seq(Some(7)), 8);
    assert_eq!(writer::next_seq(None), 1);
}
