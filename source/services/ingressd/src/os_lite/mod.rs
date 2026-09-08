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

/// init's deterministic wiring for a declared service (RFC-0069): server
/// pair 3/4, CAP_MOVE reply inbox 5/6, then the declared routes in order —
/// policyd 7, netstackd 8. A seam never routes from its hot loop.
pub(crate) mod slots {
    pub(crate) const SVC_RECV_SLOT: u32 = 0x03;
    pub(crate) const SVC_SEND_SLOT: u32 = 0x04;
    pub(crate) const REPLY_RECV_SLOT: u32 = 0x05;
    pub(crate) const REPLY_SEND_SLOT: u32 = 0x06;
    pub(crate) const POLICYD_SEND_SLOT: u32 = 0x07;
    pub(crate) const NETSTACKD_SEND_SLOT: u32 = 0x08;
}

use nexus_abi::{yield_, IpcError, MsgHeader};
use nexus_ipc::policyd::{check_cap_on, CapDecision};

use crate::dispatch::{handle_frame, Event};
use crate::intent::{HostError, IntentHost, Registry, MAX_OPEN_EXPOSURES};
use crate::table::generated::EXPOSE_ENTRIES;
use crate::wire::{decode_request, encode_expose_reply, Reason, STATUS_DENY, STATUS_REPLY_LEN};

use gateway::Gateway;
use print::Line;
use slots::*;

/// Why the service could not come up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatewayError {
    /// The wired slots never answered (init wiring missing).
    SlotsMissing,
}

/// Timed park per turn (a true kernel park bounded by the data-plane cadence).
const PARK_NS: u64 = 5_000_000;
/// How long to wait for init's wiring before giving up.
const SLOT_WAIT_NS: u64 = 10_000_000_000;

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

/// Waits until every wired slot holds a cap (init transfers them after spawn).
fn wait_for_slots() -> Result<(), GatewayError> {
    let deadline = now_ns().saturating_add(SLOT_WAIT_NS);
    loop {
        let mut ok = true;
        for slot in [SVC_RECV_SLOT, REPLY_SEND_SLOT, POLICYD_SEND_SLOT, NETSTACKD_SEND_SLOT] {
            match nexus_abi::cap_clone(slot) {
                Ok(c) => {
                    let _ = nexus_abi::cap_close(c);
                }
                Err(_) => {
                    ok = false;
                    break;
                }
            }
        }
        if ok {
            return Ok(());
        }
        if now_ns() >= deadline {
            return Err(GatewayError::SlotsMissing);
        }
        let _ = yield_();
    }
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
    wait_for_slots()?;
    let table = EXPOSE_ENTRIES;
    let mut reg: Registry<'static, MAX_OPEN_EXPOSURES> = Registry::new(table);
    let mut gw = Gateway::new();
    let mut host = OsIntentHost;
    let _ = nexus_abi::debug_println("ingressd: ready");
    nexus_abi::service_verdict_flush("ingressd");

    loop {
        let mut hdr = MsgHeader::new(0, 0, 0, 0, 0);
        let mut sid: u64 = 0;
        let mut buf = [0u8; 64];
        let park_deadline = now_ns().saturating_add(PARK_NS);
        match nexus_abi::ipc_recv_v2(
            SVC_RECV_SLOT,
            &mut hdr,
            &mut buf,
            &mut sid,
            nexus_abi::IPC_SYS_TRUNCATE,
            park_deadline,
        ) {
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
                                        .emit();
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
        gw.service(&mut reg, now_ns());
        let _ = yield_();
    }
}
