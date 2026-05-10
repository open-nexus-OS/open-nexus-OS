// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Host/std backend for rngd — for testing and development.
//!
//! This provides a mock implementation for host testing.

use crate::protocol::*;
use crate::MAX_ENTROPY_BYTES;

/// Errors from the rngd service.
#[derive(Debug, thiserror::Error)]
pub enum RngdError {
    #[error("ipc: {0}")]
    Ipc(String),
    #[error("rng device unavailable")]
    DeviceUnavailable,
}

/// Result type for rngd operations.
pub type RngdResult<T> = Result<T, RngdError>;

/// Notifies init once the service reports readiness.
pub struct ReadyNotifier(Box<dyn FnOnce() + Send>);

impl ReadyNotifier {
    /// Creates a notifier from the provided closure.
    pub fn new<F>(func: F) -> Self
    where
        F: FnOnce() + Send + 'static,
    {
        Self(Box::new(func))
    }

    /// Signals readiness to the caller.
    pub fn notify(self) {
        (self.0)();
    }
}

/// Handle a GET_ENTROPY request (for testing).
///
/// # Arguments
/// * `sender_service_id` - The kernel-provided sender service ID
/// * `n` - Number of bytes requested
/// * `policy_check` - Callback to check policy (returns true if allowed)
///
/// # Returns
/// * `(status, entropy_bytes)` tuple
pub fn handle_get_entropy_request<F>(
    sender_service_id: u64,
    n: usize,
    policy_check: F,
) -> (u8, Vec<u8>)
where
    F: FnOnce(u64, &[u8]) -> bool,
{
    // Bounds check
    if n == 0 || n > MAX_ENTROPY_BYTES {
        return (STATUS_OVERSIZED, Vec::new());
    }

    // Policy check
    if !policy_check(sender_service_id, CAP_RNG_ENTROPY) {
        return (STATUS_DENIED, Vec::new());
    }

    // Generate mock entropy (deterministic for tests)
    // SECURITY: In real implementation, never log these bytes!
    let entropy: Vec<u8> = (0..n).map(|i| (i as u8).wrapping_mul(0x5A)).collect();
    (STATUS_OK, entropy)
}

/// Parse a GET_ENTROPY request frame.
///
/// Returns `Some(n)` where n is the requested byte count, or `None` if malformed.
pub fn parse_get_entropy_request(frame: &[u8]) -> Option<usize> {
    if frame.len() != GET_ENTROPY_REQ_LEN {
        return None;
    }
    if frame[0] != MAGIC0 || frame[1] != MAGIC1 || frame[2] != VERSION {
        return None;
    }
    if frame[3] != OP_GET_ENTROPY {
        return None;
    }
    // frame[4..8] = nonce (ignored in host parsing helper)
    let n = u16::from_le_bytes([frame[8], frame[9]]) as usize;
    Some(n)
}

/// Encode a GET_ENTROPY request frame.
pub fn encode_get_entropy_request(n: u16) -> Vec<u8> {
    let mut frame = Vec::with_capacity(GET_ENTROPY_REQ_LEN);
    frame.push(MAGIC0);
    frame.push(MAGIC1);
    frame.push(VERSION);
    frame.push(OP_GET_ENTROPY);
    frame.extend_from_slice(&0u32.to_le_bytes()); // nonce
    frame.extend_from_slice(&n.to_le_bytes());
    frame
}

