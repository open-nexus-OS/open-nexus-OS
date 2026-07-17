// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: OS-lite backend for abilitymgr — the broker service loop, live
//! registry probe, and manifest-caps startup self-check (RFC-0065).
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests (OS service loop; broker/wire/caps logic host-tested in their modules)
//!
//! OS-lite backend for abilitymgr — the ability-lifecycle broker service loop.
//!
//! Routes its endpoint, receives request frames, drives the pure [`Broker`] via
//! [`wire::dispatch`], and emits deterministic `abilitymgr: …` markers. The live
//! resolve-via-bundlemgrd + spawn-via-execd + windowd surface bind is wired in P3.

use core::time::Duration;

use nexus_abi::{debug_putc, yield_};
use nexus_ipc::budget::{self, NonceMismatchBudget, RouteRetryOutcome};
use nexus_ipc::{Client as _, KernelClient, KernelServer, Server as _, Wait};

use crate::lifecycle::{AbilityState, Broker};
use crate::wire::{dispatch, Event};

/// Result type for abilitymgr OS operations.
pub type AbilitymgrResult<T> = Result<T, AbilitymgrError>;

/// Errors from the abilitymgr service.
#[derive(Debug)]
pub enum AbilitymgrError {
    /// IPC error.
    Ipc(&'static str),
}

impl core::fmt::Display for AbilitymgrError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Ipc(msg) => write!(f, "ipc: {}", msg),
        }
    }
}

/// Notifies init once the service reports readiness.
pub struct ReadyNotifier(alloc::boxed::Box<dyn FnOnce() + Send>);

impl ReadyNotifier {
    /// Creates a notifier from the provided closure.
    pub fn new<F>(func: F) -> Self
    where
        F: FnOnce() + Send + 'static,
    {
        Self(alloc::boxed::Box::new(func))
    }

    /// Signals readiness to the caller.
    pub fn notify(self) {
        (self.0)();
    }
}

/// Deterministic slots wired by init's cap_transfer for abilitymgr
/// (recv first → 3, send second → 4).
const ABILITYMGR_RECV_SLOT: u32 = 0x03;
const ABILITYMGR_SEND_SLOT: u32 = 0x04;

/// Main service loop for abilitymgr.
pub fn service_main_loop(notifier: ReadyNotifier) -> AbilitymgrResult<()> {
    notifier.notify();
    emit_line("abilitymgr: ready");

    // RFC-0065: prove the live resolve hop — ask the registry (bundlemgrd) for the
    // installed app list. Best-effort: any failure just logs and is non-fatal.
    probe_registry();

    // RFC-0065 launch authority: validate each installed app's manifest-declared
    // capabilities against the known permission set at startup, so a bad manifest
    // is caught here (a clear marker) rather than silently at launch.
    validate_manifest_caps();

    let server = route_abilitymgr_blocking().ok_or(AbilitymgrError::Ipc("route failed"))?;

    // The broker owns lifecycle state for the life of the service.
    let mut broker = Broker::new();

    nexus_abi::service_verdict_flush("abilitymgr");
    // TASK-0288 sweep: transient errors continue; only a consecutive-error
    // run marks our own endpoint defect (fleet-collapse lesson).
    let mut breaker = nexus_ipc::resilience::CircuitBreaker::new(64, 3);
    loop {
        match server.recv_request_with_meta(Wait::Blocking) {
            Ok((frame, _sender_service_id, reply)) => {
                breaker.on_success();
                // TASK-0065B session gate: OP_LAUNCH is refused until sessiond
                // reports an ACTIVE session. Fail-closed (sessiond unreachable
                // = deny): windowd's greeter gate is UX, THIS is the
                // authority-side enforcement (host-tested in `handoff`).
                let out = if is_launch_request(frame.as_slice())
                    && !launch_target_is_pre_session(frame.as_slice())
                    && !session_gate_active()
                {
                    emit_line("abilitymgr: launch denied (session)");
                    crate::wire::Dispatched { response: launch_denied_response(), event: None }
                } else {
                    dispatch(&mut broker, frame.as_slice())
                };
                if let Some(event) = out.event {
                    emit_event(&event);
                    // RFC-0065 launch chain, spawn hop (TASK-0080D): the
                    // broker approved the launch — abilitymgr (the ONLY
                    // spawner of apps, policy `proc.spawn`) now asks execd
                    // for the process. Failures are markers, never silent.
                    if let crate::wire::Event::Launched { app_id, .. } = &event {
                        spawn_app(app_id);
                    }
                }
                if let Some(reply) = reply {
                    let _ = reply.reply_and_close(&out.response);
                } else {
                    let _ = server.send(&out.response, Wait::Blocking);
                }
            }
            Err(nexus_ipc::IpcError::WouldBlock) | Err(nexus_ipc::IpcError::Timeout) => {
                let _ = yield_();
            }
            Err(nexus_ipc::IpcError::Disconnected) => {
                emit_line("abilitymgr: recv disconnected");
                return Err(AbilitymgrError::Ipc("disconnected"));
            }
            Err(_) => {
                let (should_log, verdict) = breaker.on_error();
                if should_log {
                    emit_line("abilitymgr: recv error (transient)");
                }
                match verdict {
                    nexus_ipc::resilience::BreakerVerdict::Continue => {
                        let _ = yield_();
                    }
                    nexus_ipc::resilience::BreakerVerdict::EndpointDefect => {
                        emit_line("abilitymgr: endpoint defect (consecutive error limit)");
                        return Err(AbilitymgrError::Ipc("recv"));
                    }
                }
            }
        }
    }
}

