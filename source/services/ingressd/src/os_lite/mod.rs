// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: ingressd OS-lite service loop (RFC-0092 Layer B, TASK-0052 P3).
//! Intents arrive on the server slot with the kernel-attributed sender;
//! `dispatch::handle_frame` turns each into a verdict (policyd is asked for
//! the DECLARED subject's `net.expose` over the fixed policyd slot —
//! unreachable ⇒ refused), the reply rides the caller's CAP_MOVE cap, and
//! every behaviour has its marker: `ingressd: ready` once the wired slots
//! answer, `ingressd: port open (port=…, proto=…)` after the NIC-facing
//! listener is bound AND the intent registered, `ingressd: deny
//! (reason=…)` for every refusal. Between intents the loop parks on a
//! timed recv and drives the data plane (`gateway`).
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU markers (scripts/qemu-test.sh), tests/ingress_host/ (core)
//! RFC: docs/rfcs/RFC-0092-service-exposure-contract-ingress-policy-ingressd.md

pub(crate) mod gateway;
pub(crate) mod netclient;
pub(crate) mod print;
pub(crate) mod stream;

/// ingressd's capability slots as `nexus-service-topology` declares them and init pins them
/// (TASK-0324 P4f-1a) — they used to be the numbers the transfer order happened to produce.
/// A seam never routes from its hot loop.
pub(crate) mod slots {
    use nexus_service_topology::slots::ingressd as topo;

    pub(crate) const SVC_RECV_SLOT: u32 = topo::SERVER.recv;
    /// The declared timer-notify pair (TASK-0054C P2-b).
    pub(crate) const TIMER: nexus_service_topology::SlotPair = topo::TIMER;
    pub(crate) const SVC_SEND_SLOT: u32 = topo::SERVER.send;
    pub(crate) const REPLY_RECV_SLOT: u32 = topo::REPLY.recv;
    pub(crate) const REPLY_SEND_SLOT: u32 = topo::REPLY.send;
    pub(crate) const POLICYD_SEND_SLOT: u32 = topo::POLICYD.send;
    pub(crate) const NETSTACKD_SEND_SLOT: u32 = topo::NETSTACKD.send;
}

use nexus_abi::{IpcError, MsgHeader};
use nexus_ipc::policyd::{check_cap_on, CapDecision};
use nexus_ipc::timer::{NotifyTimer, Waitset};

use crate::dispatch::{handle_frame, Event};
use crate::intent::{HostError, IntentHost, Registry, MAX_OPEN_EXPOSURES};
use crate::table::generated::EXPOSE_ENTRIES;
use crate::wire::{decode_request, encode_expose_reply, Reason, STATUS_DENY, STATUS_REPLY_LEN};

use gateway::Gateway;
use print::Line;
use slots::*;

/// Why the service could not come up — nothing, since TASK-0054C P2-b: the declared slots are
/// pinned before resume and the loop never returns; kept as the entry's error type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatewayError {}

/// The gateway's service cadence (accepts, relays, rate windows): a PERIODIC kernel timer on
/// the declared notify pair (TASK-0054C P2-b) — never a receive deadline.
const SERVICE_INTERVAL_NS: u64 = 5_000_000;

/// policyd over the fixed slots: `OP_CHECK_CAP_DELEGATED(subject, "net.expose")`.
struct OsIntentHost;

impl IntentHost for OsIntentHost {
    fn expose_capability(&mut self, subject: u64) -> Result<bool, HostError> {
        match check_cap_on(
            POLICYD_SEND_SLOT,
            REPLY_SEND_SLOT,
            REPLY_RECV_SLOT,
            subject,
            b"net.expose",
        ) {
            CapDecision::Allow => Ok(true),
            CapDecision::Deny => Ok(false),
            CapDecision::Unreachable => Err(HostError),
        }
    }
}

fn now_ns() -> u64 {
    nexus_abi::nsec().unwrap_or(0)
}

fn reply(hdr: &MsgHeader, frame: &[u8]) {
    if frame.is_empty() {
        return;
    }
    let rh = MsgHeader::new(0, 0, 0, 0, frame.len() as u32);
    if (hdr.flags & nexus_abi::ipc_hdr::CAP_MOVE) != 0 {
        // Blocking reply on the moved cap (never drop a verdict under queue
        // pressure), then release the cap.
        let _ = nexus_abi::ipc_send_v1(hdr.src, &rh, frame, 0, 0);
        let _ = nexus_abi::cap_close(hdr.src);
    } else {
        let _ = nexus_abi::ipc_send_v1(SVC_SEND_SLOT, &rh, frame, nexus_abi::IPC_SYS_NONBLOCK, 0);
    }
}

