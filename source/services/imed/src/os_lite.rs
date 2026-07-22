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
//! - OSK keys arrive ONLY on the dedicated `imed-osk` endpoint (RFC-0075
//!   Phase 2): possession of that route cap IS the authorization (init
//!   wires it to imed; execd provisions it only to `nexus.permission.IME`
//!   bundles). `source=osk` on the MAIN endpoint stays DENIED.
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

/// Main imed service loop: serve the imed wire protocol on the MAIN endpoint
/// (inputd/windowd) and the dedicated OSK endpoint (ime-ui route holders),
/// multiplexed by a kernel waitset; compose keys for the focused surface,
/// push commits/actions to windowd.
pub fn service_main_loop() -> Result<(), ImedError> {
    let (server, main_recv_slot) = match route_imed_blocking() {
        Some(v) => v,
        None => return Err(ImedError::Ipc("route failed")),
    };
    // OSK endpoint: init transfers the RECV half to the fixed slot. Absent
    // slot = OSK disabled (older init) — the hw chain must keep working.
    let osk = KernelServer::new_with_slots(OSK_RECV_SLOT, IMED_SEND_SLOT).ok();
    let Some(waitset) = build_waitset(main_recv_slot, osk.is_some()) else {
        emit_line("imed: FAIL waitset");
        return Err(ImedError::Ipc("waitset"));
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
        let member = match nexus_abi::waitset_wait(waitset, 0) {
            Ok(m) => m,
            Err(_) => {
                let _ = yield_();
                continue;
            }
        };
        if member == 0 {
            drain_main(&server, &mut core, &mut windowd, inputd_sid, windowd_sid, &mut frame_buf);
        } else if let Some(osk_server) = osk.as_ref() {
            drain_osk(osk_server, &mut core, &mut windowd, &mut frame_buf);
        }
    }
}

/// Waitset over the main RECV (member 0) and, when wired, the OSK RECV
/// (member 1). `None` on any syscall failure (fail-loud at boot).
fn build_waitset(main_recv_slot: u32, osk_wired: bool) -> Option<nexus_abi::Cap> {
    let ws = nexus_abi::waitset_create().ok()?;
    nexus_abi::waitset_add(ws, main_recv_slot).ok()?;
    if osk_wired {
        nexus_abi::waitset_add(ws, OSK_RECV_SLOT).ok()?;
    }
    Some(ws)
}

