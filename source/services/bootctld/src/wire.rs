// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

//! CONTEXT: bootctld wire protocol v1 (TASK-0050). Byte frames, magic
//! `B`,`T`. OTA op numbers 1..=5 deliberately mirror the updated wire
//! (`nexus-wire::updated`) so the client conversion is a target swap, not
//! a re-encode. Target ops are new. cfg-free: clients (updated, init,
//! selftest) import these constants instead of hand-rolling frames.
//!
//! Request:  [B, T, ver=1, op, payload…]
//! Response: [B, T, ver=1, op|0x80, status, len:u16le, payload…]
//!
//! OWNERS: @reliability @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: exercised via QEMU ladder + updated/selftest clients.
//! ADR: docs/adr/0055-bootctld-single-boot-state-authority.md

pub const MAGIC0: u8 = b'B';
pub const MAGIC1: u8 = b'T';
pub const VERSION: u8 = 1;

/// OTA slot ops (numbers mirror the updated wire, see header).
pub const OP_STAGE: u8 = 1;
pub const OP_SWITCH: u8 = 2;
pub const OP_HEALTH_OK: u8 = 3;
pub const OP_GET_STATUS: u8 = 4;
/// Boot-attempt tick; the response also carries and CLEARS `next_boot`
/// (one-shot consumption rides the same persisted transaction).
pub const OP_BOOT_ATTEMPT: u8 = 5;
/// Read `[boot_target, next_boot|0xff]`.
pub const OP_GET_TARGET: u8 = 6;
/// Arm a one-shot next-boot target (policy-gated).
pub const OP_SET_NEXT_BOOT: u8 = 7;
/// Set the persistent boot target (policy-gated).
pub const OP_SET_TARGET: u8 = 8;
/// System reset via the kernel SRST primitive (policy-gated; PR-3).
pub const OP_RESET: u8 = 9;
/// Explicit rollback to the recorded rollback slot (updated's switch
/// compensation when bundlemgrd activation fails mid-flight).
pub const OP_ROLLBACK: u8 = 10;
/// Full boot-record snapshot (read; TASK-0051 — `nx diagnose` + the
/// recovery ops surface): the 9-byte record payload v2.
pub const OP_GET_RECORD: u8 = 11;

pub const STATUS_OK: u8 = 0;
pub const STATUS_MALFORMED: u8 = 1;
pub const STATUS_UNSUPPORTED: u8 = 2;
pub const STATUS_FAILED: u8 = 3;
/// Deterministic reject: sender/policy denied (deny-by-default).
pub const STATUS_DENIED: u8 = 4;
/// TASK-0051: slot mutations are blocked while the persistent target is
/// `recovery` — commits belong to the normal boot path.
pub const STATUS_COMMIT_BLOCKED: u8 = 5;

/// `next_boot` absent on the wire.
pub const TARGET_NONE: u8 = 0xff;
