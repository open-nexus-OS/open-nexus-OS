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

/// An instance's process died (TASK-0333 6a): the reaper of record (execd) reports what it
/// reaped — a one-way EVENT, never answered, so a death never makes execd wait on this
/// service while this service waits on execd's spawn reply. The OS loop takes it from execd's
/// kernel sender id alone; a frame from anyone else is denied. The instance goes to `Stopped`
/// and leaves the table (and recents): a dead process holds no instance slot.
/// Request:  `[A,M,ver,OP_INSTANCE_EXITED, pid:u32le, code:i32le, reason:u8]`
///           (reason: 0 clean, 1 error, 2 fault, 3 killed, 4 unknown)
/// Response: none.
pub const OP_INSTANCE_EXITED: u8 = 5;

/// What the compositor sees of an app's window (TASK-0333 6d, RFC-0111 §4, ADR-0083): the
/// compositor STATES what is seen, this service DECIDES the lifecycle state and ORDERS the
/// launcher (execd) to freeze, thaw or kill. A one-way EVENT like the death report — windowd
/// never waits on this service — taken from windowd's kernel sender id alone; a frame from
/// anyone else is denied. The app is named by the kernel service id of its process
/// (`app:<bundle_id>`), the identity the compositor captured at the surface's creation.
/// Request:  `[A,M,ver,OP_SEEN, owner_sid:u64le, seen:u8]`
///           (seen: 0 focused, 1 visible, 2 hidden, 3 gone — `seen::Seen`)
/// Response: none.
pub const OP_SEEN: u8 = 6;

/// The memory authority's kill order (TASK-0333 6e-2, RFC-0100 §3, ADR-0086): memd decided
/// WHO goes when the kernel's level says somebody must; this service checks the instance is
/// not the foreground, ORDERS the parent (execd `OP_KILL`) and answers whether the order was
/// carried out. Taken from memd's kernel sender id alone; a frame from anyone else is denied.
/// The death itself comes back as the reaper's `OP_INSTANCE_EXITED`, and the instance's band
/// goes to memd as `gone` — that is the kill's proof, not this reply.
/// Request:  `[A,M,ver,OP_MEM_KILL, pid:u32le]`
/// Response: `[A,M,ver,OP_MEM_KILL|0x80, status, pid:u32le]`
pub const OP_MEM_KILL: u8 = 7;

/// The memory authority's squeeze order (TASK-0333 6f-1, RFC-0100 §5, ADR-0087): at `critical`
/// memd asks for a frozen app's private memory to be squeezed before it asks for any kill; this
/// service checks the instance is suspended, ORDERS the parent (execd `OP_SQUEEZE`) and answers
/// the parent's totals. Taken from memd's kernel sender id alone; a frame from anyone else is
/// denied.
/// Request:  `[A,M,ver,OP_MEM_SQUEEZE, pid:u32le]`
/// Response: `[A,M,ver,OP_MEM_SQUEEZE|0x80, status, pid:u32le, pages:u32le, zero:u32le,
///            kept:u32le, taken:u32le, stored:u32le]`
pub const OP_MEM_SQUEEZE: u8 = 8;

/// A new compositor runs (RFC-0112, amendment 2026-11-02): what the old one saw is void. The
/// compositor says so once at its start — a one-way EVENT like `OP_SEEN`, taken from windowd's
/// kernel sender id alone. Every frozen instance is thawed into the background (it must run to
/// draw its window again; the new compositor's sightings move it from there) and every hidden
/// clock stops.
/// Request:  `[A,M,ver,OP_COMPOSITOR_UP]`
/// Response: none.
pub const OP_COMPOSITOR_UP: u8 = 9;

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
/// The order was accepted but the parent did not carry it out (execd refused or is gone).
pub const STATUS_FAILED: u8 = 6;

/// Minimum frame length: `MAGIC0 + MAGIC1 + VERSION + OP`.
pub const MIN_FRAME_LEN: usize = 4;
