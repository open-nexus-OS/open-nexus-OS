// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(all(nexus_env = "os", feature = "os-lite"))]
#![forbid(unsafe_code)]

//! CONTEXT: bootctld os-lite backend (TASK-0050 PR-1 slice). Boots by
//! reading the persisted boot record (v2/v1/legacy — the record codec
//! migrates), announces the target truth
//! (`bootctld: target=<t> next=<t|none>`), and serves the READ half of
//! the wire (GET_STATUS / GET_TARGET). All mutating ops answer
//! UNSUPPORTED in this slice — `updated` remains the record's single
//! writer until the client conversion flips (one-writer invariant,
//! ADR-0055; a second writer during the transition would be the exact
//! authority drift this service exists to end).
//! OWNERS: @reliability @runtime
//! STATUS: Experimental (bring-up)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU ladder (`bootctld: ready`, `bootctld: target=…`);
//!   machine/record proofs are host tests (tests/record_v2.rs).
//! ADR: docs/adr/0055-bootctld-single-boot-state-authority.md

extern crate alloc;

use alloc::boxed::Box;

use core::fmt;
use core::time::Duration;

use nexus_abi::yield_;
use nexus_ipc::budget::{self, NonceMismatchBudget, RouteRetryOutcome};
use nexus_ipc::{KernelClient, KernelServer, Server as _, Wait};
use statefs::client::StatefsClient;
use statefs::StatefsError;

use crate::machine::{BootCtrl, BootTarget, Slot};
use crate::record::{self, BOOT_RECORD_KEY};
use crate::wire;

/// Result alias surfaced by the lite backend.
pub type LiteResult<T> = Result<T, ServerError>;

/// Ready notifier invoked once the service becomes available.
pub struct ReadyNotifier(Box<dyn FnOnce() + Send>);

impl ReadyNotifier {
    /// Creates a notifier from the provided closure.
    pub fn new<F>(func: F) -> Self
    where
        F: FnOnce() + Send + 'static,
    {
        Self(Box::new(func))
    }

    /// Signals readiness to the caller.
    pub fn notify(self) {
        (self.0)();
    }
}

/// Errors surfaced by the lite backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerError {
    /// Endpoint binding failed permanently.
    Unsupported,
}

impl fmt::Display for ServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => write!(f, "bootctld unsupported"),
        }
    }
}

/// Schema warmer placeholder for API parity.
pub fn touch_schemas() {}

/// init-lite control-channel slots (route requests via the responder).
const CTRL_SEND_SLOT: u32 = 1;
const CTRL_RECV_SLOT: u32 = 2;

/// Main bootctld bring-up service loop (os-lite).
pub fn service_main_loop(notifier: ReadyNotifier) -> LiteResult<()> {
    nexus_abi::service_verdict_arm();
    let server = match KernelServer::new_for("bootctld") {
        Ok(server) => server,
        Err(_) => KernelServer::new_with_slots(3, 4).map_err(|_| ServerError::Unsupported)?,
    };
    notifier.notify();
    emit("bootctld: ready");
    nexus_abi::service_verdict_flush("bootctld");

    // Boot record: read-only in this slice (updated stays the writer).
    // Best-effort with bounded retries — statefsd races bring-up; an
    // unreadable record degrades LOUD to defaults, never a boot failure.
    let mut boot: Option<BootCtrl> = None;
    let mut load_attempts: u8 = 0;
    let mut announced = false;

    let mut breaker = nexus_ipc::resilience::CircuitBreaker::new(64, 3);
    loop {
        if boot.is_none() && load_attempts < 8 {
            load_attempts += 1;
            match load_record() {
                Some(loaded) => {
                    announce_target(&loaded);
                    announced = true;
                    boot = Some(loaded);
                }
                None if load_attempts == 8 => {
                    emit("bootctld: record unavailable (defaults)");
                    let fresh = BootCtrl::new(Slot::A);
                    announce_target(&fresh);
                    announced = true;
                    boot = Some(fresh);
                }
                None => {}
            }
        }
        let _ = announced;

        let mut inbuf = [0u8; 64];
        match server
            .recv_request_with_meta_into(Wait::Timeout(Duration::from_millis(1000)), &mut inbuf)
        {
            Ok((n, _sender_service_id, reply)) => {
                breaker.on_success();
                let frame = &inbuf[..n];
                let mut rsp = [0u8; 32];
                let len = handle_frame(boot.as_ref(), frame, &mut rsp);
                if let Some(reply) = reply {
                    if reply.reply_and_close(&rsp[..len]).is_err() {
                        emit("bootctld: reply send fail");
                    }
                } else if server.send(&rsp[..len], Wait::NonBlocking).is_err() {
                    emit("bootctld: rsp send fail (dropping)");
                }
            }
            Err(nexus_ipc::IpcError::WouldBlock) | Err(nexus_ipc::IpcError::Timeout) => {
                let _ = yield_();
            }
            Err(_) => {
                let (should_log, verdict) = breaker.on_error();
                if should_log {
                    emit("bootctld: transient ipc error (continuing)");
                }
                match verdict {
                    nexus_ipc::resilience::BreakerVerdict::Continue => {
                        let _ = yield_();
                    }
                    nexus_ipc::resilience::BreakerVerdict::EndpointDefect => {
                        emit("bootctld: endpoint defect (consecutive error limit)");
                        return Err(ServerError::Unsupported);
                    }
                }
            }
        }
    }
}

