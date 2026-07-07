// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: abilitymgr wire protocol constants — ops + status codes for the
//! lifecycle broker IPC (RFC-0065).
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests (constants; exercised by `wire.rs` dispatch tests)
//!
//! abilitymgr wire protocol constants (RFC-0065 lifecycle broker IPC).
//!
//! Frame layout mirrors the other services' hand-rolled binary protocol so the
//! OS-lite loop stays allocation-light and deterministic.

/// Frame magic byte 0 (`A`bility `M`anager).
pub const MAGIC0: u8 = b'A';
/// Frame magic byte 1.
pub const MAGIC1: u8 = b'M';
/// Protocol version.
pub const VERSION: u8 = 1;

/// Response flag OR'd into the opcode of a reply.
pub const OP_RESPONSE: u8 = 0x80;

// --- Operations ---
/// Launch a new ability instance.
/// Request:  `[A,M,ver,OP_LAUNCH, app_len:u8, app..., abil_len:u8, abil...]`
/// Response: `[A,M,ver,OP_LAUNCH|0x80, status, instance_id:u32le, state:u8]`
pub const OP_LAUNCH: u8 = 1;
/// Drive a lifecycle transition on an existing instance.
/// Request:  `[A,M,ver,OP_TRANSITION, instance_id:u32le, to_state:u8]`
/// Response: `[A,M,ver,OP_TRANSITION|0x80, status, instance_id:u32le, state:u8]`
pub const OP_TRANSITION: u8 = 2;
/// Query the recents list (count only in v1).
/// Request:  `[A,M,ver,OP_RECENTS]`
/// Response: `[A,M,ver,OP_RECENTS|0x80, status, count:u16le]`
pub const OP_RECENTS: u8 = 3;

/// Resolve an app-to-app export (TASK-0081, manifest v2.2): the consumer
/// asks WHO serves `ability`; abilitymgr checks the export exists AND the
/// consumer's manifest holds the app-owned permission (fail-closed both
/// sides) and answers with the exporter's app id — the "mediated" half;
/// the endpoint mint + direct channel ride with the OS loop.
/// IDENTITY: the OS loop binds `consumer` to the VERIFIED sender (kernel
/// sender id), never to the string alone — the frame field is display/
/// lookup, the channel is the authority (the execd requester pattern).
/// Request:  `[A,M,ver,OP_RESOLVE_EXPORT, consumer_len:u8, consumer...,
///             abil_len:u8, ability...]`
/// Response: `[A,M,ver,OP_RESOLVE_EXPORT|0x80, status,
///             exporter_len:u8, exporter...]`
pub const OP_RESOLVE_EXPORT: u8 = 4;

// --- Status codes ---
/// Operation succeeded.
pub const STATUS_OK: u8 = 0;
/// Frame was malformed (bad magic/version/length).
pub const STATUS_MALFORMED: u8 = 1;
/// No instance with the given id.
pub const STATUS_UNKNOWN: u8 = 2;
/// The requested transition is illegal from the current state.
pub const STATUS_INVALID_TRANSITION: u8 = 3;
/// The instance table is full.
pub const STATUS_FULL: u8 = 4;

/// Launch denied: the app's manifest declares a capability the platform does not
/// recognize (fail-closed permission check). RFC-0065 launch authority.
pub const STATUS_DENIED: u8 = 5;

/// Minimum frame length: `MAGIC0 + MAGIC1 + VERSION + OP`.
pub const MIN_FRAME_LEN: usize = 4;