/// Drains the MAIN endpoint (inputd hw keys, windowd focus) until empty.
fn drain_main(
    server: &KernelServer,
    core: &mut ImedCore,
    windowd: &mut Option<KernelClient>,
    inputd_sid: u64,
    windowd_sid: u64,
    frame_buf: &mut [u8; 512],
) {
    loop {
        match server.recv_request_with_meta_into(Wait::NonBlocking, frame_buf) {
            Ok((len, sender_sid, reply)) => {
                let frame = &frame_buf[..len];
                let status =
                    handle_frame(core, windowd, sender_sid, inputd_sid, windowd_sid, frame);
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
            Err(_) => return,
        }
    }
}

/// Drains the OSK endpoint until empty. Possession of the route cap IS the
/// authorization (RFC-0075 Phase 2) — no sender gate; but ONLY `OP_KEY`
/// with `source=osk` is meaningful here. Replies ride ONLY an attached
/// reply cap (the selftest probe); fire-and-forget OSK taps get none.
fn drain_osk(
    server: &KernelServer,
    core: &mut ImedCore,
    windowd: &mut Option<KernelClient>,
    frame_buf: &mut [u8; 512],
) {
    loop {
        match server.recv_request_with_meta_into(Wait::NonBlocking, frame_buf) {
            Ok((len, _sender_sid, reply)) => {
                let frame = &frame_buf[..len];
                let outcome = handle_osk_frame(core, windowd, frame);
                if let (Some((op, status, echo)), Some(reply)) = (outcome, reply) {
                    let mut rsp = [0u8; 96];
                    if let Some(n) =
                        wire::encode_osk_reply(op, status, echo.commit.as_str(), &mut rsp)
                    {
                        let _ = reply.reply_and_close(&rsp[..n]);
                    }
                }
            }
            Err(_) => return,
        }
    }
}

/// Dispatches one OSK-endpoint frame; returns `(op, status, echo)` for an
/// optional reply. COMPOSITION is focus-independent (RFC-0075 Phase 3 —
/// the deterministic probe exercises the real engine without a field);
/// DELIVERY stays focus-gated inside `ImedCore`. The echo carries the
/// commit this step produced back to the INJECTING sender only.
fn handle_osk_frame(
    core: &mut ImedCore,
    windowd: &mut Option<KernelClient>,
    frame: &[u8],
) -> Option<(u8, u8, crate::StepEcho)> {
    let empty = crate::StepEcho { commit: crate::CommitText::default() };
    if frame.len() < 4 || frame[0] != wire::MAGIC0 || frame[1] != wire::MAGIC1 {
        return None;
    }
    let op = frame[3];
    match op {
        wire::OP_KEY => {
            let Some((source, kind, ch, action, _modifiers)) = wire::decode_key(frame) else {
                return Some((op, wire::STATUS_MALFORMED, empty));
            };
            if source != wire::KEY_SOURCE_OSK {
                return Some((op, wire::STATUS_DENIED, empty));
            }
            let (pushes, echo) = core.key(kind, ch, action);
            if let Some(pushes) = pushes {
                push_to_windowd(windowd, &pushes);
            }
            Some((op, wire::STATUS_OK, echo))
        }
        wire::OP_SET_LAYOUT => {
            // The OSK globe key switches the engine (capability-gated).
            let Some(layout) = wire::decode_set_layout(frame) else {
                return Some((op, wire::STATUS_MALFORMED, empty));
            };
            core.set_layout(layout);
            Some((op, wire::STATUS_OK, empty))
        }
        wire::OP_CANDIDATE_SELECT => {
            let Some(index) = wire::decode_candidate_select(frame) else {
                return Some((op, wire::STATUS_MALFORMED, empty));
            };
            let (pushes, echo) = core.candidate_select(usize::from(index));
            if let Some(pushes) = pushes {
                push_to_windowd(windowd, &pushes);
            }
            Some((op, wire::STATUS_OK, echo))
        }
        _ => Some((op, wire::STATUS_DENIED, empty)),
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
            // Hardware chain only on the MAIN endpoint; OSK-sourced keys
            // ride the DEDICATED osk endpoint (capability = authorization).
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
            let (pushes, _echo) = core.key(kind, ch, action);
            if let Some(pushes) = pushes {
                push_to_windowd(windowd, &pushes);
            }
            Some((op, wire::STATUS_OK))
        }
        wire::OP_SET_LAYOUT => {
            // Engine follows `input.keymap` — relayed by inputd only.
            if sender_sid != inputd_sid {
                return Some((op, wire::STATUS_DENIED));
            }
            let Some(layout) = wire::decode_set_layout(frame) else {
                return Some((op, wire::STATUS_MALFORMED));
            };
            core.set_layout(layout);
            Some((op, wire::STATUS_OK))
        }
        wire::OP_CANDIDATE_SELECT => {
            // windowd relays UI selection (RFC-0075 Phase 3).
            if sender_sid != windowd_sid {
                return Some((op, wire::STATUS_DENIED));
            }
            let Some(index) = wire::decode_candidate_select(frame) else {
                return Some((op, wire::STATUS_MALFORMED));
            };
            let (pushes, _echo) = core.candidate_select(usize::from(index));
            if let Some(pushes) = pushes {
                push_to_windowd(windowd, &pushes);
            }
            Some((op, wire::STATUS_OK))
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
    // Strip snapshots (RFC-0075 Phase 3): preedit + candidate page ride to
    // windowd, which relays them to the ime-ui overlay (never the app).
    if let Some(preedit) = pushes.preedit {
        let mut buf = [0u8; 96];
        if let Some(n) = wire::encode_preedit(pushes.surface_id, 0, preedit.as_str(), &mut buf) {
            sent_ok &= client.send(&buf[..n], Wait::NonBlocking).is_ok();
        }
    }
    if let Some(page) = pushes.candidates {
        let mut texts: [&str; wire::CANDIDATES_MAX] = [""; wire::CANDIDATES_MAX];
        let count = page.len().min(wire::CANDIDATES_MAX);
        for (i, slot) in texts.iter_mut().enumerate().take(count) {
            if let Some(c) = page.get(i) {
                *slot = c.as_str();
            }
        }
        let mut list = [0u8; wire::CANDIDATE_LIST_MAX_BYTES];
        if let Some(list_len) = wire::encode_candidate_list(&texts[..count], &mut list) {
            let mut buf = [0u8; 512];
            if let Some(n) = wire::encode_candidates(
                pushes.surface_id,
                page.page,
                count as u8,
                &list[..list_len],
                &mut buf,
            ) {
                sent_ok &= client.send(&buf[..n], Wait::NonBlocking).is_ok();
            }
        }
    }
    if !sent_ok {
        // Drop the cached client; the next push re-routes (windowd restart).
        *windowd = None;
    }
}

fn route_imed_blocking() -> Option<(KernelServer, u32)> {
    if let Some((send_slot, recv_slot)) = route_blocking(b"imed") {
        return KernelServer::new_with_slots(recv_slot, send_slot).ok().map(|s| (s, recv_slot));
    }
    // Routing budget expired (slow boots) — fall back to the deterministic
    // slots init wires via cap_transfer (recv=3, send=4; timed pattern).
    emit_line("imed: route fallback slots");
    KernelServer::new_with_slots(IMED_RECV_SLOT, IMED_SEND_SLOT).ok().map(|s| (s, IMED_RECV_SLOT))
}

/// Deterministic slots wired by init's cap_transfer for imed (recv first →
/// slot 3, send second → slot 4; same order as timed/metricsd).
const IMED_RECV_SLOT: u32 = 0x03;
const IMED_SEND_SLOT: u32 = 0x04;
/// The dedicated OSK endpoint's RECV half (init cap_transfer, third leg;
/// RFC-0075 Phase 2). Absent when init predates the OSK wiring.
const OSK_RECV_SLOT: u32 = 0x05;

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
