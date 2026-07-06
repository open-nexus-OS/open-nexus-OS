// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The queryd request handler: permission gate (fail-closed) →
//! namespace derivation (caller identity → key prefix; the wire cannot name
//! a namespace) → engine execution over a prefix-scoped Kv view.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: tests/loopback.rs

use crate::wire::{
    decode_col_type, decode_qval, encode_err, encode_qval, to_bytes, OP_CREATE_TABLE,
    OP_DELETE, OP_PUT, OP_QUERY,
};
use nexus_idl_runtime::queryspec_capnp as ws;
use nexus_query::{Engine, Kv, MemKv, PageToken, QueryError, QuerySpec, Range, TableDef};
use std::collections::BTreeMap;

/// The permission gate. Production wires this to abilitymgr's grant set;
/// the default answer is NO (fail-closed).
pub trait Caps {
    fn query_allowed(&self, app_id: &str) -> bool;
}

/// Denies everything — the fail-closed default.
pub struct DenyAll;

impl Caps for DenyAll {
    fn query_allowed(&self, _app_id: &str) -> bool {
        false
    }
}

/// A fixed allow-list (host tests, bring-up).
pub struct StaticCaps {
    allowed: Vec<String>,
}

impl StaticCaps {
    #[must_use]
    pub fn new(allowed: &[&str]) -> Self {
        Self { allowed: allowed.iter().map(|s| String::from(*s)).collect() }
    }
}

impl Caps for StaticCaps {
    fn query_allowed(&self, app_id: &str) -> bool {
        self.allowed.iter().any(|a| a == app_id)
    }
}

/// One table's schema + the column names the DSL speaks.
struct TableInfo {
    def: TableDef,
    names: Vec<String>,
}

/// The service state: one shared ordered KV, tables cataloged per
/// (namespace, table id). Host skeleton runs over [`MemKv`]; the
/// statefsd-journal Kv slots in behind the same seam (Phase 6).
pub struct QuerydServer<C: Caps> {
    kv: MemKv,
    tables: BTreeMap<(String, u16), TableInfo>,
    caps: C,
}

/// A namespace-scoped view: every key is prefixed with the caller's derived
/// namespace, so two apps' identical engine keys can never collide — and
/// scans stay inside the prefix by construction.
struct NsKv<'a> {
    inner: &'a mut MemKv,
    prefix: Vec<u8>,
}

impl NsKv<'_> {
    fn wrap(&self, key: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.prefix.len() + key.len());
        out.extend_from_slice(&self.prefix);
        out.extend_from_slice(key);
        out
    }
}

impl Kv for NsKv<'_> {
    fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.inner.get(&self.wrap(key))
    }

    fn put(&mut self, key: &[u8], value: &[u8]) {
        let key = self.wrap(key);
        self.inner.put(&key, value);
    }

    fn delete(&mut self, key: &[u8]) {
        let key = self.wrap(key);
        self.inner.delete(&key);
    }

    fn scan(&self, start: &[u8], end: &[u8], limit: usize) -> Vec<(Vec<u8>, Vec<u8>)> {
        self.inner
            .scan(&self.wrap(start), &self.wrap(end), limit)
            .into_iter()
            .map(|(k, v)| (k[self.prefix.len()..].to_vec(), v))
            .collect()
    }

    fn scan_rev(&self, start: &[u8], end: &[u8], limit: usize) -> Vec<(Vec<u8>, Vec<u8>)> {
        self.inner
            .scan_rev(&self.wrap(start), &self.wrap(end), limit)
            .into_iter()
            .map(|(k, v)| (k[self.prefix.len()..].to_vec(), v))
            .collect()
    }
}

/// Caller identity → namespace prefix. Identity comes from the transport
/// (bundle id via the IPC boundary), NEVER from the request payload.
fn namespace_prefix(app_id: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(app_id.len() + 4);
    out.extend_from_slice(b"ns:");
    out.extend_from_slice(app_id.as_bytes());
    out.push(0x00);
    out
}

impl<C: Caps> QuerydServer<C> {
    #[must_use]
    pub fn new(caps: C) -> Self {
        Self { kv: MemKv::new(), tables: BTreeMap::new(), caps }
    }

