// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Policyd wire helpers used by OS-lite clients and host tests.
//!
//! This module is intentionally a small, pure wrapper around `nexus_abi::policyd` so we can:
//! - write deterministic host tests for policyd request/reply framing, and
//! - keep selftest/client logic focused on assertions rather than byte parsing.
//!
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Internal (used by selftest + host tests)
//! TEST_COVERAGE: Unit tests (host)
//!
//! INVARIANTS:
//! - Never panics on malformed/truncated input
//! - No allocation required for decoding

#![forbid(unsafe_code)]

/// Errors when decoding policyd wire frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WireError {
    /// Frame is malformed or truncated.
    Malformed,
}

/// Decoded policyd response (v2/v3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Response {
    /// Protocol version (2 or 3).
    pub ver: u8,
    /// Opcode (without the response bit).
    pub op: u8,
    /// Correlation nonce.
    pub nonce: nexus_abi::policyd::Nonce,
    /// Decision status (ALLOW/DENY/MALFORMED/UNSUPPORTED).
    pub status: u8,
}

/// Decodes a policyd v2/v3 response frame.
pub fn decode_response(frame: &[u8]) -> Result<Response, WireError> {
    let (ver, op, nonce, status) =
        nexus_abi::policyd::decode_rsp_v2_or_v3(frame).ok_or(WireError::Malformed)?;
    Ok(Response { ver, op, nonce, status })
}

/// Returns true if `status` is `STATUS_ALLOW`.
pub fn is_allow(status: u8) -> bool {
    status == nexus_abi::policyd::STATUS_ALLOW
}

/// Returns true if `status` is `STATUS_DENY`.
pub fn is_deny(status: u8) -> bool {
    status == nexus_abi::policyd::STATUS_DENY
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rsp_v2_decodes() {
        let frame = nexus_abi::policyd::encode_rsp_v2(
            nexus_abi::policyd::OP_ROUTE,
            0xAABBCCDD,
            nexus_abi::policyd::STATUS_DENY,
        );
        let rsp = decode_response(&frame).unwrap();
        assert_eq!(rsp.ver, nexus_abi::policyd::VERSION_V2);
        assert_eq!(rsp.op, nexus_abi::policyd::OP_ROUTE);
        assert_eq!(rsp.nonce, 0xAABBCCDD);
        assert!(is_deny(rsp.status));
    }

    #[test]
    fn rsp_v3_decodes() {
        let frame = nexus_abi::policyd::encode_rsp_v3(
            nexus_abi::policyd::OP_EXEC,
            0x01020304,
            nexus_abi::policyd::STATUS_ALLOW,
        );
        let rsp = decode_response(&frame).unwrap();
        assert_eq!(rsp.ver, nexus_abi::policyd::VERSION_V3);
        assert_eq!(rsp.op, nexus_abi::policyd::OP_EXEC);
        assert_eq!(rsp.nonce, 0x01020304);
        assert!(is_allow(rsp.status));
    }

    #[test]
    fn malformed_response_rejected() {
        // Too short + wrong magic.
        let bad = [0u8; 7];
        assert_eq!(decode_response(&bad), Err(WireError::Malformed));
    }
}
