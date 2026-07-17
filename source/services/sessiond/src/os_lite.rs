// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: sessiond OS-lite runtime — owns the `session-start` boot stage and
//! serves the session wire protocol (`nexus_abi::sessiond`, TASK-0065B).
//! OWNERS: @runtime
//! STATUS: Functional (greeter/active state + manifest user registry; auth
//! docks later behind OP_LOGIN, `Locked` reserved)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: state/users host tests; QEMU marker ladder.
//! INVARIANTS:
//! - `sessiond: ready` emits once, before the session decision
//! - session markers are REAL state transitions, never decoration:
//!   `sessiond: greeter (n=…)` = greeter owns the display;
//!   `sessiond: session start (user=… product=…[ auto])` = the one
//!   `SessionState::login()` transition (auto-login runs the SAME transition)
//! - the request loop is fully reactive (blocking recv, no polling)
//! - clients only ever READ state or request login — state is never forged

#![cfg(all(nexus_env = "os", feature = "os-lite"))]

use alloc::boxed::Box;
use alloc::string::String;
use core::fmt;
use core::fmt::Write as _;

use nexus_abi::sessiond as wire;
use nexus_ipc::{KernelServer, Server as _, Wait};

use crate::state::SessionState;
use crate::users::{shipped_registry, UserRegistry};

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
    /// The embedded user manifest failed validation (host tests make this
    /// unreachable for shipped builds; loud, never silent).
    Manifest,
}

impl fmt::Display for SessiondError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ipc(what) => write!(f, "sessiond ipc: {what}"),
            Self::Manifest => write!(f, "sessiond: users manifest invalid"),
        }
    }
}

/// Legacy single-byte reply for pre-protocol frames (the skeleton's contract).
const LEGACY_STATUS_UNSUPPORTED: u8 = 3;

