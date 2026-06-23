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

#[cfg(any(test, all(feature = "os-lite", nexus_env = "os")))]
const SID_METRICSD_ALT: u64 = 0xed20_5ae1_e47c_393d;
#[cfg(any(test, all(feature = "os-lite", nexus_env = "os")))]
const TASK0018_CHILD_DUMP_PATH: &str = "/state/crash/child.demo.minidump.nmd";

/// Maps observed sender identities to canonical policy subjects.
///
/// This keeps policy evaluation deterministic and fail-closed while supporting
/// known bring-up identity aliases.
#[cfg(any(test, all(feature = "os-lite", nexus_env = "os")))]
#[must_use]
pub(crate) fn canonical_policy_subject_for_statefs(
    subject_id: u64,
    op: u8,
    path: &str,
    selftest_sid: u64,
    metricsd_sid: u64,
) -> u64 {
    if subject_id == 0 && op == statefs::protocol::OP_PUT && path == TASK0018_CHILD_DUMP_PATH {
        return selftest_sid;
    }
    if subject_id == SID_METRICSD_ALT {
        return metricsd_sid;
    }
    subject_id
}

#[cfg(test)]
mod tests {
    use super::canonical_policy_subject_for_statefs;

    // The delegated-cap decode is now the shared, host-tested
    // `nexus_ipc::policyd::decode_decision` (RFC-0066) — statefsd's local copy +
    // its tests were removed with the duplicated `policyd_allows`.

    #[test]
    fn test_canonical_policy_subject_maps_only_exact_task0018_child_put() {
        let selftest_sid = 0x11u64;
        assert_eq!(
            canonical_policy_subject_for_statefs(
                0,
                statefs::protocol::OP_PUT,
                "/state/crash/child.demo.minidump.nmd",
                selftest_sid,
                0x22,
            ),
            selftest_sid
        );
        assert_eq!(
            canonical_policy_subject_for_statefs(
                0,
                statefs::protocol::OP_GET,
                "/state/crash/child.demo.minidump.nmd",
                selftest_sid,
                0x22,
            ),
            0
        );
        assert_eq!(
            canonical_policy_subject_for_statefs(
                0,
                statefs::protocol::OP_PUT,
                "/state/crash/child.demo.minidump.v2.nmd",
                selftest_sid,
                0x22,
            ),
            0
        );
    }

    #[test]
    fn test_canonical_policy_subject_maps_metricsd_alias() {
        let metrics_sid = 0x44u64;
        assert_eq!(
            canonical_policy_subject_for_statefs(
                0xed20_5ae1_e47c_393d,
                statefs::protocol::OP_GET,
                "/state/metrics/x",
                0x33,
                metrics_sid
            ),
            metrics_sid
        );
    }
}
