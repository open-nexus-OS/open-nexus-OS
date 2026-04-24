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
#[cfg(any(test, all(feature = "os-lite", nexus_env = "os")))]
#[must_use]
pub(crate) fn decode_exec_policy_decision(frame: &[u8], expected_nonce: u32) -> Option<u8> {
    let (nonce, status) = nexus_abi::policy::decode_exec_check_rsp(frame)?;
    if nonce != expected_nonce {
        return None;
    }
    Some(status)
}

/// Enforces a narrow allowlist for crash-event publish callers.
///
/// v1 keeps crash publish surface private to trusted selftest paths; all other
/// callers are rejected fail-closed.
#[cfg(any(test, all(feature = "os-lite", nexus_env = "os")))]
#[must_use]
pub(crate) fn crash_event_publish_allowed(
    sender_service_id: u64,
    trusted_sender_id: u64,
    trusted_sender_alt_id: u64,
) -> bool {
    sender_service_id == trusted_sender_id || sender_service_id == trusted_sender_alt_id
}

/// Rejects spawn-time capability sets that would bypass the sandboxed VFS path.
#[cfg(any(test, all(feature = "os-lite", nexus_env = "os")))]
#[must_use]
pub(crate) fn spawn_caps_respect_vfs_boundary(caps: &[&str]) -> bool {
    !caps.iter().any(|cap| *cap == "packagefsd" || *cap == "statefsd")
}

#[cfg(any(test, all(feature = "os-lite", nexus_env = "os")))]
const DEMO_MINIDUMP_NAME: &str = "demo.minidump";
#[cfg(any(test, all(feature = "os-lite", nexus_env = "os")))]
const DEMO_MINIDUMP_PATH: &str = "/state/crash/child.demo.minidump.nmd";

/// Validates that a reported dump path is consistent with the payload name.
#[cfg(any(test, all(feature = "os-lite", nexus_env = "os")))]
#[must_use]
pub(crate) fn reported_minidump_path_matches_name(path: &str, name: &str) -> bool {
    if name == DEMO_MINIDUMP_NAME {
        return path == DEMO_MINIDUMP_PATH;
    }
    let path_bytes = path.as_bytes();
    let suffix_len = name.len().saturating_add(5);
    if path_bytes.len() < suffix_len {
        return false;
    }
    let start = path_bytes.len() - suffix_len;
    path_bytes[start] == b'.'
        && &path_bytes[start + 1..start + 1 + name.len()] == name.as_bytes()
        && &path_bytes[start + 1 + name.len()..] == b".nmd"
}

/// Validates decoded minidump fields against expected crash metadata.
#[cfg(any(test, all(feature = "os-lite", nexus_env = "os")))]
#[derive(Clone, Copy)]
pub(crate) struct MinidumpFrameMetadata<'a> {
    pub pid: u32,
    pub code: i32,
    pub name: &'a str,
    pub build_id: &'a str,
}

/// Validates decoded minidump fields against expected crash metadata.
#[cfg(any(test, all(feature = "os-lite", nexus_env = "os")))]
#[must_use]
pub(crate) fn reported_minidump_frame_matches_expected(
    frame: MinidumpFrameMetadata<'_>,
    expected: MinidumpFrameMetadata<'_>,
) -> bool {
    let pid_matches =
        frame.pid == expected.pid || (expected.name == DEMO_MINIDUMP_NAME && frame.pid == 0);
    pid_matches
        && frame.code == expected.code
        && frame.name == expected.name
        && frame.build_id == expected.build_id
}

#[cfg(test)]
mod tests {
    use super::{
        crash_event_publish_allowed, decode_exec_policy_decision, spawn_caps_respect_vfs_boundary,
        reported_minidump_frame_matches_expected, reported_minidump_path_matches_name,
        MinidumpFrameMetadata,
    };