/// Kernel-IPC backed sessiond loop: loads the user registry, runs the session
/// decision (auto-login manifest knob or greeter), then serves
/// GET_STATE/LOGIN. The greeter/authentication UI docks onto OP_LOGIN; the
/// resolved user's `product` selects the SystemUI shell profile in windowd.
pub fn service_main_loop(notifier: ReadyNotifier) -> SessiondResult<()> {
    let server = bind_server()?;
    let registry = match shipped_registry() {
        Ok(registry) => registry,
        Err(_) => {
            let _ = nexus_abi::debug_println("sessiond: users manifest invalid");
            nexus_abi::service_verdict_flush("sessiond");
            return Err(SessiondError::Manifest);
        }
    };
    let mut state = SessionState::Greeter;
    notifier.notify();
    let _ = nexus_abi::debug_println("sessiond: ready");
    // The session decision — REAL state, not decoration. Auto-login (manifest
    // knob, proof lanes / bring-up) runs the SAME login() transition the
    // greeter click does; without it the greeter owns the display.
    match registry.auto_login {
        Some(idx) if state.login(idx).is_ok() => {
            emit_session_start(&registry, idx, true);
        }
        _ => {
            let mut line = String::new();
            let _ = write!(line, "sessiond: greeter (n={})", registry.users.len());
            let _ = nexus_abi::debug_println(&line);
        }
    }
    nexus_abi::service_verdict_flush("sessiond");
    let mut rsp = [0u8; 512];
    loop {
        match server.recv_request_with_meta(Wait::Blocking) {
            Ok((frame, _sender_service_id, reply)) => {
                let len = handle_request(frame.as_slice(), &registry, &mut state, &mut rsp);
                let out = &rsp[..len];
                if let Some(reply) = reply {
                    let _ = reply.reply_and_close(out);
                } else {
                    let _ = server.send(out, Wait::Blocking);
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

/// Serves one request frame into `rsp`; returns the response length.
fn handle_request(
    frame: &[u8],
    registry: &UserRegistry,
    state: &mut SessionState,
    rsp: &mut [u8; 512],
) -> usize {
    let Some(op) = wire::decode_request_op(frame) else {
        // Pre-protocol frame: keep the skeleton's single-byte answer.
        rsp[0] = LEGACY_STATUS_UNSUPPORTED;
        return 1;
    };
    match op {
        wire::OP_GET_STATE => encode_state_rsp(registry, state, rsp),
        wire::OP_LOGIN => {
            let Some(id) = wire::decode_login_req(frame) else {
                return encode_login_status(wire::STATUS_MALFORMED, b"", rsp);
            };
            let Some(idx) = core::str::from_utf8(id).ok().and_then(|id| registry.find(id)) else {
                return encode_login_status(wire::STATUS_UNKNOWN_USER, b"", rsp);
            };
            match state.login(idx) {
                Ok(()) => {
                    emit_session_start(registry, idx, false);
                    encode_login_status(
                        wire::STATUS_OK,
                        registry.users[idx].product.as_bytes(),
                        rsp,
                    )
                }
                Err(_) => encode_login_status(wire::STATUS_WRONG_STATE, b"", rsp),
            }
        }
        // OP_LOCK reserved + anything unknown: honest unsupported header.
        op => {
            rsp[0] = wire::MAGIC0;
            rsp[1] = wire::MAGIC1;
            rsp[2] = wire::VERSION;
            rsp[3] = op | 0x80;
            rsp[4] = wire::STATUS_UNSUPPORTED;
            5
        }
    }
}

/// GET_STATE response: header + one entry per registered user.
fn encode_state_rsp(registry: &UserRegistry, state: &SessionState, rsp: &mut [u8; 512]) -> usize {
    rsp[0] = wire::MAGIC0;
    rsp[1] = wire::MAGIC1;
    rsp[2] = wire::VERSION;
    rsp[3] = wire::OP_GET_STATE | 0x80;
    rsp[4] = wire::STATUS_OK;
    rsp[5] = state.as_wire();
    rsp[6] =
        state.active_user().and_then(|idx| u8::try_from(idx).ok()).unwrap_or(wire::NO_ACTIVE_USER);
    let mut at = wire::GET_STATE_BODY_OFFSET;
    let mut count = 0u8;
    for user in registry.users.iter() {
        let id = user.id.as_bytes();
        let name = user.display_name.as_bytes();
        let product = user.product.as_bytes();
        let need = 3 + id.len() + name.len() + product.len();
        if count == u8::MAX
            || id.len() > u8::MAX as usize
            || name.len() > u8::MAX as usize
            || product.len() > u8::MAX as usize
            || at + need > rsp.len()
        {
            break;
        }
        rsp[at] = id.len() as u8;
        rsp[at + 1..at + 1 + id.len()].copy_from_slice(id);
        at += 1 + id.len();
        rsp[at] = name.len() as u8;
        rsp[at + 1..at + 1 + name.len()].copy_from_slice(name);
        at += 1 + name.len();
        rsp[at] = product.len() as u8;
        rsp[at + 1..at + 1 + product.len()].copy_from_slice(product);
        at += 1 + product.len();
        count += 1;
    }
    rsp[7] = count;
    at
}

/// LOGIN response with the given status and product payload.
fn encode_login_status(status: u8, product: &[u8], rsp: &mut [u8; 512]) -> usize {
    let plen = product.len().min(u8::MAX as usize).min(rsp.len() - 6);
    rsp[0] = wire::MAGIC0;
    rsp[1] = wire::MAGIC1;
    rsp[2] = wire::VERSION;
    rsp[3] = wire::OP_LOGIN | 0x80;
    rsp[4] = status;
    rsp[5] = plen as u8;
    rsp[6..6 + plen].copy_from_slice(&product[..plen]);
    6 + plen
}

/// The one session-start marker: fires only on a real login() transition.
fn emit_session_start(registry: &UserRegistry, idx: usize, auto: bool) {
    let user = &registry.users[idx];
    let mut line = String::new();
    let _ = write!(
        line,
        "sessiond: session start (user={} product={}{})",
        user.id,
        user.product,
        if auto { " auto" } else { "" }
    );
    let _ = nexus_abi::debug_println(&line);
}

/// Bind the server endpoint: the route registry when available, else the
/// deterministic fallback slots init's declarative arm provisioned (RFC-0069).
fn bind_server() -> SessiondResult<KernelServer> {
    if let Ok(server) = KernelServer::new_for("sessiond") {
        return Ok(server);
    }
    KernelServer::new_with_slots(3, 4).map_err(|_| SessiondError::Ipc("bind"))
}