/// Best-effort: query the registry (`bundlemgrd` `OP_LIST_APPS`) for the installed
/// app list and report the count — the live half of the launch resolve path.
///
/// Uses the production CAP_MOVE request/reply over the init-provisioned
/// `abilitymgr→bundlemgrd` route (RFC-0066 P3): route to bundlemgrd's request
/// endpoint + our `@reply` inbox, move a reply cap so bundlemgrd answers us, then
/// receive the response. Bounded + non-fatal — any failure emits a skip marker and
/// the service continues.
fn probe_registry() {
    let (send_slot, _recv) = match route_blocking(b"bundlemgrd") {
        Some(slots) => slots,
        None => {
            emit_line("abilitymgr: registry unreachable");
            return;
        }
    };
    let (reply_send_slot, reply_recv_slot) = match route_blocking(b"@reply") {
        Some(slots) => slots,
        None => {
            emit_line("abilitymgr: registry no reply inbox");
            return;
        }
    };

    let mut req = [0u8; 4];
    nexus_abi::bundlemgrd::encode_list_apps(&mut req);

    // Move a clone of our reply-send cap into the request so bundlemgrd replies to
    // our @reply inbox (CAP_MOVE).
    let reply_send_clone = match nexus_abi::cap_clone(reply_send_slot) {
        Ok(c) => c,
        Err(_) => {
            emit_line("abilitymgr: registry cap clone fail");
            return;
        }
    };
    let hdr = nexus_abi::MsgHeader::new(
        reply_send_clone,
        0,
        0,
        nexus_abi::ipc_hdr::CAP_MOVE,
        req.len() as u32,
    );

    let start = nexus_abi::nsec().unwrap_or(0);
    let deadline = start.saturating_add(500_000_000); // 500ms bound

    // Send (bounded, non-blocking).
    let mut sent = false;
    let mut spins: u32 = 0;
    loop {
        match nexus_abi::ipc_send_v1(send_slot, &hdr, &req, nexus_abi::IPC_SYS_NONBLOCK, 0) {
            Ok(_) => {
                sent = true;
                break;
            }
            Err(nexus_abi::IpcError::QueueFull) => {
                if nexus_abi::nsec().unwrap_or(0) >= deadline || spins >= 200_000 {
                    break;
                }
                spins = spins.saturating_add(1);
                let _ = yield_();
            }
            Err(_) => break,
        }
    }
    let _ = nexus_abi::cap_close(reply_send_clone);
    if !sent {
        emit_line("abilitymgr: registry send fail");
        return;
    }

    // Receive the reply on our @reply inbox (bounded).
    loop {
        let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 256];
        match nexus_abi::ipc_recv_v1(
            reply_recv_slot,
            &mut rh,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => {
                let n = core::cmp::min(n as usize, buf.len());
                if let Some((status, count)) =
                    nexus_abi::bundlemgrd::decode_list_apps_header(&buf[..n])
                {
                    if status == nexus_abi::bundlemgrd::STATUS_OK {
                        nexus_abi::debug_ts_prefix();
                        emit_prefix(b"abilitymgr: registry ok (n=");
                        emit_u32(count as u32);
                        emit_prefix(b")");
                        emit_newline();
                        return;
                    }
                }
                // Unrelated frame on the shared inbox: keep waiting until deadline.
                if nexus_abi::nsec().unwrap_or(0) >= deadline {
                    emit_line("abilitymgr: registry timeout");
                    return;
                }
                let _ = yield_();
            }
            Err(nexus_abi::IpcError::QueueEmpty) => {
                if nexus_abi::nsec().unwrap_or(0) >= deadline {
                    emit_line("abilitymgr: registry timeout");
                    return;
                }
                let _ = yield_();
            }
            Err(_) => {
                emit_line("abilitymgr: registry recv err");
                return;
            }
        }
    }
}

