#![forbid(unsafe_code)]
#![allow(unexpected_cfgs)]
#![cfg_attr(
    all(feature = "os-lite", nexus_env = "os", target_arch = "riscv64", target_os = "none"),
    no_std
)]

//! CONTEXT: execd daemon – payload executor and service spawner
//! OWNERS: @services-team
//! PUBLIC API: service_main_loop(), exec helpers
//! DEPENDS_ON: nexus_ipc, nexus_loader (host), nexus_abi (os-lite stubs)
//! ADR: docs/adr/0017-service-architecture.md

#[cfg(all(feature = "os-lite", nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
extern crate alloc;

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
mod os_lite;
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
pub use os_lite::*;

#[cfg(not(all(nexus_env = "os", feature = "os-lite")))]
mod std_server;
#[cfg(not(all(nexus_env = "os", feature = "os-lite")))]
pub use std_server::*;

/// Decodes an exec policy response with nonce correlation.
///
/// Returns `Some(status)` only when the frame is valid and `nonce` matches
/// the expected request nonce; otherwise returns `None` so callers can fail-closed.
pub(crate) fn decode_exec_policy_decision(frame: &[u8], expected_nonce: u32) -> Option<u8> {
    let (nonce, status) = nexus_abi::policy::decode_exec_check_rsp(frame)?;
    if nonce != expected_nonce {
        return None;
    }
    Some(status)
}

#[cfg(test)]
mod tests {
    use super::decode_exec_policy_decision;

    #[test]
    fn test_decode_exec_policy_decision_accepts_matching_nonce() {
        let nonce = 0xA1B2_C3D4;
        let rsp = nexus_abi::policy::encode_exec_check_rsp(nonce, nexus_abi::policy::STATUS_ALLOW);
        assert_eq!(
            decode_exec_policy_decision(&rsp, nonce),
            Some(nexus_abi::policy::STATUS_ALLOW)
        );
    }

    #[test]
    fn test_decode_exec_policy_decision_rejects_malformed() {
        let bad = [0u8; 4];
        assert_eq!(decode_exec_policy_decision(&bad, 1), None);
    }

    #[test]
    fn test_decode_exec_policy_decision_rejects_nonce_mismatch() {
        let rsp = nexus_abi::policy::encode_exec_check_rsp(7, nexus_abi::policy::STATUS_ALLOW);
        assert_eq!(decode_exec_policy_decision(&rsp, 8), None);
    }
}
