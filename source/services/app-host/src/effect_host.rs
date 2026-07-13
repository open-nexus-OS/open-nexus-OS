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
use nexus_dsl_runtime::{EffectHost, QueryCall, QueryPage, Value};
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
    seq_sym: Option<u32>,
    text_sym: Option<u32>,
    /// Lazily seeded in-process query store (`EffectHost::query()`). Same
    /// engine queryd hosts; keyset paging = the DSL's lazy-loading window.
    query_store: Option<QueryStore>,
}

/// The embedded `nexus-query` engine + its KV. v1 catalog: one demo
/// `messages` table (seq Int pk/order, text Str) seeded with a large
/// transcript — the scrolling/lazy-loading proof corpus until statefsd-backed
/// tables land (@persist wiring).
struct QueryStore {
    engine: nexus_query::Engine,
    kv: nexus_query::MemKv,
}

/// Demo transcript scale: large enough that only WINDOWS of it are ever
/// resident in the DSL store (the lazy-loading contract), small enough to
/// seed in one bounded pass.
// Demo-source size: the synthetic generator below is zero-resident (only the
// requested page is materialized), so this is just the upper bound of the
// transcript. The store-window builtin `tail(list, 96)` in chat.store.nx keeps
// the resident `messages` list (and the derived emit/layout/paint/concat cost)
// bounded, so paging this far no longer grows unbounded. The ceiling now is the
// non-freeing bump heap's tolerance for the per-page whole-scene re-emit churn
// (see chat.store.nx); 300 pages cleanly, unbounded needs emit-virtualization.
const SEED_MESSAGES: i64 = 300;

impl QueryStore {
    fn seeded() -> Self {
        use nexus_query::{QType, QVal, TableDef};
        let engine = nexus_query::Engine::new(alloc::vec![TableDef {
            id: 0,
            columns: alloc::vec![QType::Int, QType::Str],
            pk_col: 0,
            indexed: alloc::vec![0],
        }]);
        let mut kv = nexus_query::MemKv::new();
        // Deterministic two-voice transcript (no external data source yet).
        const LINES: [&str; 6] = [
            "Hast du den neuen Build schon gebootet?",
            "Ja - der Frost-Effekt sitzt jetzt richtig.",
            "Dann teste mal drei Fenster gleichzeitig.",
            "Laeuft. Fokus und Drag fuehlen sich gut an.",
            "Als naechstes kommt das lange Transcript.",
            "Genau dafuer ist diese Nachricht da.",
        ];
        for seq in 1..=SEED_MESSAGES {
            let voice = if seq % 2 == 0 { "Du" } else { "Mira" };
            let line = LINES[(seq as usize) % LINES.len()];
            let mut text = String::new();
            let _ = core::fmt::write(
                &mut text,
                format_args!("{voice} #{seq}: {line}"),
            );
            let _ = engine.put(&mut kv, 0, &[QVal::Int(seq), QVal::Str(text)]);
        }
        Self { engine, kv }
    }

    /// Column index of `name` in the `messages` table.
    fn col(name: &str) -> usize {
        match name {
            "text" => 1,
            _ => 0, // seq (pk/order)
        }
    }
}

