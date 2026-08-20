// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Supervised-restart storm proof (TASK-0049B PR-B3b/B3c, RFC-0087
//! §2/§3). Three crash→re-resolve→ping cycles against pinched with client
//! slot hygiene (previous SEND cap closed after each re-resolve — the
//! consumer half of the no-rights-drift invariant), then a restart-counter
//! truth assert against init's persisted `/state/init/restarts/pinched`
//! record (`SELFTEST: crash-loop count ok`); a non-zero baseline on a
//! keep-blk boot is the cross-boot persistence proof
//! (`SELFTEST: crash-loop persist ok`).
//! OWNERS: @runtime @reliability
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU end phase (just test-os, full/headless/smp1) +
//!   keep-blk double-boot lane.
//! ADR: docs/adr/0057-service-restart-capability-re-resolve.md

use nexus_ipc::{Client as _, KernelClient, Wait as IpcWait};

use crate::markers::emit_line;

/// TASK-0049B PR-B3b: real-service restart E2E (ADR-0057, standing —
/// ADR-0048 doctrine). Crashes pinched via the identity-gated
/// OP_SELFTEST_CRASH, then re-resolves the route until init's supervision
/// has restarted and re-provisioned it (STALE/dead window included), and
/// proves the NEW instance answers by pinging an unknown op (deterministic
/// MALFORMED echo). Fail-loud on deadline.
pub(crate) fn restart_proof() {
    const OP_SELFTEST_CRASH: u8 = 2;
    const OP_PING_UNKNOWN: u8 = 0x7e;
    const CYCLES: u32 = 3;
    const DEADLINE_NS: u64 = 30_000_000_000;

    // Restart-counter baseline (RFC-0087 §2 persistence): read init's
    // record BEFORE the storm; a non-zero baseline means this is the second
    // keep-blk boot — the cross-boot persistence proof.
    let count_before = read_restart_count();
    if count_before > 0 {
        emit_line(crate::markers::M_SELFTEST_CRASH_LOOP_PERSIST_OK);
    }

    // Restart storm: CYCLES identical crash→re-resolve→ping rounds. Slot
    // hygiene is part of the proof — the client closes its previous SEND
    // cap once a re-resolve hands out a fresh one (a client that hoards
    // dead-endpoint caps is the drift the resource discipline forbids).
    let mut prev_send: Option<u32> = None;
    for _cycle in 0..CYCLES {
        let Some((send_slot, recv_slot)) = resolve_pinched_budgeted() else {
            emit_line(crate::markers::M_SELFTEST_SERVICE_RESTART_FAIL);
            return;
        };
        if let Some(old) = prev_send {
            if old != send_slot {
                let _ = nexus_abi::cap_close(old);
            }
        }
        prev_send = Some(send_slot);
        let Ok(client) = KernelClient::new_with_slots(send_slot, recv_slot) else {
            emit_line(crate::markers::M_SELFTEST_SERVICE_RESTART_FAIL);
            return;
        };
        let crash = [b'P', b'N', 1, OP_SELFTEST_CRASH];
        if client.send(&crash, IpcWait::Timeout(core::time::Duration::from_millis(500))).is_err() {
            emit_line(crate::markers::M_SELFTEST_SERVICE_RESTART_FAIL);
            return;
        }
        // Wait for the SUPERVISED comeback: re-resolve (STALE/dead window
        // retries inside the budget helper) and ping until the NEW
        // instance answers the deterministic MALFORMED echo.
        let start = nexus_abi::nsec().unwrap_or(0);
        let mut revived = false;
        while nexus_abi::nsec().unwrap_or(u64::MAX).saturating_sub(start) <= DEADLINE_NS {
            let Some((send_slot, recv_slot)) = resolve_pinched_budgeted() else {
                let _ = nexus_abi::yield_();
                continue;
            };
            if prev_send != Some(send_slot) {
                if let Some(old) = prev_send {
                    let _ = nexus_abi::cap_close(old);
                }
                prev_send = Some(send_slot);
            }
            let Ok(client) = KernelClient::new_with_slots(send_slot, recv_slot) else {
                let _ = nexus_abi::yield_();
                continue;
            };
            let ping = [b'P', b'N', 1, OP_PING_UNKNOWN];
            if client.send(&ping, IpcWait::Timeout(core::time::Duration::from_millis(500))).is_err()
            {
                let _ = nexus_abi::yield_();
                continue;
            }
            for _ in 0..16 {
                match client.recv(IpcWait::Timeout(core::time::Duration::from_millis(500))) {
                    Ok(rsp) => {
                        if rsp.len() == 5
                            && rsp[0] == b'P'
                            && rsp[1] == b'N'
                            && rsp[3] == OP_PING_UNKNOWN | 0x80
                        {
                            revived = true;
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            if revived {
                break;
            }
            let _ = nexus_abi::yield_();
        }
        if !revived {
            emit_line(crate::markers::M_SELFTEST_SERVICE_RESTART_FAIL);
            return;
        }
    }
    emit_line(crate::markers::M_SELFTEST_SERVICE_RESTART_OK);

    // Counter assert: init must have persisted exactly CYCLES bumps.
    let count_after = read_restart_count();
    if count_after == count_before + CYCLES {
        emit_line(crate::markers::M_SELFTEST_CRASH_LOOP_COUNT_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_CRASH_LOOP_COUNT_FAIL);
    }
}

/// Bounded nonce-correlated pinched route resolve over the init responder.
fn resolve_pinched_budgeted() -> Option<(u32, u32)> {
    use nexus_ipc::budget::{self, NonceMismatchBudget, RouteRetryOutcome};
    match budget::route_with_nonce_budgeted(
        b"pinched",
        1,
        2,
        core::time::Duration::from_secs(2),
        NonceMismatchBudget::new(64),
    ) {
        RouteRetryOutcome::Success { send_slot, recv_slot } => Some((send_slot, recv_slot)),
        _ => None,
    }
}

/// Reads init's persisted restart counter for pinched (0 when absent or on
/// any wire trouble — the count assert then fails loudly downstream).
fn read_restart_count() -> u32 {
    use crate::os_lite::services::statefs::statefs_send_recv;
    use statefs::protocol as proto;
    let Ok(client) = crate::os_lite::ipc::routing::route_with_retry("statefsd") else {
        return 0;
    };
    let Ok(req) = proto::encode_key_only_request(proto::OP_GET, "/state/init/restarts/pinched")
    else {
        return 0;
    };
    let Ok(rsp) = statefs_send_recv(&client, &req) else {
        return 0;
    };
    proto::decode_get_response(&rsp)
        .ok()
        .filter(|v| v.len() == 4)
        .map(|v| u32::from_le_bytes([v[0], v[1], v[2], v[3]]))
        .unwrap_or(0)
}
