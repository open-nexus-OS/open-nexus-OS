// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: init's early boot-attempt handshake against bootctld
//! (TASK-0050 PR-2..4; split from helpers/orchestrator under the structure
//! ratchet). Ticks the attempt counter at the boot-state authority over
//! the pre-minted init-owned endpoint (the responder is not serving yet),
//! applies a rollback to bundlemgrd, and announces the one-shot next-boot
//! target the ack consumed (RFC-0087 §4 — cleared in the same persisted
//! commit; PR-5 materializes it as a stage graph).
//! OWNERS: @reliability @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU reset lane (`SELFTEST: boot target roundtrip ok`)
//!   + OTA ladder (rollback path).
//! ADR: docs/adr/0055-bootctld-single-boot-state-authority.md

use crate::bootstrap::diag::il;
use crate::os_payload::*;

/// Runs the handshake, applies its outcome and returns the RESOLVED boot
/// graph (the consumed one-shot target, RFC-0087 §4). Fail-open to
/// `Normal` on any handshake trouble — a broken handshake must never
/// brick the normal boot; a reduced graph is only ever entered on a
/// successfully committed consumption.
#[allow(clippy::too_many_arguments)]
pub(crate) fn boot_attempt_handshake(
    boot_req: Option<u32>,
    ask: nexus_ipc::SlotPair,
    bnd_req: u32,
    init_misc: &mut nexus_event::SpanTally,
    init_fold: bool,
) -> crate::boot_graph::BootGraph {
    let mut graph = crate::boot_graph::BootGraph::Normal;
    match boot_req.map_or(Ok((None, None)), |b| bootctld_boot_attempt(b, ask)) {
        Ok((rolled_back, next_boot)) => {
            // One-shot target consumed WITH the attempt ack (RFC-0087 §4).
            if let Some(target) = next_boot {
                if let Some(resolved) = crate::boot_graph::BootGraph::from_wire(target) {
                    graph = resolved;
                }
                crate::bootstrap::diag::emit_marker_atomic(
                    &[b"init: next boot target=", graph.label().as_bytes()],
                    None,
                );
            }
            if let Some(slot) = rolled_back {
                let ok = bundlemgrd_set_active_slot(bnd_req, ask, slot);
                if !ok && il(init_misc, init_fold, "init") {
                    debug_write_str("init: rollback deferred");
                    debug_write_byte(b'\n');
                }
            }
        }
        Err(_) => {
            debug_write_str("init: boot attempt fail");
            debug_write_byte(b'\n');
        }
    }
    graph
}

/// Boot-attempt handshake against bootctld (TASK-0050 PR-2, ADR-0055):
/// init ticks the attempt counter DIRECTLY at the boot-state authority —
/// over the pre-minted init-owned request endpoint, because the responder
/// is not serving yet (a route-resolving path here would deadlock on
/// ourselves). Reply payload `[rolled_back_slot|0, next_boot|0xff]`; the
/// one-shot consumption rides the same persisted commit (RFC-0087 §4).
pub(crate) fn bootctld_boot_attempt(
    boot_req: u32,
    ask: nexus_ipc::SlotPair,
) -> Result<(Option<u8>, Option<u8>)> {
    let req = [b'B', b'T', 1u8, 5u8]; // wire v1, OP_BOOT_ATTEMPT
    let decode = |frame: &[u8]| -> Option<(u8, u8, u8)> {
        if frame.len() >= 7
            && frame[0] == b'B'
            && frame[1] == b'T'
            && frame[2] == 1
            && frame[3] == (5 | 0x80)
        {
            let rolled_back = frame.get(7).copied().unwrap_or(0);
            let next_boot = frame.get(8).copied().unwrap_or(0xff);
            return Some((frame[4], rolled_back, next_boot));
        }
        None
    };
    // ONE waited exchange (TASK-0324 P8): a reply-SEND clone rides the request, the send waits
    // for queue space, the receive for bootctld's answer or its death (EOF on init's ask
    // inbox). The 20 × 500 ms attempt cadence is gone with the clock; the stash that used to
    // park foreign frames went with the shared inbox (TASK-0054C P2-d).
    let mut buf = [0u8; 16];
    match nexus_ipc::exchange::call_matching(boot_req, ask, &req, &mut buf, decode) {
        Ok(answer) => finish_boot_attempt(Some(answer)),
        Err(_) => Err(InitError::Map("bootctld boot attempt unreachable")),
    }
}

/// The decoded boot-attempt answer → `(rolled_back slot, next boot)`; a non-zero status is
/// the honest failure.
fn finish_boot_attempt(decoded: Option<(u8, u8, u8)>) -> Result<(Option<u8>, Option<u8>)> {
    let Some((status, slot, next)) = decoded else {
        return Ok((None, None));
    };
    if status != 0 {
        return Err(InitError::Map("bootctld boot attempt failed"));
    }
    let rolled = if slot == 0 { None } else { Some(slot) };
    let next = if next == 0xff { None } else { Some(next) };
    Ok((rolled, next))
}