impl AppEffectHost {
    pub(crate) fn new(symbols: &[String]) -> Self {
        Self {
            id_sym: symbols.iter().position(|s| s == "id").map(|i| i as u32),
            label_sym: symbols.iter().position(|s| s == "label").map(|i| i as u32),
            seq_sym: symbols.iter().position(|s| s == "seq").map(|i| i as u32),
            text_sym: symbols.iter().position(|s| s == "text").map(|i| i as u32),
            query_store: None,
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
            // Wait-cursor hint: tell the compositor a launch is pending so it
            // shows the loading ring until the fresh window's surface arrives
            // (fire-and-forget on the surface request slot — losing it only
            // skips the ring).
            {
                use nexus_display_proto::client_surface as wire;
                let frame = wire::encode_surface_control(wire::CONTROL_LAUNCH_PENDING, 0);
                const WINDOWD_SEND_SLOT: u32 = 5;
                let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, frame.len() as u32);
                let _ = nexus_abi::ipc_send_v1(
                    WINDOWD_SEND_SLOT,
                    &hdr,
                    &frame,
                    nexus_abi::IPC_SYS_NONBLOCK,
                    0,
                );
            }
            Ok(Value::Bool(true))
        } else {
            raw_marker("apphost: dsl svc ability.launch FAIL (abilitymgr unreachable)");
            Err(ERR_SVC_UNAVAILABLE)
        }
    }
    /// `svc.settings.get(key)` → the registered value (settingsd typed
    /// registry; unknown keys are a shape error, never a silent default).
    fn settings_get(&self, key: &str) -> Result<Value, u32> {
        use nexus_abi::settingsd as sw;
        let send_slot = Self::svc_send_slot("settings").ok_or(ERR_SVC_UNKNOWN)?;
        let mut req = [0u8; 300];
        let n = sw::encode_get_req(key, &mut req).ok_or(ERR_SVC_SHAPE)?;
        let mut resp = [0u8; REPLY_BUF];
        let Some(len) = call_reply(send_slot, &req[..n], &mut resp) else {
            raw_marker("apphost: dsl svc settings.get FAIL (settingsd unreachable)");
            return Err(ERR_SVC_UNAVAILABLE);
        };
        match sw::decode_response(sw::OP_GET, &resp[..len]) {
            Some((sw::STATUS_OK, value)) => {
                raw_marker("apphost: dsl svc settings.get ok");
                Ok(Value::Str(String::from(value)))
            }
            _ => {
                raw_marker("apphost: dsl svc settings.get FAIL (status)");
                Err(ERR_SVC_SHAPE)
            }
        }
    }

    /// `svc.settings.set(key, value)` → `Bool` (validated + persisted).
    ///
    /// PRESENTATION keys (`ui.theme.mode`, `ui.shell.mode`) route to windowd
    /// (`OP_SURFACE_CONTROL`) instead of settingsd: the compositor is the
    /// single presentation authority — it applies the change LIVE and
    /// persists via settingsd itself, so a toggle can never desynchronize
    /// the desktop from the stored value.
    fn settings_set(&self, key: &str, value: &str) -> Result<Value, u32> {
        use nexus_abi::settingsd as sw;
        if key == sw::KEY_UI_THEME_MODE || key == sw::KEY_UI_SHELL_MODE {
            return self.presentation_control(key, value);
        }
        let send_slot = Self::svc_send_slot("settings").ok_or(ERR_SVC_UNKNOWN)?;
        let mut req = [0u8; 300];
        let n = sw::encode_set_req(key, value, &mut req).ok_or(ERR_SVC_SHAPE)?;
        let mut resp = [0u8; REPLY_BUF];
        let Some(len) = call_reply(send_slot, &req[..n], &mut resp) else {
            raw_marker("apphost: dsl svc settings.set FAIL (settingsd unreachable)");
            return Err(ERR_SVC_UNAVAILABLE);
        };
        let ok = matches!(sw::decode_response(sw::OP_SET, &resp[..len]), Some((sw::STATUS_OK, _)));
        raw_marker(if ok {
            "apphost: dsl svc settings.set ok"
        } else {
            "apphost: dsl svc settings.set FAIL (status)"
        });
        Ok(Value::Bool(ok))
    }

    /// Sends a presentation control to windowd on the surface request channel
    /// (fire-and-forget NONBLOCK; windowd pushes the resulting theme/profile
    /// back over the event channel, which re-mounts the view).
    fn presentation_control(&self, key: &str, value: &str) -> Result<Value, u32> {
        use nexus_abi::settingsd as sw;
        use nexus_display_proto::client_surface as wire;
        let (control, v) = if key == sw::KEY_UI_THEME_MODE {
            let v = if value == "light" { wire::THEME_LIGHT } else { wire::THEME_DARK };
            (wire::CONTROL_THEME, v)
        } else {
            let v = if value == "desktop" { wire::PROFILE_DESKTOP } else { wire::PROFILE_TABLET };
            (wire::CONTROL_SHELL_PROFILE, v)
        };
        let frame = wire::encode_surface_control(control, v);
        // The windowd surface request slot (main.rs WINDOWD_SEND_SLOT).
        const WINDOWD_SEND_SLOT: u32 = 5;
        let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, frame.len() as u32);
        match nexus_abi::ipc_send_v1(WINDOWD_SEND_SLOT, &hdr, &frame, nexus_abi::IPC_SYS_NONBLOCK, 0)
        {
            Ok(_) => {
                raw_marker("apphost: dsl svc settings.set control ok");
                Ok(Value::Bool(true))
            }
            Err(_) => {
                raw_marker("apphost: dsl svc settings.set control FAIL (send)");
                Err(ERR_SVC_UNAVAILABLE)
            }
        }
    }

}

