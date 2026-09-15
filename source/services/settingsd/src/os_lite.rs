// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! settingsd OS-lite runtime (TASK-0072 Phase 8, RFC-0083 reactive): binds
//! the settingsd server, loads persisted prefs from statefsd at boot, and
//! serves the typed `nexus_abi::settingsd` wire protocol (GET/SET/WATCH).
//! A SET is: validate → commit in memory → REPLY → notify watchers → mark
//! the blob dirty. Persistence is ASYNC (`persist::Persister` + NONBLOCK
//! statefsd PUT/reply-drain, coalesced, bounded backoff) — a client never
//! waits on statefsd, and the loop waits on ONE waitset (server + reply inbox + timer-notify):
//! a PUT in flight ends with statefsd's answer or its death, the persist floor/backoff with a
//! kernel one-shot timer (TASK-0324 P7-d; no recv timeout, no polling, no yield-spins).
//! OWNERS: @runtime
//! STATUS: Experimental
//! INVARIANTS:
//! - `settingsd: ready` emits once, after the boot prefs load
//! - `settingsd: load prefs (n=…)` = how many persisted overrides were applied
//! - `settingsd: set key=… value=…` fires only on a REAL change, BEFORE persist
//! - `settingsd: persist ok|fail` fires only on an ACTUAL statefsd outcome
//!   (no-fake-green: an in-flight PUT reports nothing)
//! - a persist failure never rolls back the validated in-memory value
//! - a fresh watcher receives its full matching state on registration (burst)
#![cfg(all(nexus_env = "os", feature = "os-lite"))]

use alloc::string::String;
use core::fmt;
use core::fmt::Write as _;

use nexus_abi::settingsd as wire;
use nexus_ipc::{KernelClient, KernelServer, Server as _, Wait};
use statefs::client::StatefsClient;
use statefs::protocol as sf_proto;

use crate::persist::{Action, Persister};
use crate::registry::{SetError, SettingsRegistry};
use crate::watch::WatchTable;
use nexus_service_topology::slots::settingsd as slots;

/// Result alias for the lite settingsd backend.
pub type SettingsdResult<T> = Result<T, SettingsdError>;

/// Errors surfaced by the lite settingsd backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsdError {
    /// IPC transport failure.
    Ipc(&'static str),
}

impl fmt::Display for SettingsdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ipc(what) => write!(f, "settingsd ipc: {what}"),
        }
    }
}

/// Kernel-IPC settingsd loop: loads persisted prefs, then serves GET/SET over
/// the typed wire protocol. The greeter/settings panel (windowd) is a client;
/// theme/font consumers read via GET and re-read on their own cadence.
pub fn service_main_loop() -> SettingsdResult<()> {
    let server = bind_server()?;
    let mut registry = SettingsRegistry::new();

    // Boot prefs load: overrides persisted by a prior session (statefsd). Best-
    // effort — statefsd unreachable / unset simply leaves the code defaults.
    let loaded = match load_prefs() {
        Some(blob) => registry.load_prefs_blob(&blob),
        None => 0,
    };
    let mut line = String::new();
    let _ = write!(line, "settingsd: load prefs (n={loaded})");
    let _ = nexus_abi::debug_println(&line);

    let _ = nexus_service_entry::ready("settingsd: ready");
    nexus_abi::service_verdict_flush("settingsd");

    let mut rsp = [0u8; 300];
    let mut watchers = WatchTable::new();
    let mut persister = Persister::new();
    // TASK-0324 P7-d: ONE waitset over the server endpoint, the statefsd reply inbox (PUT
    // answers) and the timer-notify endpoint. A PUT in flight ends with statefsd's answer or
    // its death (EOF); the persist floor and the failure backoff are paced by a kernel
    // ONE-SHOT timer armed at the exact deadline — no recv timeout, no 50 ms tick.
    let waitset = build_waitset(&server);
    let mut timer = nexus_ipc::timer::NotifyTimer::bind(slots::TIMER).ok();
    if waitset.is_none() || timer.is_none() {
        let _ = nexus_abi::debug_println(
            "settingsd: FAIL waitset/timer (blocking on the server endpoint alone)",
        );
    }
    loop {
        // Persistence FIRST (drain PUT replies, send a due PUT) — it never blocks a client.
        pump_persist(&mut persister, &registry);
        if let Some(t) = timer.as_mut() {
            t.arm_at(persister.next_deadline_ns().unwrap_or(0));
        }
        let wait = if waitset.is_some() { Wait::NonBlocking } else { Wait::Blocking };
        for _ in 0..IPC_BATCH_LIMIT {
            match server.recv_request_with_meta(wait) {
                Ok((frame, _sender_service_id, reply)) => {
                    // OP_WATCH (RFC-0078/0083): the moved cap IS the subscription's
                    // push channel — keep it, never reply_and_close it. A fresh
                    // watcher immediately receives its full matching state
                    // (registration burst = the subscriber's boot restore).
                    if let Some((wire::OP_WATCH, prefix, _)) =
                        wire::decode_request(frame.as_slice())
                    {
                        match reply {
                            Some(chan) if watchers.register(chan.slot(), prefix) => {
                                let current = current_values(&registry);
                                let (matching, delivered) =
                                    watchers.sync(chan.slot(), &current, send_event);
                                let mut line = String::new();
                                let _ = write!(
                                    line,
                                    "settingsd: watch registered (sync {delivered}/{matching})"
                                );
                                let _ = nexus_abi::debug_println(&line);
                            }
                            _ => {
                                // No moved cap / table full: honest reject on the
                                // shared endpoint (the would-be subscriber's recv).
                                let len =
                                    encode(wire::OP_WATCH, wire::STATUS_PERSIST_FAIL, "", &mut rsp);
                                let _ = server.send(&rsp[..len], Wait::NonBlocking);
                            }
                        }
                        continue;
                    }
                    let len = handle_request(
                        frame.as_slice(),
                        &mut registry,
                        &mut rsp,
                        &mut watchers,
                        &mut persister,
                    );
                    let out = &rsp[..len];
                    if let Some(reply) = reply {
                        let _ = reply.reply_and_close(out);
                    } else {
                        let _ = server.send(out, Wait::Blocking);
                    }
                }
                Err(nexus_ipc::IpcError::WouldBlock) | Err(nexus_ipc::IpcError::Timeout) => break,
                Err(nexus_ipc::IpcError::Disconnected) => {
                    return Err(SettingsdError::Ipc("disconnected"))
                }
                Err(_) => return Err(SettingsdError::Ipc("recv")),
            }
            if waitset.is_none() {
                break;
            }
        }
        // WAIT — no clock. A timer frame means the one-shot fired (the kernel disarmed it).
        if let Some(ws) = waitset {
            let _ = nexus_abi::waitset_wait(ws, 0);
            if let Some(t) = timer.as_mut() {
                t.drain();
            }
        }
    }
}

