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
    pending: &mut nexus_ipc::reqrep::FrameStash<8, 16>,
    boot_req: Option<u32>,
    reply_send: u32,
    reply_recv: u32,
    bnd_req: u32,
    init_misc: &mut nexus_event::SpanTally,
    init_fold: bool,
) -> crate::boot_graph::BootGraph {
    let mut graph = crate::boot_graph::BootGraph::Normal;
    match boot_req
        .map_or(Ok((None, None)), |b| bootctld_boot_attempt(pending, b, reply_send, reply_recv))
    {
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
                let ok = bundlemgrd_set_active_slot(pending, bnd_req, reply_send, reply_recv, slot);
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
    pending: &mut nexus_ipc::reqrep::FrameStash<8, 16>,
    boot_req: u32,
    reply_send: u32,
    reply_recv: u32,
) -> Result<(Option<u8>, Option<u8>)> {
    let req = [b'B', b'T', 1u8, 5u8]; // wire v1, OP_BOOT_ATTEMPT
                                      // Decode → (status, rolled_back_slot|0, next_boot|0xff): the one-shot
                                      // target rides the SAME persisted commit as the attempt ack (§4).
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
    let mut attempts = 0u8;
    let max_attempts: u8 = 20;
    loop {
        attempts = attempts.saturating_add(1);
        let reply_send_clone = nexus_abi::cap_clone(reply_send).map_err(InitError::Abi)?;
        let hdr = nexus_abi::MsgHeader::new(
            reply_send_clone,
            0,
            0,
            nexus_abi::ipc_hdr::CAP_MOVE,
            req.len() as u32,
        );
        let deadline = match nexus_abi::nsec() {
            Ok(now) => now.saturating_add(500_000_000),
            Err(_) => 0,
        };
        let send = nexus_abi::ipc_send_v1(boot_req, &hdr, &req, 0, deadline);
        if send.is_err() {
            if attempts < max_attempts {
                let _ = nexus_abi::yield_();
                continue;
            }
            return Ok((None, None));
        }

        let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 16];
        loop {
            // Deterministic shared-inbox handling: first consume any previously stashed replies.
            if let Some(n) = pending.take_into_where(&mut buf, |f| decode(f).is_some()) {
                if let Some((status, slot, next)) = decode(&buf[..n]) {
                    if status != 0 {
                        return Err(InitError::Map("bootctld boot attempt failed"));
                    }
                    let rolled = if slot == 0 { None } else { Some(slot) };
                    let next = if next == 0xff { None } else { Some(next) };
                    return Ok((rolled, next));
                }
            }
            // Soft-real-time boot: BLOCK on the shared reply inbox until a frame arrives or the
            // deadline — no busy-poll (see the pre-relocation rationale in git history).
            match nexus_abi::ipc_recv_v1(
                reply_recv,
                &mut rh,
                &mut buf,
                nexus_abi::IPC_SYS_TRUNCATE,
                deadline,
            ) {
                Ok(n) => {
                    let n = core::cmp::min(n as usize, buf.len());
                    if let Some((status, slot, next)) = decode(&buf[..n]) {
                        if status != 0 {
                            return Err(InitError::Map("bootctld boot attempt failed"));
                        }
                        let rolled = if slot == 0 { None } else { Some(slot) };
                        let next = if next == 0xff { None } else { Some(next) };
                        return Ok((rolled, next));
                    }
                    // Stash unrelated replies deterministically for the next consumer of this inbox.
                    let _ = pending.push(&buf[..n]);
                    continue;
                }
                // Deadline hit (or transient error): fall out to the outer attempt loop.
                Err(_) => break,
            }
        }
        if attempts < max_attempts {
            let _ = nexus_abi::yield_();
            continue;
        }
        // bootctld not ready yet; skip the boot attempt for this cycle.
        return Ok((None, None));
    }
}
