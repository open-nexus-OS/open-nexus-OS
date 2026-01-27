// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RNG daemon — single entropy authority service
//! OWNERS: @runtime @security
//! STATUS: Functional (OS-lite backend)
//! API_STABILITY: Stable (v1.0)
//! TEST_COVERAGE: 12 unit tests (host/std) + QEMU selftest markers (selftest-client)
//!
//! PUBLIC API: service_main_loop(), ReadyNotifier
//! DEPENDS_ON: nexus_ipc, nexus_abi, rng-virtio
//! ADR: docs/adr/0006-device-identity-architecture.md
//!
//! SECURITY INVARIANTS:
//!   - Entropy bytes MUST NOT be logged
//!   - All requests MUST be policy-gated via sender_service_id
//!   - Requests are bounded to MAX_ENTROPY_BYTES

#![forbid(unsafe_code)]
#![cfg_attr(
    all(feature = "os-lite", nexus_env = "os", target_arch = "riscv64", target_os = "none"),
    no_std
)]

#[cfg(all(feature = "os-lite", nexus_env = "os"))]
extern crate alloc;

#[cfg(all(feature = "os-lite", nexus_env = "os"))]
mod os_lite;

#[cfg(all(feature = "os-lite", nexus_env = "os"))]
pub use os_lite::*;

#[cfg(feature = "std")]
mod std_impl;

#[cfg(feature = "std")]
pub use std_impl::*;

/// Maximum entropy bytes per request (matching rng-virtio).
pub const MAX_ENTROPY_BYTES: usize = 256;

/// Wire protocol constants.
pub mod protocol {
    pub const MAGIC0: u8 = b'R';
    pub const MAGIC1: u8 = b'G';
    pub const VERSION: u8 = 1;

    // Operations
    pub const OP_GET_ENTROPY: u8 = 1;

    // Response flag
    pub const OP_RESPONSE: u8 = 0x80;

    // Status codes
    pub const STATUS_OK: u8 = 0;
    pub const STATUS_OVERSIZED: u8 = 1;
    pub const STATUS_DENIED: u8 = 2;
    pub const STATUS_UNAVAILABLE: u8 = 3;
    pub const STATUS_MALFORMED: u8 = 4;

    /// Minimum frame length: MAGIC0 + MAGIC1 + VERSION + OP
    pub const MIN_FRAME_LEN: usize = 4;

    /// Request header length for GET_ENTROPY:
    /// MAGIC0 + MAGIC1 + VERSION + OP + nonce:u32le + n:u16le
    pub const GET_ENTROPY_REQ_LEN: usize = 10;

    /// Capability required for entropy requests.
    pub const CAP_RNG_ENTROPY: &[u8] = b"rng.entropy";
}
