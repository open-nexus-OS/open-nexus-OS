// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Host contract tests for updated's bootctl persistence codec
//!   (Integrity envelopes, TASK-0025 step 4)
//! OWNERS: @services-team
//! STATUS: Functional
//! API_STABILITY: Test-only
//! TEST_COVERAGE: Envelope roundtrip, legacy migration read, stale-seq
//!   rejection (server contract), malformed-envelope determinism
//!
//! ADR: docs/adr/0024-updates-ab-packaging-architecture.md

use statefs::envelope::{SeqTracker, ENVELOPE_MAGIC};
use statefs::{writer, StatefsError};
use updated::bootctl_state::{self, BOOTCTRL_STATE_KEY};
use updates::{BootCtrl, Slot};

/// A non-trivial boot-control fixture: staged + switched (pending, 3 tries).
fn pending_bootctrl() -> BootCtrl {
    let mut boot = BootCtrl::new(Slot::A);
    boot.stage();
    boot.switch(3).expect("switch");
    boot
}

fn assert_same_state(a: &BootCtrl, b: &BootCtrl) {
    assert_eq!(bootctl_state::encode_bootctrl_state(a), bootctl_state::encode_bootctrl_state(b));
}

#[test]
fn test_bootctl_envelope_roundtrip() {
    let boot = pending_bootctrl();
    let sealed = bootctl_state::seal_bootctl(&boot, 1, 42).expect("seal");
    assert_eq!(&sealed[..4], &ENVELOPE_MAGIC, "sealed record carries envelope magic");
    let (reloaded, seq) = bootctl_state::open_bootctl(&sealed).expect("open");
    assert_same_state(&boot, &reloaded);
    assert_eq!(seq, Some(1));
}

#[test]
fn test_bootctl_legacy_raw_still_readable() {
    // Pre-migration journals hold the raw 6-byte state; reads must keep
    // working and report "no seq" (next write starts at seq = 1).
    let boot = pending_bootctrl();
    let raw = bootctl_state::encode_bootctrl_state(&boot);
    let (reloaded, seq) = bootctl_state::open_bootctl(&raw).expect("legacy open");
    assert_same_state(&boot, &reloaded);
    assert_eq!(seq, None);
    assert_eq!(writer::next_seq(seq), 1);
}

#[test]
fn test_bootctl_selftest_probe_shape() {
    // QEMU regression (2026-08-15): the `SELFTEST: bootctl persist` probe
    // reads the record raw over statefs GET and checks the 6-byte v1 payload.
    // After envelope adoption it must unwrap first — mirror its exact
    // predicate here for both storage forms.
    let boot = pending_bootctrl();
    let probe = |bytes: &[u8]| -> bool {
        match writer::open_stored(bytes) {
            Ok(stored) => {
                let payload = stored.payload();
                payload.len() == 6 && payload[0] == bootctl_state::BOOTCTRL_STATE_VERSION
            }
            Err(_) => false,
        }
    };
    let sealed = bootctl_state::seal_bootctl(&boot, 1, 42).expect("seal");
    assert!(probe(&sealed), "enveloped record passes the selftest predicate");
    let raw = bootctl_state::encode_bootctrl_state(&boot);
    assert!(probe(&raw), "legacy raw record passes the selftest predicate");
    assert!(!probe(b"NXEVgarbage"), "malformed envelope fails deterministically");
}

#[test]
fn test_reject_stale_seq_bootctl_rewrite() {
    // Server contract (statefsd SeqTracker): a rewrite whose seq does not
    // exceed the max-seen one is a rollback. The read-modify-write
    // discipline (seq = last_seen + 1) must clear it; a stale seq must not.
    let mut tracker = SeqTracker::new();
    tracker.observe(BOOTCTRL_STATE_KEY, 2).expect("observe");
    let stale = writer::next_seq(None); // writer that ignored the stored seq
    assert_eq!(tracker.check_put(BOOTCTRL_STATE_KEY, stale), Err(StatefsError::RollbackDetected));
    let fresh = writer::next_seq(Some(2));
    assert_eq!(tracker.check_put(BOOTCTRL_STATE_KEY, fresh), Ok(()));
}

#[test]
fn test_reject_malformed_envelope_deterministic_no_panic() {
    // Magic-bearing garbage must be a deterministic error, never a legacy
    // fallback (that would mask tampering) and never a panic.
    let mut truncated = Vec::from(&ENVELOPE_MAGIC[..]);
    truncated.extend_from_slice(&[0u8; 3]);
    let err = bootctl_state::open_bootctl(&truncated).map(|_| ()).unwrap_err();
    assert_eq!(err, StatefsError::Corrupted);

    // A valid envelope wrapping a corrupt payload (bad version byte) is
    // rejected by the payload codec, still deterministically.
    let boot = pending_bootctrl();
    let mut raw = bootctl_state::encode_bootctrl_state(&boot);
    raw[0] = 0xEE; // not BOOTCTRL_STATE_VERSION
    let sealed = writer::seal_integrity(
        BOOTCTRL_STATE_KEY,
        1,
        bootctl_state::SUBJECT,
        bootctl_state::PURPOSE,
        0,
        &raw,
    )
    .expect("seal");
    let err = bootctl_state::open_bootctl(&sealed).map(|_| ()).unwrap_err();
    assert_eq!(err, StatefsError::Corrupted);
}
