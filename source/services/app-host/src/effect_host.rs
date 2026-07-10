// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: app-host DSL `EffectHost` over the **provisioned fixed slots**
//! (TASK-0080C Umbau #16). Unlike windowd's in-compositor host — which ROUTES
//! each `svc.*` call through the init control channel it holds — an app-host
//! child has no control channel. execd provisions its manifest-declared routes
//! at launch (`nexus-sdk-routes`): a fresh `@reply` inbox in the child's fixed
//! reply slots and one SEND per routable cap in the service's fixed
//! `child_slot`. This host derives `svc.<name>` → slot from that same SSOT and
//! speaks the service wire directly:
//!   - `bundlemgr.enumerate` → `OP_LIST_APPS` (reply via `@reply` CAP_MOVE),
//!   - `session.users`/`session.login` → sessiond `OP_GET_STATE`/`OP_LOGIN`,
//!   - `ability.launch` → abilitymgr `OP_LAUNCH` (fire-and-forget: the launch
//!     outcome is abilitymgr's own marker chain — windowd's host records the
//!     intent without awaiting a reply, same contract).
//! A cap-gated route that was never granted leaves its slot empty; the send
//! fails bounded and the call returns `Err(ERR_SVC_UNAVAILABLE)` — the DSL's
//! `Err(e)` arm renders, never a silent hang.
//! OWNERS: @ui @runtime
//! STATUS: Experimental (TASK-0080C Umbau #16)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: the length-prefixed entry parsers mirror windowd's
//! host-tested `take_lp_str`/`from_list_apps_response`; the fixed-slot
//! transport is proven via QEMU markers (`apphost: dsl svc …`) once
//! shell/greeter launch as app-host (#17).

#![cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]

use alloc::string::String;
use alloc::vec::Vec;
use nexus_dsl_runtime::{EffectHost, Value};
use nexus_sdk_routes::{route_for_svc, CHILD_REPLY_RECV_SLOT, CHILD_REPLY_SEND_SLOT};

/// Stable effect error codes (surfaced to the DSL `Err(e)` arm) — the same
/// numbering windowd's host uses, so a program is host-agnostic.
const ERR_SVC_UNAVAILABLE: u32 = 1;
const ERR_SVC_UNKNOWN: u32 = 2;
const ERR_SVC_SHAPE: u32 = 3;

/// Per-call budget: the fixed slots are populated before resume, but the
/// backing service may still be finishing bring-up. Time-bounded (not
/// iteration-bounded) — the service decides when the reply lands.
const SVC_DEADLINE_NS: u64 = 2_000_000_000;
/// Reply-inbox scratch bound (list responses carry every entry).
const REPLY_BUF: usize = 512;

/// Proof marker that bypasses verdict folding (the app-host process arms
/// folding for every line via `nexus-service-entry`; the svc chain must stay
/// visible for boot verification).
fn raw_marker(line: &str) {
    let mut buf = [0u8; 96];
    let bytes = line.as_bytes();
    let n = bytes.len().min(buf.len() - 1);
    buf[..n].copy_from_slice(&bytes[..n]);
    buf[n] = b'\n';
    let _ = nexus_abi::debug_write(&buf[..n + 1]);
}

/// The DSL service host for a launched app. Holds the field symbol ids the
/// record shapes need (resolved once from the program's symbol table).
pub(crate) struct AppEffectHost {
    id_sym: Option<u32>,
    label_sym: Option<u32>,
}

impl AppEffectHost {
    pub(crate) fn new(symbols: &[String]) -> Self {
        Self {
            id_sym: symbols.iter().position(|s| s == "id").map(|i| i as u32),
            label_sym: symbols.iter().position(|s| s == "label").map(|i| i as u32),
        }
    }

    /// The SEND slot the route for `svc` landed in (execd provisioned it iff
    /// the manifest granted the backing permission). `None` = not routable.
    fn svc_send_slot(svc: &str) -> Option<u32> {
        route_for_svc(svc).map(|r| r.child_slot)
    }