/// Requests served per pass before the loop returns to its other waitset members.
const IPC_BATCH_LIMIT: usize = 32;

/// The loop's waitset (TASK-0324 P7-d): its server endpoint, the statefsd reply inbox and the
/// timer-notify endpoint — declared slots, so the members exist before this task runs.
fn build_waitset(server: &KernelServer) -> Option<u32> {
    let ws = nexus_abi::waitset_create().ok()?;
    let (server_recv, _) = server.slots();
    for slot in [server_recv, slots::REPLY.recv, slots::TIMER_RECV] {
        nexus_abi::waitset_add(ws, slot).ok()?;
    }
    Some(ws)
}

/// One NONBLOCK persistence step: harvest any statefsd PUT reply, then send
/// a due PUT of the CURRENT blob. Markers only on real outcomes
/// (no-fake-green); failure markers bounded so a dead statefsd cannot flood
/// the UART while the backoff retries forever.
fn pump_persist(persister: &mut Persister, registry: &SettingsRegistry) {
    let now = nexus_abi::nsec().unwrap_or(0);
    while let Some(ok) = poll_put_reply(persister.in_flight()) {
        persister.on_reply(ok, now);
        if ok {
            let _ = nexus_abi::debug_println("settingsd: persist ok");
        } else if persister.failures() <= 4 {
            let _ = nexus_abi::debug_println("settingsd: persist fail (statefsd status)");
        }
    }
    if persister.poll(now) == Action::SendPut {
        if try_send_put(&registry.to_prefs_blob()) {
            persister.on_put_sent(now);
        } else {
            persister.on_send_failed(now);
            if persister.failures() <= 4 {
                let _ = nexus_abi::debug_println("settingsd: persist fail (send)");
            }
        }
    }
}

/// The registry's full current state, for watch burst/heal frames.
fn current_values(registry: &SettingsRegistry) -> alloc::vec::Vec<(&'static str, &str)> {
    SettingsRegistry::keys().filter_map(|k| registry.get(k).map(|v| (k, v))).collect()
}