/// True for a v1 OP_LAUNCH request frame (the only session-gated op).
fn is_launch_request(frame: &[u8]) -> bool {
    frame.len() >= 4
        && frame[0] == crate::protocol::MAGIC0
        && frame[1] == crate::protocol::MAGIC1
        && frame[2] == crate::protocol::VERSION
        && frame[3] == crate::protocol::OP_LAUNCH
}

/// True when the OP_LAUNCH target is a PRE-SESSION role (bundle_type=greeter,
/// build-generated `APP_PRE_SESSION`): the login surface must launch BEFORE a
/// session exists — that is its entire purpose. Declarative via bundle_type;
/// every other launch stays session-gated (fail-closed).
fn launch_target_is_pre_session(frame: &[u8]) -> bool {
    // `[A,M,ver,OP_LAUNCH, app_len:u8, app...]`
    let Some(&app_len) = frame.get(4) else { return false };
    let Some(app) = frame.get(5..5 + app_len as usize) else { return false };
    let Ok(app) = core::str::from_utf8(app) else { return false };
    crate::caps::APP_PRE_SESSION.contains(&app)
}

/// The gate's denial reply: `[A, M, ver, OP_LAUNCH|RESPONSE, STATUS_DENIED]`.
fn launch_denied_response() -> alloc::vec::Vec<u8> {
    alloc::vec![
        crate::protocol::MAGIC0,
        crate::protocol::MAGIC1,
        crate::protocol::VERSION,
        crate::protocol::OP_LAUNCH | crate::protocol::OP_RESPONSE,
        crate::protocol::STATUS_DENIED,
    ]
}

/// Live session gate (TASK-0065B): one bounded GET_STATE query to sessiond per
/// launch request (launches are user-paced — no caching needed in v0).
/// Fail-closed: any routing/transport/decode failure counts as "no session".
fn session_gate_active() -> bool {
    let Some((send_slot, _recv)) = route_blocking(b"sessiond") else {
        emit_line("abilitymgr: session gate unreachable (deny)");
        return false;
    };
    let Some((reply_send_slot, reply_recv_slot)) = route_blocking(b"@reply") else {
        emit_line("abilitymgr: session gate no reply inbox (deny)");
        return false;
    };

    let mut req = [0u8; 4];
    nexus_abi::sessiond::encode_get_state(&mut req);
    let Ok(reply_send_clone) = nexus_abi::cap_clone(reply_send_slot) else {
        return false;
    };
    let hdr = nexus_abi::MsgHeader::new(
        reply_send_clone,
        0,
        0,
        nexus_abi::ipc_hdr::CAP_MOVE,
        req.len() as u32,
    );

    let start = nexus_abi::nsec().unwrap_or(0);
    let deadline = start.saturating_add(500_000_000); // 500ms bound

    let mut sent = false;
    let mut spins: u32 = 0;
    loop {
        match nexus_abi::ipc_send_v1(send_slot, &hdr, &req, nexus_abi::IPC_SYS_NONBLOCK, 0) {
            Ok(_) => {
                sent = true;
                break;
            }
            Err(nexus_abi::IpcError::QueueFull) => {
                if nexus_abi::nsec().unwrap_or(0) >= deadline || spins >= 200_000 {
                    break;
                }
                spins = spins.saturating_add(1);
                let _ = yield_();
            }
            Err(_) => break,
        }
    }
    let _ = nexus_abi::cap_close(reply_send_clone);
    if !sent {
        return false;
    }

    loop {
        let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 512];
        match nexus_abi::ipc_recv_v1(
            reply_recv_slot,
            &mut rh,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => {
                let n = core::cmp::min(n as usize, buf.len());
                if let Some((status, state, _idx, _count)) =
                    nexus_abi::sessiond::decode_get_state_header(&buf[..n])
                {
                    return status == nexus_abi::sessiond::STATUS_OK
                        && state == nexus_abi::sessiond::STATE_ACTIVE;
                }
                if nexus_abi::nsec().unwrap_or(0) >= deadline {
                    return false;
                }
                let _ = yield_();
            }
            Err(nexus_abi::IpcError::QueueEmpty) => {
                if nexus_abi::nsec().unwrap_or(0) >= deadline {
                    return false;
                }
                let _ = yield_();
            }
            Err(_) => return false,
        }
    }
}