    /// Handles one `[opcode u8][capnp request]` frame from `app_id` and
    /// returns the capnp response bytes (`AckResponse` for writes,
    /// `QueryResponse` for queries). Every failure path is a typed wire
    /// error — never a panic, never a silent default.
    pub fn handle_frame(&mut self, app_id: &str, frame: &[u8]) -> Vec<u8> {
        let Some((&opcode, body)) = frame.split_first() else {
            return ack_err(ws::QueryErr::BadRequest);
        };
        if !self.caps.query_allowed(app_id) {
            // Fail-closed BEFORE touching the payload.
            return match opcode {
                OP_QUERY => query_err(ws::QueryErr::Denied),
                _ => ack_err(ws::QueryErr::Denied),
            };
        }
        match opcode {
            OP_CREATE_TABLE => self.create_table(app_id, body),
            OP_PUT => self.put(app_id, body),
            OP_DELETE => self.delete(app_id, body),
            OP_QUERY => self.query(app_id, body),
            _ => ack_err(ws::QueryErr::BadRequest),
        }
    }

    fn create_table(&mut self, app_id: &str, body: &[u8]) -> Vec<u8> {
        let Ok(message) = capnp::serialize::read_message(body, Default::default()) else {
            return ack_err(ws::QueryErr::BadRequest);
        };
        let Ok(req) = message.get_root::<ws::create_table_request::Reader<'_>>() else {
            return ack_err(ws::QueryErr::BadRequest);
        };
        let (Ok(names), Ok(types), Ok(indexed)) =
            (req.get_names(), req.get_types(), req.get_indexed())
        else {
            return ack_err(ws::QueryErr::BadRequest);
        };
        if names.len() != types.len() || req.get_pk_col() as u32 >= types.len() {
            return ack_err(ws::QueryErr::BadRequest);
        }
        let mut columns = Vec::with_capacity(types.len() as usize);
        for ty in types.iter() {
            let Ok(ty) = ty else { return ack_err(ws::QueryErr::BadRequest) };
            columns.push(decode_col_type(ty));
        }
        let mut col_names = Vec::with_capacity(names.len() as usize);
        for name in names.iter() {
            let Ok(name) = name.and_then(|n| n.to_str().map_err(Into::into)) else {
                return ack_err(ws::QueryErr::BadRequest);
            };
            col_names.push(String::from(name));
        }
        let def = TableDef {
            id: req.get_table(),
            columns,
            pk_col: req.get_pk_col() as usize,
            indexed: indexed.iter().map(|i| i as usize).collect(),
        };
        self.tables.insert(
            (String::from(app_id), req.get_table()),
            TableInfo { def, names: col_names },
        );
        ack_ok()
    }

    fn engine_for(&self, app_id: &str) -> Engine {
        Engine::new(
            self.tables
                .iter()
                .filter(|((ns, _), _)| ns == app_id)
                .map(|(_, info)| info.def.clone())
                .collect(),
        )
    }

    fn put(&mut self, app_id: &str, body: &[u8]) -> Vec<u8> {
        let Ok(message) = capnp::serialize::read_message(body, Default::default()) else {
            return ack_err(ws::QueryErr::BadRequest);
        };
        let Ok(req) = message.get_root::<ws::put_request::Reader<'_>>() else {
            return ack_err(ws::QueryErr::BadRequest);
        };
        let Ok(row_list) = req.get_row() else { return ack_err(ws::QueryErr::BadRequest) };
        let mut row = Vec::with_capacity(row_list.len() as usize);
        for value in row_list.iter() {
            match decode_qval(value) {
                Ok(v) => row.push(v),
                Err(e) => return ack_err(encode_err(e)),
            }
        }
        let engine = self.engine_for(app_id);
        let mut kv = NsKv { inner: &mut self.kv, prefix: namespace_prefix(app_id) };
        match engine.put(&mut kv, req.get_table(), &row) {
            Ok(()) => ack_ok(),
            Err(e) => ack_err(encode_err(e)),
        }
    }

    fn delete(&mut self, app_id: &str, body: &[u8]) -> Vec<u8> {
        let Ok(message) = capnp::serialize::read_message(body, Default::default()) else {
            return ack_err(ws::QueryErr::BadRequest);
        };
        let Ok(req) = message.get_root::<ws::delete_request::Reader<'_>>() else {
            return ack_err(ws::QueryErr::BadRequest);
        };
        let pk = match req.get_pk().map_err(|_| QueryError::Unsupported).and_then(decode_qval)
        {
            Ok(v) => v,
            Err(e) => return ack_err(encode_err(e)),
        };
        let engine = self.engine_for(app_id);
        let mut kv = NsKv { inner: &mut self.kv, prefix: namespace_prefix(app_id) };
        match engine.delete(&mut kv, req.get_table(), &pk) {
            Ok(()) => ack_ok(),
            Err(e) => ack_err(encode_err(e)),
        }
    }

    fn query(&mut self, app_id: &str, body: &[u8]) -> Vec<u8> {
        let Ok(message) = capnp::serialize::read_message(body, Default::default()) else {
            return query_err(ws::QueryErr::BadRequest);
        };
        let Ok(req) = message.get_root::<ws::query_request::Reader<'_>>() else {
            return query_err(ws::QueryErr::BadRequest);
        };
        let table = req.get_table();
        let Some(info) = self.tables.get(&(String::from(app_id), table)) else {
            return query_err(ws::QueryErr::UnknownTable);
        };
        let col_index = |name: &str| info.names.iter().position(|n| n == name);
        let Ok(order_col_name) =
            req.get_order_col().and_then(|t| t.to_str().map_err(Into::into))
        else {
            return query_err(ws::QueryErr::BadRequest);
        };
        let Some(order_col) = col_index(order_col_name) else {
            return query_err(ws::QueryErr::UnknownColumn);
        };
        let mut spec = QuerySpec {
            table,
            eq: Vec::new(),
            range: None,
            order_col,
            descending: req.get_descending(),
            limit: req.get_limit(),
        };
        let Ok(preds) = req.get_preds() else { return query_err(ws::QueryErr::BadRequest) };
        let mut low = None;
        let mut high = None;
        for pred in preds.iter() {
            let Ok(col_name) = pred.get_col().and_then(|t| t.to_str().map_err(Into::into))
            else {
                return query_err(ws::QueryErr::BadRequest);
            };
            let Some(col) = col_index(col_name) else {
                return query_err(ws::QueryErr::UnknownColumn);
            };
            let value = match pred.get_value().map_err(|_| QueryError::Unsupported).and_then(decode_qval) {
                Ok(v) => v,
                Err(e) => return query_err(encode_err(e)),
            };
            match pred.get_op() {
                Ok(ws::QueryOp::Eq) => spec.eq.push((col, value)),
                Ok(ws::QueryOp::Ge) if col == order_col => low = Some(value),
                Ok(ws::QueryOp::Le) if col == order_col => high = Some(value),
                // Range off the order column / unknown op = outside v1.
                _ => return query_err(ws::QueryErr::Unsupported),
            }
        }
        if low.is_some() || high.is_some() {
            spec.range = Some(Range { low, high });
        }
        let token = match req.get_token() {
            Ok([]) => None,
            Ok(bytes) => match PageToken::from_bytes(bytes) {
                Some(t) => Some(t),
                None => return query_err(ws::QueryErr::BadToken),
            },
            Err(_) => return query_err(ws::QueryErr::BadRequest),
        };

        let engine = self.engine_for(app_id);
        let kv = NsKv { inner: &mut self.kv, prefix: namespace_prefix(app_id) };
        match engine.query(&kv, &spec, token.as_ref()) {
            Ok(page) => {
                let mut message = capnp::message::Builder::new_default();
                {
                    let response = message.init_root::<ws::query_response::Builder<'_>>();
                    let mut ok = response.init_ok();
                    {
                        let mut rows = ok.reborrow().init_rows(page.rows.len() as u32);
                        for (i, row) in page.rows.iter().enumerate() {
                            let mut values = rows
                                .reborrow()
                                .get(i as u32)
                                .init_values(row.len() as u32);
                            for (j, value) in row.iter().enumerate() {
                                encode_qval(value, values.reborrow().get(j as u32));
                            }
                        }
                    }
                    match &page.next {
                        Some(next) => ok.set_next(next.as_bytes()),
                        None => ok.set_next(&[]),
                    }
                }
                to_bytes(&message)
            }
            Err(e) => query_err(encode_err(e)),
        }
    }
}

fn ack_ok() -> Vec<u8> {
    let mut message = capnp::message::Builder::new_default();
    message.init_root::<ws::ack_response::Builder<'_>>().set_ok(());
    to_bytes(&message)
}

fn ack_err(err: ws::QueryErr) -> Vec<u8> {
    let mut message = capnp::message::Builder::new_default();
    message.init_root::<ws::ack_response::Builder<'_>>().set_err(err);
    to_bytes(&message)
}

fn query_err(err: ws::QueryErr) -> Vec<u8> {
    let mut message = capnp::message::Builder::new_default();
    message.init_root::<ws::query_response::Builder<'_>>().set_err(err);
    to_bytes(&message)
}