    /// `svc.bundlemgr.enumerate(query)` → `List<AppEntry{ id, label }>` from
    /// the installed-bundle registry. Records are FIELD-SORTED by symbol id
    /// (the `Value::Record` contract).
    fn enumerate(&self) -> Result<Value, u32> {
        let (Some(id_sym), Some(label_sym)) = (self.id_sym, self.label_sym) else {
            raw_marker("apphost: dsl svc bundlemgr.enumerate FAIL (no id/label symbol)");
            return Err(ERR_SVC_SHAPE);
        };
        let send_slot = Self::svc_send_slot("bundlemgr").ok_or(ERR_SVC_UNKNOWN)?;
        let mut req = [0u8; 4];
        nexus_abi::bundlemgrd::encode_list_apps(&mut req);
        let mut resp = [0u8; REPLY_BUF];
        let Some(len) = call_reply(send_slot, &req, &mut resp) else {
            raw_marker("apphost: dsl svc bundlemgr.enumerate FAIL (registry unreachable)");
            return Err(ERR_SVC_UNAVAILABLE);
        };
        let entries = parse_app_entries(&resp[..len]);
        let rows: Vec<Value> = entries
            .into_iter()
            .map(|(id, label)| {
                let mut fields =
                    alloc::vec![(id_sym, Value::Str(id)), (label_sym, Value::Str(label))];
                fields.sort_by_key(|(sym, _)| *sym);
                Value::Record(fields)
            })
            .collect();
        raw_marker("apphost: dsl svc bundlemgr.enumerate ok");
        Ok(Value::List(rows))
    }

    /// `svc.session.users()` → `List<Str>` of greeter user display names.
    fn session_users(&self) -> Result<Value, u32> {
        let send_slot = Self::svc_send_slot("session").ok_or(ERR_SVC_UNKNOWN)?;
        let mut req = [0u8; 4];
        nexus_abi::sessiond::encode_get_state(&mut req);
        let mut resp = [0u8; REPLY_BUF];
        let Some(len) = call_reply(send_slot, &req, &mut resp) else {
            raw_marker("apphost: dsl svc session.users FAIL (sessiond unreachable)");
            return Err(ERR_SVC_UNAVAILABLE);
        };
        let names = parse_session_user_names(&resp[..len]);
        let rows: Vec<Value> = names.into_iter().map(Value::Str).collect();
        raw_marker("apphost: dsl svc session.users ok");
        Ok(Value::List(rows))
    }

    /// `svc.session.login(user_id)` → `Bool` (whether sessiond accepted it).
    fn session_login(&self, user: &str) -> Result<Value, u32> {
        let send_slot = Self::svc_send_slot("session").ok_or(ERR_SVC_UNKNOWN)?;
        let mut req = [0u8; 5 + 255];
        let Some(n) = nexus_abi::sessiond::encode_login_req(user.as_bytes(), &mut req) else {
            return Err(ERR_SVC_SHAPE);
        };
        let mut resp = [0u8; REPLY_BUF];
        let Some(len) = call_reply(send_slot, &req[..n], &mut resp) else {
            raw_marker("apphost: dsl svc session.login FAIL (sessiond unreachable)");
            return Err(ERR_SVC_UNAVAILABLE);
        };
        let ok = matches!(
            nexus_abi::sessiond::decode_login_rsp(&resp[..len]),
            Some((nexus_abi::sessiond::STATUS_OK, _))
        );
        raw_marker(if ok {
            "apphost: dsl svc session.login ok"
        } else {
            "apphost: dsl svc session.login denied"
        });
        Ok(Value::Bool(ok))
    }

