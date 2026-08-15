// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: updated bootctl persistence codec — A/B boot-control state
//!   <-> statefs Integrity envelope (TASK-0025 step 4)
//! OWNERS: @services-team
//! STATUS: Functional
//! API_STABILITY: Unstable (service-internal; value format SSOT is `statefs::envelope`)
//! TEST_COVERAGE: tests/bootctl_envelope.rs (roundtrip, legacy, stale-seq,
//!   malformed negatives)
//!
//! PUBLIC API:
//!   - BOOTCTRL_STATE_KEY / SUBJECT / PURPOSE
//!   - encode_slot / decode_slot, encode/decode_bootctrl_state, bootctrl_from_state
//!   - seal_bootctl / open_bootctl: BootCtrl <-> Integrity envelope
//!
//! `/state/boot/*` sits on the Integrity floor (alg = none, monotonic seq —
//! chicken-egg rule: no MAC key needed). Reads accept BOTH envelope v1 and
//! pre-migration legacy raw bytes; writes are always enveloped with
//! seq = last_seen + 1 (first write: seq = 1). Moved cfg-free out of
//! os_lite.rs so the host suite proves the same bytes the OS path writes.
//!
//! ADR: docs/adr/0024-updates-ab-packaging-architecture.md

extern crate alloc;

use alloc::vec::Vec;

use statefs::{writer, StatefsError};
use updates::{BootCtrl, BootCtrlError, Slot};

/// statefs key of the persisted boot-control record.
pub const BOOTCTRL_STATE_KEY: &str = "/state/boot/bootctl.v1";
/// Envelope meta subject for every updated-authored record.
pub const SUBJECT: &str = "updated";
/// Envelope meta purpose for the boot-control record.
pub const PURPOSE: &str = "bootctl";

/// Version byte of the (payload-internal) bootctl state encoding.
pub const BOOTCTRL_STATE_VERSION: u8 = 1;
const SLOT_NONE: u8 = 0xff;

/// Wire encoding of a slot id (1 = A, 2 = B).
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

/// Snapshot of the persisted boot-control fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootCtrlState {
    /// Currently active slot.
    pub active: Slot,
    /// Slot pending health confirmation, if any.
    pub pending: Option<Slot>,
    /// Slot with a staged (unswitched) system set, if any.
    pub staged: Option<Slot>,
    /// Remaining boot attempts before rollback.
    pub tries_left: u8,
    /// Whether the active slot has been health-confirmed.
    pub health_ok: bool,
}

impl BootCtrlState {
    /// Capture the persisted fields from a live `BootCtrl`.
    pub fn from_bootctrl(boot: &BootCtrl) -> Self {
        Self {
            active: boot.active_slot(),
            pending: boot.pending_slot(),
            staged: boot.staged_slot(),
            tries_left: boot.tries_left(),
            health_ok: boot.health_ok(),
        }
    }
}

/// Encode the boot-control state as the 6-byte payload (v1).
pub fn encode_bootctrl_state(boot: &BootCtrl) -> [u8; 6] {
    let state = BootCtrlState::from_bootctrl(boot);
    [
        BOOTCTRL_STATE_VERSION,
        encode_slot(state.active),
        state.pending.map(encode_slot).unwrap_or(SLOT_NONE),
        state.staged.map(encode_slot).unwrap_or(SLOT_NONE),
        state.tries_left,
        if state.health_ok { 1 } else { 0 },
    ]
}

/// Decode the 6-byte payload (v1); bounded, deterministic, no panic.
pub fn decode_bootctrl_state(bytes: &[u8]) -> Result<BootCtrlState, StatefsError> {
    if bytes.len() != 6 || bytes[0] != BOOTCTRL_STATE_VERSION {
        return Err(StatefsError::Corrupted);
    }
    let active = decode_slot(bytes[1])?.ok_or(StatefsError::Corrupted)?;
    let pending = decode_slot(bytes[2])?;
    let staged = decode_slot(bytes[3])?;
    let tries_left = bytes[4];
    let health_ok = bytes[5] == 1;
    Ok(BootCtrlState { active, pending, staged, tries_left, health_ok })
}

/// Reconstruct a live `BootCtrl` from a decoded state snapshot.
pub fn bootctrl_from_state(state: BootCtrlState) -> Result<BootCtrl, BootCtrlError> {
    if state.pending.is_some() {
        let base = state.active.other();
        let mut boot = BootCtrl::new(base);
        boot.stage();
        let _ = boot.switch(state.tries_left)?;
        return Ok(boot);
    }
    if state.health_ok {
        let base = state.active.other();
        let mut boot = BootCtrl::new(base);
        boot.stage();
        let _ = boot.switch(0)?;
        boot.commit_health()?;
        return Ok(boot);
    }
    let mut boot = BootCtrl::new(state.active);
    if state.staged.is_some() {
        boot.stage();
    }
    Ok(boot)
}

/// Seal the boot-control state as an Integrity envelope with `seq`.
pub fn seal_bootctl(boot: &BootCtrl, seq: u64, ts: u64) -> Result<Vec<u8>, StatefsError> {
    let payload = encode_bootctrl_state(boot);
    writer::seal_integrity(BOOTCTRL_STATE_KEY, seq, SUBJECT, PURPOSE, ts, &payload)
}

/// Open a stored boot-control record: envelope v1 or legacy raw 6-byte
/// payload (pre-migration journals). Malformed input is `Corrupted` —
/// deterministic, never a panic (journal bytes are untrusted input).
pub fn open_bootctl(bytes: &[u8]) -> Result<(BootCtrl, Option<u64>), StatefsError> {
    let stored = writer::open_stored(bytes)?;
    let state = decode_bootctrl_state(stored.payload())?;
    let boot = bootctrl_from_state(state).map_err(|_| StatefsError::Corrupted)?;
    Ok((boot, stored.seq()))
}