/// One request → one bounded response; returns the response length.
fn handle_frame(boot: Option<&BootCtrl>, frame: &[u8], rsp: &mut [u8; 32]) -> usize {
    let op = frame.get(3).copied().unwrap_or(0);
    if frame.len() < 4
        || frame[0] != wire::MAGIC0
        || frame[1] != wire::MAGIC1
        || frame[2] != wire::VERSION
    {
        return encode_status(rsp, op, wire::STATUS_MALFORMED);
    }
    let Some(boot) = boot else {
        // Record not loaded yet: honest FAILED, never fabricated state.
        return encode_status(rsp, op, wire::STATUS_FAILED);
    };
    match op {
        wire::OP_GET_STATUS => {
            let payload = [
                record::encode_slot(boot.active_slot()),
                boot.pending_slot().map(record::encode_slot).unwrap_or(0),
                boot.tries_left(),
                if boot.health_ok() { 1 } else { 0 },
            ];
            encode_payload(rsp, op, &payload)
        }
        wire::OP_GET_TARGET => {
            let payload = [
                record::encode_target(boot.boot_target()),
                boot.next_boot().map(record::encode_target).unwrap_or(wire::TARGET_NONE),
            ];
            encode_payload(rsp, op, &payload)
        }
        // Mutating ops arrive with the updated-client conversion (PR-2/4);
        // answering ok before then would make bootctld a second writer.
        wire::OP_STAGE
        | wire::OP_SWITCH
        | wire::OP_HEALTH_OK
        | wire::OP_BOOT_ATTEMPT
        | wire::OP_SET_NEXT_BOOT
        | wire::OP_SET_TARGET
        | wire::OP_RESET => encode_status(rsp, op, wire::STATUS_UNSUPPORTED),
        _ => encode_status(rsp, op, wire::STATUS_UNSUPPORTED),
    }
}

fn encode_status(rsp: &mut [u8; 32], op: u8, status: u8) -> usize {
    rsp[0] = wire::MAGIC0;
    rsp[1] = wire::MAGIC1;
    rsp[2] = wire::VERSION;
    rsp[3] = op | 0x80;
    rsp[4] = status;
    rsp[5] = 0;
    rsp[6] = 0;
    7
}

fn encode_payload(rsp: &mut [u8; 32], op: u8, payload: &[u8]) -> usize {
    let base = encode_status(rsp, op, wire::STATUS_OK);
    let len = payload.len().min(rsp.len() - base);
    rsp[5..7].copy_from_slice(&(len as u16).to_le_bytes());
    rsp[base..base + len].copy_from_slice(&payload[..len]);
    base + len
}

/// Reads the persisted record via the shared statefs client (@reply inbox
/// — never the shared response queue). `NotFound` = fresh image: defaults
/// announced immediately; wire trouble = retry (bounded by the caller).
fn load_record() -> Option<BootCtrl> {
    let (state_send, _) = route_blocking(b"statefsd")?;
    let (reply_send, reply_recv) = route_blocking(b"@reply")?;
    let client = KernelClient::new_with_slots(state_send, reply_recv).ok()?;
    let reply = KernelClient::new_with_slots(reply_send, reply_recv).ok();
    let statefs = StatefsClient::from_clients(client, reply);
    match statefs.get(BOOT_RECORD_KEY) {
        Ok(bytes) => match record::open_record(&bytes) {
            Ok((boot, _seq)) => Some(boot),
            Err(_) => {
                emit("bootctld: record corrupt (defaults)");
                Some(BootCtrl::new(Slot::A))
            }
        },
        Err(StatefsError::NotFound) => Some(BootCtrl::new(Slot::A)),
        Err(_) => None,
    }
}

fn announce_target(boot: &BootCtrl) {
    let target = target_label(boot.boot_target());
    let next = boot.next_boot().map(target_label).unwrap_or("none");
    let mut line = [0u8; 48];
    let mut len = 0usize;
    for part in ["bootctld: target=", target, " next=", next] {
        let bytes = part.as_bytes();
        if len + bytes.len() > line.len() {
            return;
        }
        line[len..len + bytes.len()].copy_from_slice(bytes);
        len += bytes.len();
    }
    if let Ok(msg) = core::str::from_utf8(&line[..len]) {
        emit(msg);
    }
}

fn target_label(target: BootTarget) -> &'static str {
    match target {
        BootTarget::Normal => "normal",
        BootTarget::Recovery => "recovery",
        BootTarget::Safe => "safe",
    }
}

fn route_blocking(name: &[u8]) -> Option<(u32, u32)> {
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

fn emit(message: &str) {
    let _ = nexus_abi::debug_println(message);
}
