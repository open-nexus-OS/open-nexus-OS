// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: os-lite runtime backend for the imed daemon (RFC-0075 Phase 1).
//! OWNERS: @ui
//! PUBLIC API: service_main_loop()
//! DEPENDS_ON: nexus-abi IPC syscalls, nexus-ipc transports, imed::ImedCore
//! INVARIANTS:
//! - ready marker emits once, only after route/server are armed
//! - OP_KEY accepted ONLY from inputd, OP_SET_FOCUS ONLY from windowd
//!   (kernel sender_service_id — never payload identity)
//! - typed text NEVER appears in log lines or markers
//! - fixed frame buffers; no per-key allocation in the loop

use core::sync::atomic::{AtomicBool, Ordering};
use core::time::Duration;

use nexus_abi::{debug_putc, yield_};
use nexus_ipc::budget::{self, NonceMismatchBudget, RouteRetryOutcome};
use nexus_ipc::{Client as _, KernelClient, KernelServer, Server as _, Wait};
use nexus_wire::imed as wire;

use crate::ImedCore;

static READY_MARKER_EMITTED: AtomicBool = AtomicBool::new(false);
static FOREIGN_KEY_REJECT_EMITTED: AtomicBool = AtomicBool::new(false);

/// Errors returned by the imed service loop.
#[derive(Debug)]
pub enum ImedError {
    Ipc(&'static str),
}

/// Main imed service loop: serve the imed wire protocol, compose keys for
/// the focused surface, push commits/actions to windowd.
pub fn service_main_loop() -> Result<(), ImedError> {
    let server = match route_imed_blocking() {
        Some(v) => v,
        None => return Err(ImedError::Ipc("route failed")),
    };
    if !READY_MARKER_EMITTED.swap(true, Ordering::Relaxed) {
        emit_line(crate::READY_MARKER);
    }

    let inputd_sid = nexus_abi::service_id_from_name(b"inputd");
    let windowd_sid = nexus_abi::service_id_from_name(b"windowd");
    let mut core = ImedCore::new();
    let mut windowd: Option<KernelClient> = None;
    let mut frame_buf = [0u8; 512];

    nexus_abi::service_verdict_flush("imed");
    loop {
        match server.recv_request_with_meta_into(Wait::Blocking, &mut frame_buf) {
            Ok((len, sender_sid, reply)) => {
                let frame = &frame_buf[..len];
                let status = handle_frame(
                    &mut core,
                    &mut windowd,
                    sender_sid,
                    inputd_sid,
                    windowd_sid,
                    frame,
                );
                // Push senders fire-and-forget: OK produces no reply. Rejects
                // (DENIED/MALFORMED) answer on the reply cap when attached,
                // else non-blocking on the shared response endpoint — the
                // negative selftest reads its verdict there; a full queue
                // drops the reply (bounded, never wedges the serve loop).
                if let Some((op, status)) = status {
                    let rsp = wire::encode_response(op, status);
                    if let Some(reply) = reply {
                        let _ = reply.reply_and_close(&rsp);
                    } else if status != wire::STATUS_OK {
                        let _ = server.send(&rsp, Wait::NonBlocking);
                    }
                }
            }
            Err(nexus_ipc::IpcError::WouldBlock) | Err(nexus_ipc::IpcError::Timeout) => {
                let _ = yield_();
            }
            Err(nexus_ipc::IpcError::Disconnected) => return Err(ImedError::Ipc("disconnected")),
            Err(_) => return Err(ImedError::Ipc("recv")),
        }
    }
}

/// Dispatches one frame; returns `(op, status)` for an optional reply.
fn handle_frame(
    core: &mut ImedCore,
    windowd: &mut Option<KernelClient>,
    sender_sid: u64,
    inputd_sid: u64,
    windowd_sid: u64,
    frame: &[u8],
) -> Option<(u8, u8)> {
    if frame.len() < 4 || frame[0] != wire::MAGIC0 || frame[1] != wire::MAGIC1 {
        return None; // not ours — drop, no state change
    }
    let op = frame[3];
    match op {
        wire::OP_SET_FOCUS => {
            if sender_sid != windowd_sid {
                return Some((op, wire::STATUS_DENIED));
            }
            let Some((surface_id, focused, field_kind, _x, _y, _w, _h)) =
                wire::decode_set_focus(frame)
            else {
                return Some((op, wire::STATUS_MALFORMED));
            };
            core.set_focus(surface_id, focused != 0, field_kind);
            Some((op, wire::STATUS_OK))
        }
        wire::OP_KEY => {
            // Phase 1: hardware chain only. OSK-sourced keys (source=osk from
            // the vetted ime-ui host) arrive with RFC-0075 Phase 2.
            if sender_sid != inputd_sid {
                if !FOREIGN_KEY_REJECT_EMITTED.swap(true, Ordering::Relaxed) {
                    emit_line("imed: reject foreign key source");
                }
                return Some((op, wire::STATUS_DENIED));
            }
            let Some((source, kind, ch, action, _modifiers)) = wire::decode_key(frame) else {
                return Some((op, wire::STATUS_MALFORMED));
            };
            if source != wire::KEY_SOURCE_HW {
                return Some((op, wire::STATUS_DENIED));
            }
            if let Some(pushes) = core.key(kind, ch, action) {
                push_to_windowd(windowd, &pushes);
            }
            Some((op, wire::STATUS_OK))
        }
        wire::OP_CANDIDATE_SELECT => {
            if sender_sid != windowd_sid {
                return Some((op, wire::STATUS_DENIED));
            }
            // Candidates arrive with the CJK engines (RFC-0075 Phase 3).
            Some((op, wire::STATUS_UNSUPPORTED))
        }
        _ => Some((op, wire::STATUS_MALFORMED)),
    }
}

fn push_to_windowd(windowd: &mut Option<KernelClient>, pushes: &crate::KeyPushes) {
    if windowd.is_none() {
        *windowd = KernelClient::new_for("windowd").ok();
    }
    let Some(client) = windowd.as_ref() else {
        return;
    };
    let mut sent_ok = true;
    if let Some(commit) = pushes.commit {
        let mut buf = [0u8; 96];
        if let Some(n) = wire::encode_commit(pushes.surface_id, commit.as_str(), &mut buf) {
            sent_ok &= client.send(&buf[..n], Wait::NonBlocking).is_ok();
        }
    }
    if let Some(action) = pushes.action {
        let frame = wire::encode_action(pushes.surface_id, action);
        sent_ok &= client.send(&frame, Wait::NonBlocking).is_ok();
    }
    if !sent_ok {
        // Drop the cached client; the next push re-routes (windowd restart).
        *windowd = None;
    }
}

fn route_imed_blocking() -> Option<KernelServer> {
    if let Some((send_slot, recv_slot)) = route_blocking(b"imed") {
        return KernelServer::new_with_slots(recv_slot, send_slot).ok();
    }
    // Routing budget expired (slow boots) — fall back to the deterministic
    // slots init wires via cap_transfer (recv=3, send=4; timed pattern).
    emit_line("imed: route fallback slots");
    KernelServer::new_with_slots(IMED_RECV_SLOT, IMED_SEND_SLOT).ok()
}

/// Deterministic slots wired by init's cap_transfer for imed (recv first →
/// slot 3, send second → slot 4; same order as timed/metricsd).
const IMED_RECV_SLOT: u32 = 0x03;
const IMED_SEND_SLOT: u32 = 0x04;

fn route_blocking(name: &[u8]) -> Option<(u32, u32)> {
    const CTRL_SEND_SLOT: u32 = 1;
    const CTRL_RECV_SLOT: u32 = 2;
    match budget::route_with_nonce_budgeted(
        name,
        CTRL_SEND_SLOT,
        CTRL_RECV_SLOT,
        Duration::from_secs(2),
        NonceMismatchBudget::new(64),
    ) {
        RouteRetryOutcome::Success { send_slot, recv_slot } => Some((send_slot, recv_slot)),
        _ => None,
    }
}

fn emit_line(message: &str) {
    if nexus_abi::service_line(message.as_bytes()) {
        return;
    }
    for byte in message.as_bytes().iter().copied().chain(core::iter::once(b'\n')) {
        let _ = debug_putc(byte);
    }
}
