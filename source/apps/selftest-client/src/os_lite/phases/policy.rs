// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Phase 4 of 12 — policy (bundlemgrd-route-execd deny, identity-bound
//!   allow/deny, MMIO-policy deny, ABI-filter profile distribution, audit-log
//!   verification, keystored sign denied, policyd requester spoof denied,
//!   policyd malformed-frame reject).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) — policy / audit slice.
//!
//! Extracted in Cut P2-07 of TASK-0023B. Marker order and marker strings are
//! byte-identical to the pre-cut body. `bundlemgrd`, `policyd`, `logd`,
//! `keystored` handles are all local to this phase; downstream phases
//! re-resolve via the silent `route_with_retry` / `resolve_keystored_client`
//! (no marker change).
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

use nexus_abi::yield_;

use crate::markers::{emit_byte, emit_bytes, emit_hex_u64, emit_line};
use crate::os_lite::context::PhaseCtx;
use crate::os_lite::ipc::routing::route_with_retry;
use crate::os_lite::services;

pub(crate) fn run(_ctx: &mut PhaseCtx) -> core::result::Result<(), ()> {
    let bundlemgrd = route_with_retry("bundlemgrd").map_err(|_| ())?;
    let policyd = route_with_retry("policyd").map_err(|_| ())?;

    // Policyd-gated routing proof: bundlemgrd asking for execd must be DENIED.
    let (st, route_st) = services::bundlemgrd::bundlemgrd_v1_route_status(&bundlemgrd, "execd")?;
    if st == 0 && route_st == nexus_abi::routing::STATUS_DENIED {
        emit_line(crate::markers::M_SELFTEST_BUNDLEMGRD_ROUTE_EXECD_DENIED_OK);
    } else {
        emit_bytes(crate::markers::M_SELFTEST_BUNDLEMGRD_ROUTE_EXECD_DENIED_ST_0X.as_bytes());
        emit_hex_u64(st as u64);
        emit_bytes(b" route=0x");
        emit_hex_u64(route_st as u64);
        emit_byte(b'\n');
        emit_line(crate::markers::M_SELFTEST_BUNDLEMGRD_ROUTE_EXECD_DENIED_FAIL);
    }
    // Policy check tests: selftest-client must check its own permissions (identity-bound).
    // selftest-client has ["ipc.core"] in policy, so CHECK should return ALLOW.
    if services::policyd::policy_check(&policyd, "selftest-client").unwrap_or(false) {
        emit_line(crate::markers::M_SELFTEST_POLICY_ALLOW_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_POLICY_ALLOW_FAIL);
    }
    // Deny proof (identity-bound): ask policyd whether *selftest-client* has a capability it does NOT have.
    // Use OP_CHECK_CAP so policyd can evaluate a specific capability for the caller, without trusting payload IDs.
    let deny_ok = services::policyd::policyd_check_cap(&policyd, "selftest-client", "crypto.sign")
        .unwrap_or(false)
        == false;
    if deny_ok {
        emit_line(crate::markers::M_SELFTEST_POLICY_DENY_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_POLICY_DENY_FAIL);
    }

    // Device-MMIO policy negative proof: a stable service must NOT be granted a non-matching MMIO capability.
    // netstackd is allowed `device.mmio.net` but must be denied `device.mmio.blk`.
    let mmio_deny_ok =
        services::policyd::policyd_check_cap(&policyd, "netstackd", "device.mmio.blk")
            .unwrap_or(false)
            == false;
    if mmio_deny_ok {
        emit_line(crate::markers::M_SELFTEST_MMIO_POLICY_DENY_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_MMIO_POLICY_DENY_FAIL);
    }

    // TASK-0019: ABI syscall guardrail profile distribution + deny/allow proofs.
    let selftest_sid = nexus_abi::service_id_from_name(b"selftest-client");
    match services::policyd::policyd_fetch_abi_profile(&policyd, selftest_sid) {
        Ok(profile) => {
            if profile.subject_service_id() != selftest_sid {
                emit_line(crate::markers::M_SELFTEST_ABI_FILTER_DENY_FAIL);
                emit_line(crate::markers::M_SELFTEST_ABI_FILTER_ALLOW_FAIL);
                emit_line(crate::markers::M_SELFTEST_ABI_NETBIND_DENY_FAIL);
            } else {
                if profile.check_statefs_put(b"/state/forbidden", 16)
                    == nexus_abi::abi_filter::RuleAction::Deny
                {
                    emit_line(crate::markers::M_ABI_FILTER_DENY_SUBJECT_SELFTEST_CLIENT_SYSCALL_STATEFS_PUT);
                    emit_line(crate::markers::M_SELFTEST_ABI_FILTER_DENY_OK);
                } else {
                    emit_line(crate::markers::M_SELFTEST_ABI_FILTER_DENY_FAIL);
                }

                if profile.check_statefs_put(b"/state/app/selftest/token", 16)
                    == nexus_abi::abi_filter::RuleAction::Allow
                {
                    emit_line(crate::markers::M_SELFTEST_ABI_FILTER_ALLOW_OK);
                } else {
                    emit_line(crate::markers::M_SELFTEST_ABI_FILTER_ALLOW_FAIL);
                }

                if profile.check_net_bind(80) == nexus_abi::abi_filter::RuleAction::Deny {
                    emit_line(
                        crate::markers::M_ABI_FILTER_DENY_SUBJECT_SELFTEST_CLIENT_SYSCALL_NET_BIND,
                    );
                    emit_line(crate::markers::M_SELFTEST_ABI_NETBIND_DENY_OK);
                } else {
                    emit_line(crate::markers::M_SELFTEST_ABI_NETBIND_DENY_FAIL);
                }
            }
        }
        Err(_) => {
            emit_line(crate::markers::M_SELFTEST_ABI_FILTER_DENY_FAIL);
            emit_line(crate::markers::M_SELFTEST_ABI_FILTER_ALLOW_FAIL);
            emit_line(crate::markers::M_SELFTEST_ABI_NETBIND_DENY_FAIL);
        }
    }

    let logd = route_with_retry("logd")?;
    emit_bytes(crate::markers::M_SELFTEST_LOGD_SLOTS.as_bytes());
    let (logd_send, logd_recv) = logd.slots();
    emit_hex_u64(logd_send as u64);
    emit_byte(b' ');
    emit_hex_u64(logd_recv as u64);
    emit_byte(b'\n');
    for _ in 0..64 {
        let _ = yield_();
    }
    // Debug: count records in logd
    let record_count = services::logd::logd_query_count(&logd).unwrap_or(0);
    emit_bytes(crate::markers::M_SELFTEST_LOGD_RECORD_COUNT.as_bytes());
    emit_hex_u64(record_count as u64);
    emit_byte(b'\n');
    // Debug: try to find any audit record
    let any_audit =
        services::logd::logd_query_contains_since_paged(&logd, 0, b"audit").unwrap_or(false);
    if any_audit {
        emit_line(crate::markers::M_SELFTEST_LOGD_HAS_AUDIT_RECORDS);
    } else {
        emit_line(crate::markers::M_SELFTEST_LOGD_HAS_NO_AUDIT_RECORDS);
    }
    let allow_audit = services::logd::logd_query_contains_since_paged(
        &logd,
        0,
        b"audit v1 op=check decision=allow",
    )
    .unwrap_or(false);
    if allow_audit {
        emit_line(crate::markers::M_SELFTEST_POLICY_ALLOW_AUDIT_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_POLICY_ALLOW_AUDIT_FAIL);
    }
    // Deny audit is produced by OP_CHECK_CAP (op=check_cap), not OP_CHECK.
    let deny_audit = services::logd::logd_query_contains_since_paged(
        &logd,
        0,
        b"audit v1 op=check_cap decision=deny",
    )
    .unwrap_or(false);
    if deny_audit {
        emit_line(crate::markers::M_SELFTEST_POLICY_DENY_AUDIT_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_POLICY_DENY_AUDIT_FAIL);
    }
    let keystored = services::keystored::resolve_keystored_client().map_err(|_| ())?;
    if services::policyd::keystored_sign_denied(&keystored).is_ok() {
        emit_line(crate::markers::M_SELFTEST_KEYSTORED_SIGN_DENIED_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_KEYSTORED_SIGN_DENIED_FAIL);
    }
    if services::policyd::policyd_requester_spoof_denied(&policyd).is_ok() {
        emit_line(crate::markers::M_SELFTEST_POLICYD_REQUESTER_SPOOF_DENIED_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_POLICYD_REQUESTER_SPOOF_DENIED_FAIL);
    }

    // Malformed policyd frame should not produce allow/deny.
    let clock = nexus_ipc::budget::OsClock;
    nexus_ipc::budget::send_budgeted(
        &clock,
        &policyd,
        b"bad",
        core::time::Duration::from_millis(100),
    )
    .map_err(|_| ())?;
    let rsp =
        nexus_ipc::budget::recv_budgeted(&clock, &policyd, core::time::Duration::from_millis(100))
            .map_err(|_| ())?;
    if rsp.len() == 6 && rsp[0] == b'P' && rsp[1] == b'O' && rsp[2] == 1 && rsp[4] == 2 {
        emit_line(crate::markers::M_SELFTEST_POLICY_MALFORMED_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_POLICY_MALFORMED_FAIL);
    }

    let _ = (bundlemgrd, policyd, logd, keystored);
    Ok(())
}
