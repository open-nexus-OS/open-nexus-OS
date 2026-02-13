// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: statefsd service — journaled key-value store for /state (IPC authority)
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Unit tests (delegated-cap nonce-decoder hardening) + service bring-up/QEMU marker proofs
//! ADR: docs/adr/0023-statefs-persistence-architecture.md

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

#[cfg(not(all(feature = "os-lite", nexus_env = "os")))]
mod std_server;
#[cfg(not(all(feature = "os-lite", nexus_env = "os")))]
pub use std_server::*;

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
