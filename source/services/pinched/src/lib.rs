// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: pinched — system-internal compute broker (parallel batch work on
//! the shared nexus-workpool). INVISIBLE to app developers by design: no DSL
//! surface, no `dsl_services.capnp` entry, no app-facing route — only system
//! services and the SDK use it, and only on latency-uncritical batch paths
//! ("user is waiting", e.g. asset bakes). Frame hotpaths must never call it.
//! v1 backend = deterministic partition→map on nexus-workpool; the wire
//! protocol stays backend-agnostic so an interaction-net evaluator
//! (nexus-inet) can slot in later.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: host unit tests (broker transform); QEMU markers
//!   `SELFTEST: pinched determinism ok` / `SELFTEST: pinched bounded ok`.
//! PUBLIC API: service_main_loop(), ReadyNotifier, protocol, broker
//! DEPENDS_ON: nexus-workpool, nexus-ipc, nexus-abi
//! ADR: docs/adr/0016-kernel-libs-architecture.md

#![forbid(unsafe_code)]
#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(all(feature = "os-lite", nexus_env = "os"))]
extern crate alloc;

#[cfg(all(feature = "os-lite", nexus_env = "os"))]
mod os_lite;

#[cfg(all(feature = "os-lite", nexus_env = "os"))]
pub use os_lite::*;

pub mod broker;

/// Upper bound on job elements (bounded-everything: oversized jobs are
/// REJECTED via the completion header, never queued or truncated). 16 Ki
/// u32 elements = 64 KiB payload — sized to the service's static job buffer.
pub const MAX_JOB_ELEMS: usize = 16384;

/// Fixed worker count for the service's workpool (one compute thread per
/// expected secondary CPU; workers self-pin, see nexus-workpool C4).
pub const PINCHED_WORKERS: usize = 2;

/// Wire protocol v1 (system-internal; carried over the init-wired route).
///
/// Request frame: `[MAGIC0, MAGIC1, VERSION, OP_COMPUTE, kind:u8, total:u32le]`
/// plus the job VMO attached via CAP_MOVE (the moved cap IS the data channel,
/// mirroring the RFC-0072 `OP_READ_VMO` splice pattern).
///
/// VMO layout: 16-byte completion header, then `total` u32le elements at
/// [`DATA_OFFSET`]. The client writes header = zeroes (pending) + input
/// elements, sends, then polls the header. The service computes in place and
/// writes payload FIRST, header LAST (release fence): a client that sees
/// [`DONE_MAGIC`] sees complete output. There is no frame reply for
/// OP_COMPUTE — the header IS the completion.
pub mod protocol {
    pub const MAGIC0: u8 = b'P';
    pub const MAGIC1: u8 = b'N';
    pub const VERSION: u8 = 1;

    /// Run a job on the caller's VMO (CAP_MOVE) and complete via its header.
    pub const OP_COMPUTE: u8 = 1;
    /// Response flag for frame replies (malformed/non-compute traffic only).
    pub const OP_RESPONSE: u8 = 0x80;

    pub const STATUS_OK: u32 = 0;
    pub const STATUS_MALFORMED: u32 = 1;
    pub const STATUS_OVERSIZED: u32 = 2;
    pub const STATUS_BAD_KIND: u32 = 3;
    pub const STATUS_IO: u32 = 4;

    /// Job kinds. v1 ships the deterministic proof transform; SVG tessellation
    /// becomes the next kind behind the same partition→map contract.
    pub const JOB_MAP_MIX_U32: u8 = 1;

    pub const MIN_FRAME_LEN: usize = 4;
    /// `[MAGIC0, MAGIC1, VERSION, OP, kind:u8, total:u32le]`
    pub const COMPUTE_REQ_LEN: usize = 9;

    /// Completion header: `[magic:u32le, status:u32le, elems:u32le, workers:u32le]`.
    /// `workers` reports the executing backend width (0 = inline fallback) —
    /// the honest dispatch counter for speedup result-proofs.
    pub const HDR_LEN: usize = 16;
    pub const DATA_OFFSET: usize = 16;
    /// Header magic while the job is in flight (client writes this).
    pub const PENDING_MAGIC: u32 = 0;
    /// Header magic once the service completed the job ("PNC1").
    pub const DONE_MAGIC: u32 = 0x504E_4331;
}