    /// `svc.ability.launch(app_id)` → `Bool` (accepted for dispatch). Wire:
    /// `[A, M, ver, OP_LAUNCH, app_len, app…, abil_len, abil…]`. Fire-and-
    /// forget: abilitymgr owns lifecycle + emits the launch marker chain; the
    /// caller does not block on a reply (windowd's host has the same contract).
    fn ability_launch(&self, app_id: &str) -> Result<Value, u32> {
        let send_slot = Self::svc_send_slot("ability").ok_or(ERR_SVC_UNKNOWN)?;
        let app = app_id.as_bytes();
        const ABIL: &[u8] = b"main";
        if app.is_empty() || app.len() > u8::MAX as usize {
            return Err(ERR_SVC_SHAPE);
        }
        let mut req = Vec::with_capacity(6 + app.len() + ABIL.len());
        req.extend_from_slice(&[b'A', b'M', 1, 1]); // MAGIC, ver, OP_LAUNCH
        req.push(app.len() as u8);
        req.extend_from_slice(app);
        req.push(ABIL.len() as u8);
        req.extend_from_slice(ABIL);
        if send_fire_and_forget(send_slot, &req) {
            raw_marker("apphost: dsl svc ability.launch ok");
            Ok(Value::Bool(true))
        } else {
            raw_marker("apphost: dsl svc ability.launch FAIL (abilitymgr unreachable)");
            Err(ERR_SVC_UNAVAILABLE)
        }
    }
}

impl EffectHost for AppEffectHost {
    fn call(
        &mut self,
        service: &str,
        method: &str,
        args: &[Value],
        _timeout_ms: u32,
    ) -> Result<Value, u32> {
        match (service, method) {
            ("bundlemgr", "enumerate") => self.enumerate(),
            ("session", "users") => self.session_users(),
            ("session", "login") => {
                let user = args.first().and_then(str_of).ok_or(ERR_SVC_SHAPE)?;
                self.session_login(user)
            }
            ("ability", "launch") => {
                let id = args.first().and_then(str_of).ok_or(ERR_SVC_SHAPE)?;
                self.ability_launch(id)
            }
            _ => Err(ERR_SVC_UNKNOWN),
        }
    }
}

fn str_of(v: &Value) -> Option<&str> {
    match v {
        Value::Str(s) => Some(s.as_str()),
        _ => None,
    }
}

/// Fixed-slot request/reply over the child's provisioned `@reply` inbox: clone
/// the reply SEND (child slot 10), MOVE it into the request so the service
/// answers our inbox, send on `service_send_slot` (bounded), then receive on
/// the reply RECV (child slot 9). Returns the reply frame length, or `None` on
/// any send/recv failure or timeout (the caller renders the `Err` arm).
fn call_reply(service_send_slot: u32, req: &[u8], resp: &mut [u8]) -> Option<usize> {
    let reply_send = nexus_abi::cap_clone(CHILD_REPLY_SEND_SLOT).ok()?;
    let hdr = nexus_abi::MsgHeader::new(
        reply_send,
        0,
        0,
        nexus_abi::ipc_hdr::CAP_MOVE,
        req.len() as u32,
    );
    let start = nexus_abi::nsec().unwrap_or(0);
    let deadline = start.saturating_add(SVC_DEADLINE_NS);

    let mut sent = false;
    loop {
        match nexus_abi::ipc_send_v1(service_send_slot, &hdr, req, nexus_abi::IPC_SYS_NONBLOCK, 0) {
            Ok(_) => {
                sent = true;
                break;
            }
            Err(nexus_abi::IpcError::QueueFull) => {
                if nexus_abi::nsec().unwrap_or(u64::MAX) >= deadline {
                    break;
                }
                let _ = nexus_abi::yield_();
            }
            Err(_) => break,
        }
    }
    // Reclaims the clone on a failed send; a successful CAP_MOVE already
    // consumed it, so this is a harmless no-op there (registry_client pattern).
    let _ = nexus_abi::cap_close(reply_send);
    if !sent {
        return None;
    }

    loop {
        let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        match nexus_abi::ipc_recv_v1(
            CHILD_REPLY_RECV_SLOT,
            &mut rh,
            resp,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => return Some((n as usize).min(resp.len())),
            Err(nexus_abi::IpcError::QueueEmpty) => {
                if nexus_abi::nsec().unwrap_or(u64::MAX) >= deadline {
                    return None;
                }
                let _ = nexus_abi::yield_();
            }
            Err(_) => return None,
        }
    }
}