/// execd image id of the shared app-host runtime (execd os_lite
/// `IMG_APPHOST`) — the ONE process image every ui-program app runs in; the
/// per-app payload arrives via bundlemgrd GET_PAYLOAD (child VMO slot 7).
const IMG_APPHOST: u8 = 4;

/// app-id → execd image id, manifest-driven: any bundle declaring
/// `payload_kind = "ui-program"` (build-time `APP_UI_PROGRAMS` table from
/// `bundles/<app>/manifest.toml`) spawns the app-host runtime.
fn image_for_app(app_id: &str) -> Option<u8> {
    crate::caps::APP_UI_PROGRAMS.iter().any(|id| *id == app_id).then_some(IMG_APPHOST)
}

/// Requests the process spawn from execd (`OP_EXEC_IMAGE` frame, requester
/// = "abilitymgr"; policyd verifies identity + `proc.spawn`). Bounded reply
/// wait; every failure path is a marker.
fn spawn_app(app_id: &str) {
    let Some(image_id) = image_for_app(app_id) else {
        // Registry apps without a spawnable payload (chat/search placeholder
        // ELFs) launch their windowd-hosted windows instead — by value.
        emit_line("abilitymgr: launch spawn skipped (no payload image)");
        return;
    };
    let Some((send_slot, recv_slot)) = route_blocking(b"execd") else {
        emit_line("abilitymgr: FAIL launch spawn (execd unreachable)");
        return;
    };
    // Request v1 (+append-only app-id extension, TASK-0080D GET_PAYLOAD):
    // [E, X, ver, op=1, image_id, stack_pages, requester_len, requester...,
    //  app_len:u8, app...] — execd resolves the payload for the app id.
    const REQUESTER: &[u8] = b"abilitymgr";
    let app = app_id.as_bytes();
    if app.is_empty() || app.len() > 48 {
        emit_line("abilitymgr: FAIL launch spawn (app id length)");
        return;
    }
    let mut req = [0u8; 128];
    req[0] = b'E';
    req[1] = b'X';
    req[2] = 1;
    req[3] = 1; // OP_EXEC_IMAGE
    req[4] = image_id;
    req[5] = 8; // stack pages (service default)
    req[6] = REQUESTER.len() as u8;
    req[7..7 + REQUESTER.len()].copy_from_slice(REQUESTER);
    let mut len = 7 + REQUESTER.len();
    req[len] = app.len() as u8;
    req[len + 1..len + 1 + app.len()].copy_from_slice(app);
    len += 1 + app.len();
    let Ok(client) = KernelClient::new_with_slots(send_slot, recv_slot) else {
        emit_line("abilitymgr: FAIL launch spawn (client)");
        return;
    };
    if client.send(&req[..len], Wait::Blocking).is_err() {
        emit_line("abilitymgr: FAIL launch spawn (send)");
        return;
    }
    // Bounded reply wait: [E,X,ver,op|0x80,status,pid:u32le].
    let mut rsp = [0u8; 16];
    for _ in 0..20_000 {
        match client.recv_into(Wait::NonBlocking, &mut rsp) {
            Ok(n) if n >= 9 && rsp[3] == 0x81 => {
                if rsp[4] == 0 {
                    emit_line("abilitymgr: spawn ok");
                } else {
                    emit_line("abilitymgr: FAIL launch spawn (execd status)");
                }
                return;
            }
            Ok(_) => {} // unrelated frame on the shared channel — keep waiting
            Err(_) => {
                let _ = nexus_abi::yield_();
            }
        }
    }
    emit_line("abilitymgr: FAIL launch spawn (reply timeout)");
}

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

