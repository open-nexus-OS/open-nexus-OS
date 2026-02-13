// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RNG daemon — single entropy authority service
//! OWNERS: @runtime @security
//! STATUS: Functional (OS-lite backend)
//! API_STABILITY: Stable (v1.0)
//! TEST_COVERAGE: Unit tests (host/std, including nonce-decoder hardening) + QEMU selftest markers (selftest-client)
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

/// Decodes a policyd delegated-capability v2 response with nonce correlation.
///
/// Returns `Some(status)` only when the response shape is valid and nonce matches.
pub(crate) fn decode_delegated_cap_decision(frame: &[u8], expected_nonce: u32) -> Option<u8> {
    if frame.len() != 10 || frame[0] != b'P' || frame[1] != b'O' || frame[2] != 2 {
        return None;
    }
    if frame[3] != (5 | 0x80) {
        return None;
    }
    let nonce = u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
    if nonce != expected_nonce {
        return None;
    }
    Some(frame[8])
}

#[cfg(test)]
mod tests {
    use super::decode_delegated_cap_decision;

    #[test]
    fn test_decode_delegated_cap_decision_accepts_matching_nonce() {
        let nonce = 0xA1B2_C3D4;
        let rsp = [b'P', b'O', 2, 5 | 0x80, 0xD4, 0xC3, 0xB2, 0xA1, 0, 0];
        assert_eq!(decode_delegated_cap_decision(&rsp, nonce), Some(0));
    }

    #[test]
    fn test_decode_delegated_cap_decision_rejects_malformed() {
        assert_eq!(decode_delegated_cap_decision(&[0u8; 4], 1), None);
    }

    #[test]
    fn test_decode_delegated_cap_decision_rejects_nonce_mismatch() {
        let rsp = [b'P', b'O', 2, 5 | 0x80, 1, 0, 0, 0, 0, 0];
        assert_eq!(decode_delegated_cap_decision(&rsp, 2), None);
    }
}