/// The one NONBLOCK event send watchers use (failures flag resync — the
/// heal path re-sends full state on the next change). Failures are logged
/// bounded WITH the errno: an undelivered event was previously invisible.
fn send_event(chan: u32, ev: &[u8]) -> bool {
    let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, ev.len() as u32);
    match nexus_abi::ipc_send_v1(chan, &hdr, ev, nexus_abi::IPC_SYS_NONBLOCK, 0) {
        Ok(_) => true,
        Err(e) => {
            static LOGGED: core::sync::atomic::AtomicUsize =
                core::sync::atomic::AtomicUsize::new(0);
            if LOGGED.fetch_add(1, core::sync::atomic::Ordering::Relaxed) < 6 {
                let mut line = String::new();
                let _ = write!(line, "settingsd: event send FAIL chan={chan} err={e:?}");
                let _ = nexus_abi::debug_println(&line);
            }
            false
        }
    }
}

/// Serves one request frame into `rsp`; returns the response length.
///
/// RFC-0083 ordering on a changed SET: commit in memory → notify watchers
/// (NONBLOCK, µs) → mark the blob dirty → return the reply. The client's
/// reply is NEVER gated behind statefsd — the persist pump handles that
/// asynchronously with coalescing and backoff.
fn handle_request(
    frame: &[u8],
    registry: &mut SettingsRegistry,
    rsp: &mut [u8; 300],
    watchers: &mut WatchTable,
    persister: &mut Persister,
) -> usize {
    let Some((op, key, value)) = wire::decode_request(frame) else {
        // Pre-protocol / malformed frame: honest malformed header (GET-shaped).
        return encode(wire::OP_GET, wire::STATUS_MALFORMED, "", rsp);
    };
    match op {
        wire::OP_GET => match registry.get(key) {
            Some(v) => encode(op, wire::STATUS_OK, v, rsp),
            None => encode(op, wire::STATUS_UNKNOWN_KEY, "", rsp),
        },
        wire::OP_SET => match registry.set(key, value) {
            Ok(changed) => {
                if changed {
                    let mut line = String::new();
                    let _ = write!(line, "settingsd: set key={key} value={value}");
                    let _ = nexus_abi::debug_println(&line);
                    // RFC-0078/0083: notify subscribers of the APPLIED change;
                    // a resync-flagged watcher is healed with its full
                    // matching state instead of only the changed key.
                    let current = current_values(registry);
                    watchers.notify(key, value, &current, send_event);
                    persister.mark_dirty();
                }
                // Echo the now-current value (the validated new one).
                let current = registry.get(key).unwrap_or(value);
                encode(op, wire::STATUS_OK, current, rsp)
            }
            Err(SetError::UnknownKey) => encode(op, wire::STATUS_UNKNOWN_KEY, "", rsp),
            Err(SetError::InvalidValue) => encode(op, wire::STATUS_INVALID_VALUE, "", rsp),
        },
        // Unknown op: honest unsupported (reuse INVALID_VALUE's slot is wrong;
        // answer a malformed header so clients don't mistake it for a value).
        _ => encode(op, wire::STATUS_MALFORMED, "", rsp),
    }
}

/// Encode a settingsd response into `rsp`, returning its length. Values that
/// somehow exceed the frame are clamped (never a panic).
fn encode(op: u8, status: u8, value: &str, rsp: &mut [u8; 300]) -> usize {
    // Reserve 7 header bytes; clamp the value to what fits + the u8 length field.
    let max_val = (rsp.len() - 7).min(u8::MAX as usize);
    let v = &value.as_bytes()[..value.len().min(max_val)];
    let v = core::str::from_utf8(v).unwrap_or("");
    wire::encode_response(op, status, v, rsp).unwrap_or_else(|| {
        // Unreachable given the clamp, but never panic — emit a bare header.
        rsp[0] = wire::MAGIC0;
        rsp[1] = wire::MAGIC1;
        rsp[2] = wire::VERSION;
        rsp[3] = op | 0x80;
        rsp[4] = wire::STATUS_MALFORMED;
        rsp[5] = wire::TYPE_TEXT;
        rsp[6] = 0;
        7
    })
}

/// Bind the server endpoint: the declared slots init's arm pinned before resume (no route
/// ask — asks have no clock since TASK-0324 P7-b, and a start-up ask can deadlock init).
fn bind_server() -> SettingsdResult<KernelServer> {
    let slots = nexus_service_topology::slots::settingsd::SERVER;
    KernelServer::new_with_slots(slots.recv, slots.send).map_err(|_| SettingsdError::Ipc("bind"))
}

// ── statefsd persistence (TASK-0025 step 3: wire SSOT = `statefs`) ───────────
// The hand-rolled wire copy (`statefs_client.rs`) is gone: the boot-time GET
// goes through the shared `statefs::client::StatefsClient`, and the RFC-0083
// ASYNC persist path (NONBLOCK PUT send + NONBLOCK reply drain, driven by
// `persist::Persister`) encodes/filters frames via `statefs::protocol`. Only
// slot-level glue lives here. Best-effort throughout: a routing/policy/IPC
// failure degrades to defaults / retry-with-backoff — never a boot failure,
// never a blocked client.

