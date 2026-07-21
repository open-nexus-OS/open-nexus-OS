// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! settingsd OS-lite runtime (TASK-0072 Phase 8): binds the settingsd server,
//! loads persisted prefs from statefsd at boot, and serves the typed
//! `nexus_abi::settingsd` wire protocol (GET/SET). A SET is atomic:
//! validate → persist (statefsd) → apply → reply. Fully reactive (blocking
//! recv, no polling); clients only ever read or request a validated change.
//! OWNERS: @runtime
//! STATUS: Experimental
//! INVARIANTS:
//! - `settingsd: ready` emits once, after the boot prefs load
//! - `settingsd: load prefs (n=…)` = how many persisted overrides were applied
//! - `settingsd: set key=… value=… persist=…` fires only on a REAL change
//! - a persist failure never rolls back the validated in-memory value
#![cfg(all(nexus_env = "os", feature = "os-lite"))]

use alloc::string::String;
use core::fmt;
use core::fmt::Write as _;

use nexus_abi::settingsd as wire;
use nexus_ipc::{KernelServer, Server as _, Wait};

use crate::registry::{SetError, SettingsRegistry};
use crate::statefs_client;
use crate::watch::WatchTable;

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
    let loaded = match statefs_client::load_prefs() {
        Some(blob) => registry.load_prefs_blob(&blob),
        None => 0,
    };
    let mut line = String::new();
    let _ = write!(line, "settingsd: load prefs (n={loaded})");
    let _ = nexus_abi::debug_println(&line);

    let _ = nexus_abi::debug_println("settingsd: ready");
    nexus_abi::service_verdict_flush("settingsd");

    let mut rsp = [0u8; 300];
    let mut watchers = WatchTable::new();
    loop {
        match server.recv_request_with_meta(Wait::Blocking) {
            Ok((frame, _sender_service_id, reply)) => {
                // OP_WATCH (RFC-0078): the moved cap IS the subscription's
                // push channel — keep it, never reply_and_close it.
                if let Some((wire::OP_WATCH, prefix, _)) = wire::decode_request(frame.as_slice()) {
                    match reply {
                        Some(chan) if watchers.register(chan.slot(), prefix) => {
                            let _ = nexus_abi::debug_println("settingsd: watch registered");
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
                let len = handle_request(frame.as_slice(), &mut registry, &mut rsp, &mut watchers);
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
                return Err(SettingsdError::Ipc("disconnected"))
            }
            Err(_) => return Err(SettingsdError::Ipc("recv")),
        }
    }
}

/// Serves one request frame into `rsp`; returns the response length.
fn handle_request(
    frame: &[u8],
    registry: &mut SettingsRegistry,
    rsp: &mut [u8; 300],
    watchers: &mut WatchTable,
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
                    apply_change(registry, key, value);
                    // RFC-0078: notify subscribers of the APPLIED change
                    // (fire-and-forget, bounded; failures set resync).
                    watchers.notify(key, value, |chan, ev| {
                        let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, ev.len() as u32);
                        nexus_abi::ipc_send_v1(chan, &hdr, ev, nexus_abi::IPC_SYS_NONBLOCK, 0)
                            .is_ok()
                    });
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

/// A validated value CHANGED: persist the whole prefs blob to statefsd (atomic
/// single PUT) and emit the honest `set` marker with the persist outcome. The
/// in-memory value is already committed — a persist failure only forfeits
/// reboot survival, never the running value (Phase 10 layers the live
/// cross-service apply, e.g. windowd theme, on top of this).
fn apply_change(registry: &SettingsRegistry, key: &str, value: &str) {
    let persisted = statefs_client::store_prefs(&registry.to_prefs_blob());
    let mut line = String::new();
    let _ = write!(
        line,
        "settingsd: set key={key} value={value} persist={}",
        if persisted { "ok" } else { "fail" }
    );
    let _ = nexus_abi::debug_println(&line);
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

/// Bind the server endpoint: the route registry when available, else the
/// deterministic fallback slots init's declarative arm provisioned (RFC-0069).
fn bind_server() -> SettingsdResult<KernelServer> {
    if let Ok(server) = KernelServer::new_for("settingsd") {
        return Ok(server);
    }
    KernelServer::new_with_slots(3, 4).map_err(|_| SettingsdError::Ipc("bind"))
}