fn to_qval(value: &Value) -> Option<nexus_query::QVal> {
    match value {
        Value::Bool(b) => Some(nexus_query::QVal::Bool(*b)),
        Value::Int(i) => Some(nexus_query::QVal::Int(*i)),
        Value::Fx(f) => Some(nexus_query::QVal::Fx(*f)),
        Value::Str(s) => Some(nexus_query::QVal::Str(s.clone())),
        _ => None,
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0xf) as usize] as char);
    }
    out
}

fn hex_decode(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 {
        return None;
    }
    (0..text.len() / 2)
        .map(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).ok())
        .collect()
}

impl EffectHost for AppEffectHost {
    fn query(&mut self, call: &QueryCall) -> Result<QueryPage, u32> {
        if call.source != "messages" {
            return Err(ERR_SVC_UNKNOWN);
        }
        let (Some(seq_sym), Some(text_sym)) = (self.seq_sym, self.text_sym) else {
            raw_marker("apphost: dsl query FAIL (no seq/text symbol)");
            return Err(ERR_SVC_SHAPE);
        };
        // Demo `messages` source: rows are GENERATED from the keyset cursor —
        // no in-process KV. The 1000-row MemKv seed cost ~½ of the app heap
        // and, together with scene+layout growth, page-faulted the 2MB bump
        // heap ("wieder ein absturz"). A synthetic source is the honest
        // placeholder until statefsd-backed tables land: identical paging
        // contract (token = last seq, `""` at the end), ZERO resident bytes,
        // arbitrarily large. The real engine path stays proven by the
        // dsl_conformance EngineHost tests.
        let start: i64 = if call.token.is_empty() {
            1
        } else {
            call.token.parse::<i64>().map_err(|_| ERR_SVC_SHAPE)?.saturating_add(1)
        };
        let limit = call.limit.clamp(1, 200) as i64;
        let end = (start + limit - 1).min(SEED_MESSAGES);
        if start > SEED_MESSAGES {
            raw_marker("apphost: dsl query messages page ok");
            // Past the end: ECHO the cursor (never ""). An empty `next` is
            // ambiguous with the mount cursor, so returning it here would make
            // the next LoadMore re-page from seq 1 — re-materializing the whole
            // transcript and, on the non-freeing heap, eventually OOM'ing. A
            // stable non-empty cursor keeps over-scroll an empty no-op.
            return Ok(QueryPage { rows: Value::List(Vec::new()), next: call.token.clone() });
        }
        const LINES: [&str; 6] = [
            "Hast du den neuen Build schon gebootet?",
            "Ja - der Frost-Effekt sitzt jetzt richtig.",
            "Dann teste mal drei Fenster gleichzeitig.",
            "Laeuft. Fokus und Drag fuehlen sich gut an.",
            "Als naechstes kommt das lange Transcript.",
            "Genau dafuer ist diese Nachricht da.",
        ];
        let mut rows: Vec<Value> = Vec::with_capacity((end - start + 1) as usize);
        for seq in start..=end {
            let voice = if seq % 2 == 0 { "Du" } else { "Mira" };
            let line = LINES[(seq as usize) % LINES.len()];
            let mut text = String::new();
            let _ = core::fmt::write(&mut text, format_args!("{voice} #{seq}: {line}"));
            let mut fields: Vec<(u32, Value)> =
                alloc::vec![(seq_sym, Value::Int(seq)), (text_sym, Value::Str(text))];
            fields.sort_by_key(|(sym, _)| *sym);
            rows.push(Value::Record(fields));
        }
        // The cursor is ALWAYS the last returned seq (never "" — see the
        // past-the-end branch above). At the end this pins the cursor at
        // SEED_MESSAGES, so a further LoadMore reads start>end → empty no-op
        // instead of restarting the transcript from the first page.
        let mut next = String::new();
        let _ = core::fmt::write(&mut next, format_args!("{end}"));
        raw_marker("apphost: dsl query messages page ok");
        return Ok(QueryPage { rows: Value::List(rows), next });
        // Unreachable engine path below is kept for the NEXT real table
        // source (statefsd-backed) — see QueryStore.
        #[allow(unreachable_code)]
        {
        use nexus_query::{PageToken, QuerySpec, Range};
        let store = self.query_store.get_or_insert_with(QueryStore::seeded);
        let mut spec = QuerySpec {
            table: 0,
            eq: call
                .eq
                .iter()
                .map(|(name, v)| Some((QueryStore::col(name), to_qval(v)?)))
                .collect::<Option<Vec<_>>>()
                .ok_or(ERR_SVC_SHAPE)?,
            range: None,
            order_col: QueryStore::col(&call.order_col),
            descending: call.descending,
            limit: call.limit,
        };
        if call.low.is_some() || call.high.is_some() {
            spec.range = Some(Range {
                low: call.low.as_ref().and_then(to_qval),
                high: call.high.as_ref().and_then(to_qval),
            });
        }
        let token = if call.token.is_empty() {
            None
        } else {
            Some(
                hex_decode(&call.token)
                    .and_then(|b| PageToken::from_bytes(&b))
                    .ok_or(ERR_SVC_SHAPE)?,
            )
        };
        let page = store
            .engine
            .query(&store.kv, &spec, token.as_ref())
            .map_err(|_| ERR_SVC_UNAVAILABLE)?;
        let rows: Vec<Value> = page
            .rows
            .into_iter()
            .map(|row| {
                let mut fields: Vec<(u32, Value)> = row
                    .into_iter()
                    .enumerate()
                    .map(|(i, qv)| {
                        let sym = if i == 0 { seq_sym } else { text_sym };
                        let v = match qv {
                            nexus_query::QVal::Int(n) => Value::Int(n),
                            nexus_query::QVal::Str(s) => Value::Str(s),
                            nexus_query::QVal::Bool(b) => Value::Bool(b),
                            nexus_query::QVal::Fx(f) => Value::Fx(f),
                        };
                        (sym, v)
                    })
                    .collect();
                fields.sort_by_key(|(s, _)| *s);
                Value::Record(fields)
            })
            .collect();
        raw_marker("apphost: dsl query messages page ok");
        Ok(QueryPage {
            rows: Value::List(rows),
            next: page.next.map(|t| hex_encode(t.as_bytes())).unwrap_or_default(),
        })
        }
    }

    fn call(
        &mut self,
        service: &str,
        method: &str,
        args: &[Value],
        _timeout_ms: u32,
    ) -> Result<Value, u32> {
        match (service, method) {
            ("bundlemgr", "enumerate") => self.enumerate(),
            ("settings", "get") => {
                let key = args.first().and_then(str_of).ok_or(ERR_SVC_SHAPE)?;
                self.settings_get(key)
            }
            ("settings", "set") => {
                let key = args.first().and_then(str_of).ok_or(ERR_SVC_SHAPE)?;
                let value = args.get(1).and_then(str_of).ok_or(ERR_SVC_SHAPE)?;
                self.settings_set(key, value)
            }
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