/// Bounded fire-and-forget send on a provisioned SEND slot (no reply awaited).
fn send_fire_and_forget(send_slot: u32, req: &[u8]) -> bool {
    let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, req.len() as u32);
    let start = nexus_abi::nsec().unwrap_or(0);
    let deadline = start.saturating_add(SVC_DEADLINE_NS);
    loop {
        match nexus_abi::ipc_send_v1(send_slot, &hdr, req, nexus_abi::IPC_SYS_NONBLOCK, 0) {
            Ok(_) => return true,
            Err(nexus_abi::IpcError::QueueFull) => {
                if nexus_abi::nsec().unwrap_or(u64::MAX) >= deadline {
                    return false;
                }
                let _ = nexus_abi::yield_();
            }
            Err(_) => return false,
        }
    }
}

/// Parses the `OP_LIST_APPS` response body into `(id, label)` pairs. Header +
/// per-entry length-prefixed strings (`[id_len, id, label_len, label]`); a
/// malformed/short frame yields the entries parsed so far (fail-soft, bounded).
fn parse_app_entries(frame: &[u8]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Some((status, count)) = nexus_abi::bundlemgrd::decode_list_apps_header(frame) else {
        return out;
    };
    if status != nexus_abi::bundlemgrd::STATUS_OK {
        return out;
    }
    let mut pos = nexus_abi::bundlemgrd::LIST_APPS_BODY_OFFSET;
    for _ in 0..count {
        let Some(id) = take_lp_str(frame, &mut pos) else { break };
        let Some(label) = take_lp_str(frame, &mut pos) else { break };
        out.push((id, label));
    }
    out
}

/// Parses the sessiond `OP_GET_STATE` response into user DISPLAY NAMES. Each
/// entry is `[id_len, id, name_len, name, product_len, product]`; we keep the
/// name (the greeter renders it). Fail-soft like [`parse_app_entries`].
fn parse_session_user_names(frame: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let Some((status, _state, _active, count)) =
        nexus_abi::sessiond::decode_get_state_header(frame)
    else {
        return out;
    };
    if status != nexus_abi::sessiond::STATUS_OK {
        return out;
    }
    let mut pos = nexus_abi::sessiond::GET_STATE_BODY_OFFSET;
    for _ in 0..count {
        let id = take_lp_str(frame, &mut pos);
        let name = take_lp_str(frame, &mut pos);
        let _product = take_lp_str(frame, &mut pos);
        // The rows feed `Pick(user)` → `svc.session.login(user)` — login
        // needs the USER ID, not the display name (returning the name made
        // every DSL login `UNKNOWN_USER`-denied). The id doubles as the
        // display string until `session.users` grows a record row
        // ({id, label}, like bundlemgr's AppEntry) in the service surface.
        match id {
            Some(id) if name.is_some() && _product.is_some() => out.push(id),
            _ => break,
        }
    }
    out
}

/// Reads a `[len:u8, bytes…]` UTF-8 string, advancing `pos`. `None` on a short
/// frame or invalid UTF-8 (the bound the callers stop on).
fn take_lp_str(frame: &[u8], pos: &mut usize) -> Option<String> {
    let len = *frame.get(*pos)? as usize;
    let start = pos.checked_add(1)?;
    let end = start.checked_add(len)?;
    let bytes = frame.get(start..end)?;
    *pos = end;
    core::str::from_utf8(bytes).ok().map(String::from)
}
