// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: sessiond OS-lite runtime — owns the `session-start` boot stage.
//! OWNERS: @runtime
//! STATUS: Experimental (skeleton — auto-starts the default session)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder.
//! INVARIANTS:
//! - `sessiond: ready` emits once, before the session decision
//! - the session decision is a REAL state transition marker, not decoration:
//!   the login track replaces the auto-start behind the same markers
//! - the request loop is fully reactive (blocking recv, no polling)

#![cfg(all(nexus_env = "os", feature = "os-lite"))]

extern crate alloc;

use alloc::boxed::Box;
use core::fmt;

use nexus_ipc::{KernelServer, Server as _, Wait};

/// Result alias used by the lite sessiond backend.
pub type SessiondResult<T> = Result<T, SessiondError>;

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

/// Errors surfaced by the lite sessiond backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessiondError {
    /// IPC transport failure.
    Ipc(&'static str),
}

impl fmt::Display for SessiondError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ipc(what) => write!(f, "sessiond ipc: {what}"),
        }
    }
}

/// Session-request status bytes (protocol placeholder until the login track
/// defines the real greeter/session wire contract).
const STATUS_UNSUPPORTED: u8 = 3;

/// Minimal kernel-IPC backed sessiond loop.
///
/// Owns the `stage: session-start` transition: today the DEFAULT session starts
/// immediately (the shell is windowd-hosted — zero visible change); the
/// greeter/login later replaces the auto-start behind the same markers, and the
/// resolved user session selects the SystemUI shell profile.
pub fn service_main_loop(notifier: ReadyNotifier) -> SessiondResult<()> {
    let server = bind_server()?;
    notifier.notify();
    let _ = nexus_abi::debug_println("sessiond: ready");
    // The session decision — REAL state, not decoration: this is the exact
    // point the login track replaces with greeter → auth → user session.
    let _ = nexus_abi::debug_println("session: started (default)");
    nexus_abi::service_verdict_flush("sessiond");
    loop {
        match server.recv_request_with_meta(Wait::Blocking) {
            Ok((_frame, _sender_service_id, reply)) => {
                // No session ops are defined yet — answer honestly.
                let rsp = [STATUS_UNSUPPORTED];
                if let Some(reply) = reply {
                    let _ = reply.reply_and_close(&rsp);
                } else {
                    let _ = server.send(&rsp, Wait::Blocking);
                }
            }
            Err(nexus_ipc::IpcError::WouldBlock) | Err(nexus_ipc::IpcError::Timeout) => {
                let _ = nexus_abi::yield_();
            }
            Err(nexus_ipc::IpcError::Disconnected) => {
                return Err(SessiondError::Ipc("disconnected"))
            }
            Err(_) => return Err(SessiondError::Ipc("recv")),
        }
    }
}

/// Bind the server endpoint: the route registry when available, else the
/// deterministic fallback slots init's declarative arm provisioned (RFC-0069).
fn bind_server() -> SessiondResult<KernelServer> {
    if let Ok(server) = KernelServer::new_for("sessiond") {
        return Ok(server);
    }
    KernelServer::new_with_slots(3, 4).map_err(|_| SessiondError::Ipc("bind"))
}
