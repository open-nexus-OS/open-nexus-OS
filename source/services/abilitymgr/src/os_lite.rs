// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! OS-lite backend for abilitymgr — the ability-lifecycle broker service loop.
//!
//! Routes its endpoint, receives request frames, drives the pure [`Broker`] via
//! [`wire::dispatch`], and emits deterministic `abilitymgr: …` markers. The live
//! resolve-via-bundlemgrd + spawn-via-execd + windowd surface bind is wired in P3.

use core::time::Duration;

use nexus_abi::{debug_putc, yield_};
use nexus_ipc::budget::{self, NonceMismatchBudget, RouteRetryOutcome};
use nexus_ipc::{KernelServer, Server as _, Wait};

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

    let server = route_abilitymgr_blocking().ok_or(AbilitymgrError::Ipc("route failed"))?;

    // The broker owns lifecycle state for the life of the service.
    let mut broker = Broker::new();

    loop {
        match server.recv_request_with_meta(Wait::Blocking) {
            Ok((frame, _sender_service_id, reply)) => {
                let out = dispatch(&mut broker, frame.as_slice());
                if let Some(event) = out.event {
                    emit_event(&event);
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
                emit_line("abilitymgr: recv error");
                return Err(AbilitymgrError::Ipc("recv"));
            }
        }
    }
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
    emit_prefix(prefix);
    emit_u32(id);
    emit_prefix(b")");
    emit_newline();
}

fn emit_line(message: &str) {
    emit_str(message);
    emit_newline();
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
