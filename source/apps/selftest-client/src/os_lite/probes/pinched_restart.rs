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

use crate::markers::emit_line;

/// TASK-0049B PR-B3b: real-service restart E2E (ADR-0057, standing —
/// ADR-0048 doctrine). Crashes pinched via the identity-gated
/// OP_SELFTEST_CRASH, then re-resolves the route — the ask parks until init's
/// supervision has restarted and re-provisioned it — and proves the NEW instance
/// answers by pinging an unknown op (deterministic MALFORMED echo). No yield and no
/// deadline: every wait ends by an answer or a death.
pub(crate) fn restart_proof() {
    const OP_SELFTEST_CRASH: u8 = 2;
    const OP_PING_UNKNOWN: u8 = 0x7e;
    const CYCLES: u32 = 3;

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
        let Some(send_slot) = resolve_pinched(&mut prev_send) else {
            emit_line(crate::markers::M_SELFTEST_SERVICE_RESTART_FAIL);
            return;
        };
        // The crash request is an exchange like any other: its ack, or the instance's death
        // before it answered — both mean the order arrived, neither is waited for by a clock.
        let crash = [b'P', b'N', 1, OP_SELFTEST_CRASH];
        let _ = exchange(send_slot, &crash, OP_SELFTEST_CRASH);
        // The SUPERVISED comeback, by events only: init parks a route ask for a service it
        // knows is down and answers it when the new instance stands; a ping is answered by the
        // new instance or ended by the death of the one it reached (the old one, when the ask
        // overtook init's notice of the exit — the board, 2026-11-11: that ping went into a
        // queue that died with it, and a receive without an end waited on it for good). A
        // refused ask (the supervisor gave the service up) is the failure, said at once.
        loop {
            let Some(send_slot) = resolve_pinched(&mut prev_send) else {
                emit_line(crate::markers::M_SELFTEST_SERVICE_RESTART_FAIL);
                return;
            };
            let ping = [b'P', b'N', 1, OP_PING_UNKNOWN];
            if exchange(send_slot, &ping, OP_PING_UNKNOWN) {
                break;
            }
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

/// One request to pinched answered on the harness's reply inbox: `true` when the frame carrying
/// `op`'s response arrived, `false` when the instance died first (or was already gone). No
/// clock: the answer or the peer's death ends the wait (`nexus_ipc::exchange`).
fn exchange(send_slot: u32, frame: &[u8], op: u8) -> bool {
    let reply = nexus_service_topology::slots::selftest_client::REPLY;
    let mut rsp = [0u8; 16];
    nexus_ipc::exchange::call_matching(send_slot, reply, frame, &mut rsp, |rsp| {
        (rsp.len() == 5 && rsp[0] == b'P' && rsp[1] == b'N' && rsp[3] == op | 0x80).then_some(())
    })
    .is_ok()
}

/// Asks init for the route to pinched (the ask parks while the service is down) and keeps the
/// client's table clean: the previous SEND capability is closed once a fresh one is handed
/// out — a client hoarding capabilities to dead endpoints is the drift the proof forbids.
fn resolve_pinched(prev_send: &mut Option<u32>) -> Option<u32> {
    use nexus_ipc::budget::{self, NonceMismatchBudget, RouteRetryOutcome};
    let RouteRetryOutcome::Success { send_slot, .. } =
        budget::route_with_nonce(b"pinched", NonceMismatchBudget::new(64))
    else {
        return None;
    };
    if let Some(old) = prev_send.replace(send_slot) {
        if old != send_slot {
            let _ = nexus_abi::cap_close(old);
        }
    }
    Some(send_slot)
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
