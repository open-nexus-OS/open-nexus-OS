// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

//! CONTEXT: bootctld boot-record codec — the ONE persisted boot-state
//! record (ADR-0055) at `/state/boot/bootctl.v1` as a statefs Integrity
//! envelope (alg = none, monotonic seq — chicken-egg rule: no MAC key at
//! boot; TASK-0289 anchors trust later). Payload v2 (9 bytes) persists
//! EVERY machine field — including the rollback slot the v1 codec lost
//! (which forced a stage/switch replay hack on load) — plus the RFC-0087
//! §4 target axis. Reads accept v2, v1 (6-byte payload, subject
//! "updated") and pre-envelope legacy raw bytes; writes are always v2
//! under subject "bootctld" with seq = last_seen + 1. The statefs KEY
//! stays `bootctl.v1` on purpose: it names the record, not the payload
//! version — changing it would orphan every existing image's OTA state.
//! OWNERS: @reliability @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable (service-internal; envelope SSOT is
//!   `statefs::envelope`)
//! TEST_COVERAGE: tests/record_v2.rs (v2 roundtrip, v1 + legacy
//!   migration, target codec rejects, stale-seq, malformed-no-panic).
//! ADR: docs/adr/0055-bootctld-single-boot-state-authority.md

extern crate alloc;

use alloc::vec::Vec;

use statefs::{writer, StatefsError};

use crate::machine::{BootCtrl, BootTarget, Slot};

/// statefs key of the persisted boot record (name is stable — see header).
pub const BOOT_RECORD_KEY: &str = "/state/boot/bootctl.v1";
/// Envelope meta subject for bootctld-authored records.
pub const SUBJECT: &str = "bootctld";
/// Envelope meta purpose for the boot record.
pub const PURPOSE: &str = "bootctl";

/// Payload version this codec writes.
pub const RECORD_VERSION_V2: u8 = 2;
/// Payload version the v1-era codec (updated) wrote; accepted on read.
pub const RECORD_VERSION_V1: u8 = 1;

const SLOT_NONE: u8 = 0xff;
const TARGET_NONE: u8 = 0xff;
const PAYLOAD_LEN_V1: usize = 6;
const PAYLOAD_LEN_V2: usize = 9;

/// Wire encoding of a slot id (1 = A, 2 = B) — unchanged from v1.
pub fn encode_slot(slot: Slot) -> u8 {
    match slot {
        Slot::A => 1,
        Slot::B => 2,
    }
}

/// Decode a slot byte (`SLOT_NONE` = absent); anything else is corrupt.
pub fn decode_slot(byte: u8) -> Result<Option<Slot>, StatefsError> {
    match byte {
        1 => Ok(Some(Slot::A)),
        2 => Ok(Some(Slot::B)),
        SLOT_NONE => Ok(None),
        _ => Err(StatefsError::Corrupted),
    }
}

/// Wire encoding of a boot target (0 = normal, 1 = recovery, 2 = safe).
pub fn encode_target(target: BootTarget) -> u8 {
    match target {
        BootTarget::Normal => 0,
        BootTarget::Recovery => 1,
        BootTarget::Safe => 2,
    }
}

/// Decode a target byte; anything unknown is corrupt (fail closed — a
/// future target added by a newer image must not silently boot `normal`).
pub fn decode_target(byte: u8) -> Result<BootTarget, StatefsError> {
    match byte {
        0 => Ok(BootTarget::Normal),
        1 => Ok(BootTarget::Recovery),
        2 => Ok(BootTarget::Safe),
        _ => Err(StatefsError::Corrupted),
    }
}

fn decode_next_boot(byte: u8) -> Result<Option<BootTarget>, StatefsError> {
    if byte == TARGET_NONE {
        return Ok(None);
    }
    decode_target(byte).map(Some)
}

/// Encode the full machine state as the 9-byte v2 payload.
pub fn encode_record(boot: &BootCtrl) -> [u8; PAYLOAD_LEN_V2] {
    [
        RECORD_VERSION_V2,
        encode_slot(boot.active_slot()),
        boot.pending_slot().map(encode_slot).unwrap_or(SLOT_NONE),
        boot.staged_slot().map(encode_slot).unwrap_or(SLOT_NONE),
        boot.tries_left(),
        if boot.health_ok() { 1 } else { 0 },
        boot.rollback_slot().map(encode_slot).unwrap_or(SLOT_NONE),
        encode_target(boot.boot_target()),
        boot.next_boot().map(encode_target).unwrap_or(TARGET_NONE),
    ]
}

/// Decode a payload: v2 (9 bytes) restores every field directly; v1
/// (6 bytes, updated-era) migrates — rollback derives from the pending
/// switch exactly like the machine's `switch()` sets it, targets default
/// to `normal`/none. Bounded, deterministic, never a panic.
pub fn decode_record(bytes: &[u8]) -> Result<BootCtrl, StatefsError> {
    match (bytes.first().copied(), bytes.len()) {
        (Some(RECORD_VERSION_V2), PAYLOAD_LEN_V2) => {
            let active = decode_slot(bytes[1])?.ok_or(StatefsError::Corrupted)?;
            let pending = decode_slot(bytes[2])?;
            let staged = decode_slot(bytes[3])?;
            let tries_left = bytes[4];
            let health_ok = bytes[5] == 1;
            let rollback = decode_slot(bytes[6])?;
            // Invariant: a pending switch without a rollback destination is
            // unrepresentable in the machine (switch() always records one).
            if pending.is_some() && rollback.is_none() {
                return Err(StatefsError::Corrupted);
            }
            let boot_target = decode_target(bytes[7])?;
            let next_boot = decode_next_boot(bytes[8])?;
            Ok(BootCtrl::restore(
                active,
                pending,
                staged,
                rollback,
                tries_left,
                health_ok,
                boot_target,
                next_boot,
            ))
        }
        (Some(RECORD_VERSION_V1), PAYLOAD_LEN_V1) => {
            let active = decode_slot(bytes[1])?.ok_or(StatefsError::Corrupted)?;
            let pending = decode_slot(bytes[2])?;
            let staged = decode_slot(bytes[3])?;
            let tries_left = bytes[4];
            let health_ok = bytes[5] == 1;
            // v1 never persisted the rollback slot; while a switch is
            // pending it is by construction the OTHER slot (the machine
            // only ever switches active -> other).
            let rollback = pending.map(|_| active.other());
            Ok(BootCtrl::restore(
                active,
                pending,
                staged,
                rollback,
                tries_left,
                health_ok,
                BootTarget::Normal,
                None,
            ))
        }
        _ => Err(StatefsError::Corrupted),
    }
}

/// Seal the boot record as an Integrity envelope with `seq`.
pub fn seal_record(boot: &BootCtrl, seq: u64, ts: u64) -> Result<Vec<u8>, StatefsError> {
    let payload = encode_record(boot);
    writer::seal_integrity(BOOT_RECORD_KEY, seq, SUBJECT, PURPOSE, ts, &payload)
}

/// Open a stored boot record: envelope (v2 or v1 payload, any subject —
/// v1-era records carry "updated") or pre-envelope legacy raw bytes.
pub fn open_record(bytes: &[u8]) -> Result<(BootCtrl, Option<u64>), StatefsError> {
    let stored = writer::open_stored(bytes)?;
    let boot = decode_record(stored.payload())?;
    Ok((boot, stored.seq()))
}