/// settingsd's key in statefsd's flat KV store. Stable across boots (a const),
/// so the same overrides load back every time. statefsd's journal engine
/// accepts ONLY `/state/`-rooted keys (validate_key) — the original bare
/// `settingsd/prefs` earned STATUS_INVALID_KEY on every PUT, which was the
/// actual `persist=fail` after the route was wired.
const PREFS_KEY: &str = "/state/settingsd/prefs";

/// Load the persisted prefs blob via the shared statefs client, or `None`
/// when statefsd is unreachable / the key is unset. The returned string is
/// the `key=value\n` override blob
/// [`crate::registry::SettingsRegistry::load_prefs_blob`] consumes. BOOT
/// ONLY (before the serve loop starts); the client's request/reply exchange
/// is bounded, never an indefinite block.
fn load_prefs() -> Option<String> {
    let (send_slot, reply_send_slot, reply_recv_slot) = statefs_slots();
    // Named-route slots are persistent and `KernelClient` never closes its
    // slots, so wrapping the cached slots here is drop-safe.
    let client = KernelClient::new_with_slots(send_slot, reply_recv_slot).ok()?;
    let reply = KernelClient::new_with_slots(reply_send_slot, reply_recv_slot).ok();
    let statefs = StatefsClient::from_clients(client, reply);
    match statefs.get(PREFS_KEY) {
        Ok(value) => String::from_utf8(value).ok(),
        Err(_) => None,
    }
}

/// NONBLOCK send of one prefs PUT (RFC-0083 async persist). Returns whether
/// the frame LEFT — the reply arrives later via [`poll_put_reply`]. A refused
/// send (full queue / no route) is the caller's backoff signal, never a spin.
fn try_send_put(blob: &str) -> bool {
    let (send_slot, reply_send_slot, _) = statefs_slots();
    let Ok(req) = sf_proto::encode_put_request(PREFS_KEY, blob.as_bytes()) else {
        return false;
    };
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
    // Queue space is waited for in the kernel (TASK-0324 P7-d); a refused send is statefsd's
    // absence, reported to the persister as a failure (backoff, no clock in the wait).
    match nexus_abi::ipc_send_v1(send_slot, &hdr, &req, 0, 0) {
        Ok(_) => true,
        Err(_) => {
            let _ = nexus_abi::cap_close(reply_send_clone);
            false
        }
    }
}

/// NONBLOCK drain of the shared `@reply` inbox for a statefsd PUT reply.
/// `Some(ok)` = a PUT reply arrived; `None` = nothing (or only foreign
/// frames) waiting. Foreign frames are skipped, bounded per call. The status
/// byte sits at offset 4 in both v1 and v2 statefs replies.
/// Harvests ONE statefsd PUT reply from the inbox, `None` when there is none. With a PUT in
/// flight the inbox is read EOF-opted (RFC-0079): statefsd holds the moved reply cap until it
/// answers, so `PeerClosed` with nothing queued means statefsd died — the PUT failed
/// (`Some(false)`), no clock decides it. Foreign inbox traffic is skipped (bounded).
fn poll_put_reply(in_flight: bool) -> Option<bool> {
    let (_, _, reply_recv_slot) = statefs_slots();
    let eof = if in_flight { nexus_abi::IPC_SYS_EOF } else { 0 };
    for _ in 0..4 {
        let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 64]; // a PUT status reply is a handful of bytes
        match nexus_abi::ipc_recv_v1(
            reply_recv_slot,
            &mut rh,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE | eof,
            0,
        ) {
            Ok(n) => {
                let n = core::cmp::min(n as usize, buf.len());
                let frame = &buf[..n];
                if n >= 5
                    && frame[0] == sf_proto::MAGIC0
                    && frame[1] == sf_proto::MAGIC1
                    && frame[3] == (sf_proto::OP_PUT | 0x80)
                {
                    return Some(frame[4] == sf_proto::STATUS_OK);
                }
            }
            Err(nexus_abi::IpcError::PeerClosed) if in_flight => return Some(false),
            Err(_) => return None,
        }
    }
    None
}

/// Route slots resolved ONCE (named routes are persistent slots — never
/// closed, safe to cache): `(statefsd send, @reply send, @reply recv)`.
/// The statefsd leg and the reply inbox: DECLARED slots (`nexus-service-topology`), pinned
/// by init before this task runs — no runtime route ask (TASK-0324 P7-b/P7-d).
fn statefs_slots() -> (u32, u32, u32) {
    (slots::STATEFSD.send, slots::REPLY.send, slots::REPLY.recv)
}