/// The gateway loop; returns only when the service cannot come up.
pub fn service_main_loop() -> Result<(), GatewayError> {
    // Verdict folding (RFC-0068): routine markers fold in interactive boots.
    nexus_abi::service_verdict_arm();
    let table = EXPOSE_ENTRIES;
    let mut reg: Registry<'static, MAX_OPEN_EXPOSURES> = Registry::new(table);
    let mut gw = Gateway::new();
    let mut host = OsIntentHost;
    let _ = nexus_service_entry::ready("ingressd: ready");
    nexus_abi::service_verdict_flush("ingressd");

    // TASK-0054C P2-b: the service cadence is a periodic kernel timer on the declared notify
    // pair, a waitset member beside the server endpoint. A request or the tick wakes the loop;
    // nothing here holds a deadline (the 5 ms timed park that used to double as the clock is
    // gone). Declared slots are pinned before resume, so there is nothing to wait for first.
    let mut timer = NotifyTimer::bind_with_interval(TIMER, SERVICE_INTERVAL_NS).ok();
    let waitset = timer.as_ref().and_then(|t| Waitset::over(&[SVC_RECV_SLOT, t.recv_slot()]).ok());
    if let Some(t) = timer.as_mut() {
        t.arm_in(SERVICE_INTERVAL_NS);
    }
    if waitset.is_none() {
        Line::prefixed("FAIL waitset/timer (blocking on the server endpoint alone)").emit_raw();
    }
    let recv_flags = if waitset.is_some() {
        nexus_abi::IPC_SYS_TRUNCATE | nexus_abi::IPC_SYS_NONBLOCK
    } else {
        nexus_abi::IPC_SYS_TRUNCATE
    };
    loop {
        let mut hdr = MsgHeader::new(0, 0, 0, 0, 0);
        let mut sid: u64 = 0;
        let mut buf = [0u8; 64];
        match nexus_abi::ipc_recv_v2(SVC_RECV_SLOT, &mut hdr, &mut buf, &mut sid, recv_flags, 0) {
            Ok(n) => {
                let n = (n as usize).min(buf.len());
                let mut out = [0u8; STATUS_REPLY_LEN];
                let outcome = handle_frame(&mut reg, &mut host, sid, &buf[..n], &mut out);
                let mut reply_len = outcome.reply_len;
                match outcome.event {
                    Event::Opened { idx, port, proto } => {
                        if let Some(entry) = table.get(idx) {
                            match gw.open(idx, entry) {
                                Ok(()) => {
                                    Line::prefixed("port open (port=")
                                        .push_dec(u64::from(port))
                                        .push(b", proto=")
                                        .push_str(proto.label())
                                        .push(b")")
                                        .emit();
                                }
                                Err(_) => {
                                    // Facade refused/unreachable: the intent
                                    // is NOT registered (fail closed).
                                    Line::prefixed("port open FAIL (port=")
                                        .push_dec(u64::from(port))
                                        .push(b", proto=")
                                        .push_str(proto.label())
                                        .push(b")")
                                        .emit_raw();
                                    let _ = reg.close(entry.subject_id, port, proto);
                                    if let Ok(req) = decode_request(&buf[..n]) {
                                        reply_len = encode_expose_reply(
                                            req.op,
                                            req.nonce,
                                            STATUS_DENY,
                                            Reason::Limit,
                                            &mut out,
                                        )
                                        .unwrap_or(0);
                                    }
                                }
                            }
                        }
                    }
                    Event::Closed { idx, .. } => gw.close(idx),
                    Event::Denied { reason } => gw.deny(sid, reason, now_ns()),
                    Event::Status | Event::Malformed | Event::Unsupported => {}
                }
                reply(&hdr, &out[..reply_len]);
            }
            Err(IpcError::QueueEmpty) | Err(IpcError::TimedOut) => {}
            Err(_) => {
                static RECV_ERR_LOGGED: core::sync::atomic::AtomicBool =
                    core::sync::atomic::AtomicBool::new(false);
                if !RECV_ERR_LOGGED.swap(true, core::sync::atomic::Ordering::Relaxed) {
                    let _ = nexus_abi::debug_println("ingressd: ipc recv err");
                }
            }
        }
        if let Some(t) = timer.as_mut() {
            t.drain();
        }
        gw.service(&mut reg, now_ns());
        // WAIT — no clock: the next request or the next tick.
        if let Some(ws) = waitset.as_ref() {
            let _ = ws.wait();
        }
    }
}