fn route_abilitymgr_blocking() -> Option<KernelServer> {
    if let Some((send_slot, recv_slot)) = route_blocking(b"abilitymgr") {
        return KernelServer::new_with_slots(recv_slot, send_slot).ok();
    }
    // Routing budget expired (slow boots): fall back to the deterministic slots
    // init wires via cap_transfer (recv → 3, send → 4).
    emit_line("abilitymgr: route fallback slots");
    KernelServer::new_with_slots(ABILITYMGR_RECV_SLOT, ABILITYMGR_SEND_SLOT).ok()
}

/// Emits the deterministic UART marker for a lifecycle event.
fn emit_event(event: &Event) {
    match event {
        Event::Launched { app_id, instance_id } => {
            // `abilitymgr: launch (app=<app>, inst=<id>)`
            nexus_abi::debug_ts_prefix();
            emit_prefix(b"abilitymgr: launch (app=");
            emit_str(app_id);
            emit_prefix(b", inst=");
            emit_u32(*instance_id);
            emit_prefix(b")");
            emit_newline();
        }
        Event::Transitioned { instance_id, to } => match to {
            AbilityState::Foreground => emit_inst_line(b"abilitymgr: fg (inst=", *instance_id),
            AbilityState::Background => emit_inst_line(b"abilitymgr: bg (inst=", *instance_id),
            AbilityState::Suspended => emit_inst_line(b"abilitymgr: suspend (inst=", *instance_id),
            AbilityState::Stopped => emit_inst_line(b"abilitymgr: stop (inst=", *instance_id),
            AbilityState::Started => emit_inst_line(b"abilitymgr: start (inst=", *instance_id),
            AbilityState::Created => {}
        },
    }
}

fn emit_inst_line(prefix: &[u8], id: u32) {
    nexus_abi::debug_ts_prefix();
    emit_prefix(prefix);
    emit_u32(id);
    emit_prefix(b")");
    emit_newline();
}

fn emit_line(message: &str) {
    if nexus_abi::service_line(message.as_bytes()) {
        return;
    }
    nexus_abi::debug_ts_prefix();
    emit_str(message);
    emit_newline();
}

/// RFC-0065 launch authority self-check: validate each installed app's
/// manifest-declared capabilities (build-time table from `bundles/<app>/
/// manifest.toml`) against the known permission set. Emits one marker per app —
/// `abilitymgr: caps ok app=<id> (n=<count>)` when every permission is
/// recognized, or `abilitymgr: caps reject app=<id> cap=<perm>` naming the first
/// unrecognized permission. Makes a bad manifest a boot-visible failure instead
/// of a silent launch denial later.
fn validate_manifest_caps() {
    for (app, caps) in crate::caps::APP_MANIFEST_CAPS.iter().copied() {
        match crate::caps::first_unknown(caps) {
            None => {
                nexus_abi::debug_ts_prefix();
                emit_prefix(b"abilitymgr: caps ok app=");
                emit_str(app);
                emit_prefix(b" (n=");
                emit_u32(caps.len() as u32);
                emit_prefix(b")");
                emit_newline();
            }
            Some(bad) => {
                nexus_abi::debug_ts_prefix();
                emit_prefix(b"abilitymgr: caps reject app=");
                emit_str(app);
                emit_prefix(b" cap=");
                emit_str(bad);
                emit_newline();
            }
        }
    }
}

fn emit_str(s: &str) {
    for byte in s.as_bytes().iter().copied() {
        let _ = debug_putc(byte);
    }
}

fn emit_prefix(bytes: &[u8]) {
    for byte in bytes.iter().copied() {
        let _ = debug_putc(byte);
    }
}

fn emit_newline() {
    let _ = debug_putc(b'\n');
}

/// Emits `id` as decimal ASCII (no allocation).
fn emit_u32(mut id: u32) {
    if id == 0 {
        let _ = debug_putc(b'0');
        return;
    }
    let mut buf = [0u8; 10];
    let mut i = buf.len();
    while id > 0 {
        i -= 1;
        buf[i] = b'0' + (id % 10) as u8;
        id /= 10;
    }
    for byte in buf[i..].iter().copied() {
        let _ = debug_putc(byte);
    }
}