/// Parse a response frame.
///
/// Returns `Some((status, payload))` or `None` if malformed.
pub fn parse_response(frame: &[u8]) -> Option<(u8, Vec<u8>)> {
    if frame.len() < 9 {
        return None;
    }
    if frame[0] != MAGIC0 || frame[1] != MAGIC1 || frame[2] != VERSION {
        return None;
    }
    if frame[3] != (OP_GET_ENTROPY | OP_RESPONSE) {
        return None;
    }
    let status = frame[4];
    // frame[5..9] = nonce
    let payload = frame[9..].to_vec();
    Some((status, payload))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_parse_request() {
        let frame = encode_get_entropy_request(32);
        let n = parse_get_entropy_request(&frame);
        assert_eq!(n, Some(32));
    }

    #[test]
    fn test_reject_oversized_request() {
        let (status, entropy) = handle_get_entropy_request(
            1, // sender_service_id
            MAX_ENTROPY_BYTES + 1,
            |_, _| true, // policy would allow
        );
        assert_eq!(status, STATUS_OVERSIZED);
        assert!(entropy.is_empty());
    }

    #[test]
    fn test_reject_zero_length_request() {
        let (status, entropy) = handle_get_entropy_request(1, 0, |_, _| true);
        assert_eq!(status, STATUS_OVERSIZED);
        assert!(entropy.is_empty());
    }

    #[test]
    fn test_reject_denied_by_policy() {
        let (status, entropy) = handle_get_entropy_request(
            1,
            32,
            |_, _| false, // policy denies
        );
        assert_eq!(status, STATUS_DENIED);
        assert!(entropy.is_empty());
    }

    #[test]
    fn test_success_with_policy_allow() {
        let (status, entropy) = handle_get_entropy_request(1, 32, |_, _| true);
        assert_eq!(status, STATUS_OK);
        assert_eq!(entropy.len(), 32);
    }

    #[test]
    fn test_max_size_request() {
        let (status, entropy) = handle_get_entropy_request(1, MAX_ENTROPY_BYTES, |_, _| true);
        assert_eq!(status, STATUS_OK);
        assert_eq!(entropy.len(), MAX_ENTROPY_BYTES);
    }

    #[test]
    fn test_parse_malformed_request() {
        // Wrong magic
        let frame = vec![b'X', b'Y', VERSION, OP_GET_ENTROPY, 0, 32];
        assert!(parse_get_entropy_request(&frame).is_none());

        // Too short
        let frame = vec![MAGIC0, MAGIC1, VERSION];
        assert!(parse_get_entropy_request(&frame).is_none());
    }

    #[test]
    fn test_parse_response_rejects_wrong_op() {
        let mut frame = vec![MAGIC0, MAGIC1, VERSION, OP_GET_ENTROPY, STATUS_OK, 0, 0, 0, 0];
        frame.push(0xAA);
        assert!(parse_response(&frame).is_none());
    }

    #[test]
    fn test_parse_response_too_short() {
        let frame = vec![MAGIC0, MAGIC1, VERSION, OP_GET_ENTROPY | OP_RESPONSE, STATUS_OK];
        assert!(parse_response(&frame).is_none());
    }

    // ==========================================================================
    // Negative tests (security proofs)
    // ==========================================================================

    #[test]
    fn test_reject_oversized_entropy_request_boundary() {
        // Test exactly at MAX_ENTROPY_BYTES + 1
        let (status, entropy) = handle_get_entropy_request(1, MAX_ENTROPY_BYTES + 1, |_, _| true);
        assert_eq!(status, STATUS_OVERSIZED);
        assert!(entropy.is_empty());
    }

    #[test]
    fn test_reject_entropy_without_capability() {
        // Test that denial is returned when policy check fails
        let (status, entropy) = handle_get_entropy_request(
            0xDEADBEEF, // arbitrary service ID
            32,
            |_, cap| {
                // Simulate denial for rng.entropy capability
                cap != CAP_RNG_ENTROPY
            },
        );
        assert_eq!(status, STATUS_DENIED);
        assert!(entropy.is_empty());
    }

    #[test]
    fn test_entropy_bytes_not_logged() {
        // This test verifies the contract that entropy is NOT logged.
        // We can't directly test logging, but we verify the API contract:
        // - Success returns entropy bytes only in the response
        // - Error returns empty payload
        let (status, entropy) = handle_get_entropy_request(1, 32, |_, _| true);
        assert_eq!(status, STATUS_OK);
        assert_eq!(entropy.len(), 32);
        // Contract: entropy is returned but must NOT be logged by callers
    }
}
