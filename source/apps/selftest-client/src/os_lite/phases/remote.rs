// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Phase 11 of 12 — remote (TASK-0005 cross-VM remote proxy proof,
//!   opt-in 2-VM harness: remote resolve / remote query / remote statefs rw
//!   roundtrip / remote pkgfs read).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU 2-VM marker ladder (`tools/os2vm.sh` / Node A); single-VM
//!   smoke skips this phase by design.
//!
//! Extracted in Cut P2-12 of TASK-0023B. Marker order and marker strings are
//! byte-identical to the pre-cut body.
//!
//! Gating:
//!   Only Node A (`ctx.os2vm && ctx.local_ip.is_some()`) emits the markers;
//!   single-VM smoke must not block on remote RPC waits.
//!
//! Determinism:
//!   Each remote RPC retries on a 4_000 ms wall-clock budget (RFC-0019
//!   nonce-correlated request/reply via dsoftbusd) so that the test stays
//!   bounded if the peer is slow to come up.
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

use nexus_abi::yield_;

use crate::markers::emit_line;
use crate::os_lite::context::PhaseCtx;
use crate::os_lite::dsoftbus;

pub(crate) fn run(ctx: &mut PhaseCtx) -> core::result::Result<(), ()> {
    // TASK-0005: Cross-VM remote proxy proof (opt-in 2-VM harness).
    // Only Node A emits the markers; single-VM smoke must not block on remote RPC waits.
    if ctx.os2vm && ctx.local_ip.is_some() {
        // Retry with a wall-clock bound to keep tests deterministic and fast.
        // dsoftbusd must establish the session first.
        let start_ms = (nexus_abi::nsec().unwrap_or(0) / 1_000_000) as u64;
        let deadline_ms = start_ms.saturating_add(4_000);
        let mut ok = false;
        loop {
            if dsoftbus::remote::resolve::dsoftbusd_remote_resolve("bundlemgrd").is_ok() {
                ok = true;
                break;
            }
            let now_ms = (nexus_abi::nsec().unwrap_or(0) / 1_000_000) as u64;
            if now_ms >= deadline_ms {
                break;
            }
            let _ = yield_();
        }
        if ok {
            emit_line("SELFTEST: remote resolve ok");
        } else {
            emit_line("SELFTEST: remote resolve FAIL");
        }

        let start_ms = (nexus_abi::nsec().unwrap_or(0) / 1_000_000) as u64;
        let deadline_ms = start_ms.saturating_add(4_000);
        let mut got: Option<u16> = None;
        loop {
            if let Ok(count) = dsoftbus::remote::resolve::dsoftbusd_remote_bundle_list() {
                got = Some(count);
                break;
            }
            let now_ms = (nexus_abi::nsec().unwrap_or(0) / 1_000_000) as u64;
            if now_ms >= deadline_ms {
                break;
            }
            let _ = yield_();
        }
        if let Some(_count) = got {
            emit_line("SELFTEST: remote query ok");
        } else {
            emit_line("SELFTEST: remote query FAIL");
        }

        let start_ms = (nexus_abi::nsec().unwrap_or(0) / 1_000_000) as u64;
        let deadline_ms = start_ms.saturating_add(4_000);
        let mut statefs_ok = false;
        loop {
            if dsoftbus::remote::statefs::dsoftbusd_remote_statefs_rw_roundtrip().is_ok() {
                statefs_ok = true;
                break;
            }
            let now_ms = (nexus_abi::nsec().unwrap_or(0) / 1_000_000) as u64;
            if now_ms >= deadline_ms {
                break;
            }
            let _ = yield_();
        }
        if statefs_ok {
            emit_line("SELFTEST: remote statefs rw ok");
        } else {
            emit_line("SELFTEST: remote statefs rw FAIL");
        }

        let start_ms = (nexus_abi::nsec().unwrap_or(0) / 1_000_000) as u64;
        let deadline_ms = start_ms.saturating_add(4_000);
        let mut pkg_ok = false;
        loop {
            if let Ok(bytes) = dsoftbus::remote::pkgfs::dsoftbusd_remote_pkgfs_read_once(
                "pkg:/system/build.prop",
                64,
            ) {
                if !bytes.is_empty() {
                    pkg_ok = true;
                    break;
                }
            }
            let now_ms = (nexus_abi::nsec().unwrap_or(0) / 1_000_000) as u64;
            if now_ms >= deadline_ms {
                break;
            }
            let _ = yield_();
        }
        if pkg_ok {
            emit_line("SELFTEST: remote pkgfs read ok");
        } else {
            emit_line("SELFTEST: remote pkgfs read FAIL");
        }
    }

    Ok(())
}