    #[test]
    fn test_decode_exec_policy_decision_accepts_matching_nonce() {
        let nonce = 0xA1B2_C3D4;
        let rsp = nexus_abi::policy::encode_exec_check_rsp(nonce, nexus_abi::policy::STATUS_ALLOW);
        assert_eq!(decode_exec_policy_decision(&rsp, nonce), Some(nexus_abi::policy::STATUS_ALLOW));
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

    #[test]
    fn test_reject_unauthenticated_crash_event_publish() {
        let trusted = nexus_abi::service_id_from_name(b"selftest-client");
        let trusted_alt = 0x68c1_66c3_7bcd_7154u64;
        let attacker = nexus_abi::service_id_from_name(b"attacker");
        assert!(!crash_event_publish_allowed(attacker, trusted, trusted_alt));
    }

    #[test]
    fn test_allow_authenticated_crash_event_publish() {
        let trusted = nexus_abi::service_id_from_name(b"selftest-client");
        let trusted_alt = 0x68c1_66c3_7bcd_7154u64;
        assert!(crash_event_publish_allowed(trusted, trusted, trusted_alt));
        assert!(crash_event_publish_allowed(trusted_alt, trusted, trusted_alt));
    }

    #[test]
    fn test_reject_direct_fs_cap_bypass_at_spawn_boundary() {
        assert!(!spawn_caps_respect_vfs_boundary(&["vfsd", "packagefsd"]));
        assert!(!spawn_caps_respect_vfs_boundary(&["statefsd"]));
        assert!(spawn_caps_respect_vfs_boundary(&["vfsd", "logd"]));
    }

    #[test]
    fn test_reported_minidump_path_matches_name_accepts_demo_exact_path() {
        assert!(reported_minidump_path_matches_name(
            "/state/crash/child.demo.minidump.nmd",
            "demo.minidump"
        ));
        assert!(!reported_minidump_path_matches_name(
            "/state/crash/forged.demo.minidump.nmd",
            "demo.minidump"
        ));
    }

    #[test]
    fn test_reported_minidump_path_matches_name_rejects_mismatched_suffix() {
        assert!(reported_minidump_path_matches_name(
            "/state/crash/123.99.demo.exit42.nmd",
            "demo.exit42"
        ));
        assert!(!reported_minidump_path_matches_name(
            "/state/crash/123.99.demo.other.nmd",
            "demo.exit42"
        ));
    }

    #[test]
    fn test_reported_minidump_frame_matches_expected_rejects_any_mismatch() {
        assert!(reported_minidump_frame_matches_expected(
            MinidumpFrameMetadata { pid: 7, code: 42, name: "demo.minidump", build_id: "b123" },
            MinidumpFrameMetadata { pid: 7, code: 42, name: "demo.minidump", build_id: "b123" },
        ));
        assert!(reported_minidump_frame_matches_expected(
            MinidumpFrameMetadata { pid: 0, code: 42, name: "demo.minidump", build_id: "b123" },
            MinidumpFrameMetadata { pid: 7, code: 42, name: "demo.minidump", build_id: "b123" },
        ));
        assert!(!reported_minidump_frame_matches_expected(
            MinidumpFrameMetadata { pid: 8, code: 42, name: "demo.minidump", build_id: "b123" },
            MinidumpFrameMetadata { pid: 7, code: 42, name: "demo.minidump", build_id: "b123" },
        ));
        assert!(!reported_minidump_frame_matches_expected(
            MinidumpFrameMetadata { pid: 7, code: 43, name: "demo.minidump", build_id: "b123" },
            MinidumpFrameMetadata { pid: 7, code: 42, name: "demo.minidump", build_id: "b123" },
        ));
        assert!(!reported_minidump_frame_matches_expected(
            MinidumpFrameMetadata { pid: 7, code: 42, name: "demo.other", build_id: "b123" },
            MinidumpFrameMetadata { pid: 7, code: 42, name: "demo.minidump", build_id: "b123" },
        ));
        assert!(!reported_minidump_frame_matches_expected(
            MinidumpFrameMetadata { pid: 7, code: 42, name: "demo.minidump", build_id: "b999" },
            MinidumpFrameMetadata { pid: 7, code: 42, name: "demo.minidump", build_id: "b123" },
        ));
    }
}
