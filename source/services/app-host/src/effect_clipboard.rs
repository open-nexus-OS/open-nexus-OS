// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: the `svc.clipboard.*` surface of the app-host DSL `EffectHost`
//! (RFC-0094, TASK-0067) — thin verbs over clipboardd on the child slot the
//! `CLIPBOARD` permission landed the route in: `list(query)` (the history,
//! newest first, filtered AT the service), `read(seq)` (one item's full text;
//! 0 = the newest), `write(text)`, `restore(seq)` (copy-back) and `clear()`.
//! NO clipboard logic here: storage and the focus gate live in clipboardd.
//! A gate refusal maps to `ERR_SVC_DENIED` so a page renders the honest
//! state; contents never reach a marker.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: record shape mirrored by the shell/ime-ui conformance fixtures;
//!   transport proven by `apphost: dsl svc clipboard.restore ok (seq=…)` on the
//!   visible lane

#![cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]

use super::effect_host::{
    call_reply, int_of, raw_marker, str_of, AppEffectHost, ERR_SVC_DENIED, ERR_SVC_SHAPE,
    ERR_SVC_UNAVAILABLE, ERR_SVC_UNKNOWN,
};
use alloc::string::String;
use alloc::vec::Vec;
use nexus_dsl_runtime::Value;
use nexus_wire::clipboardd as wire;

impl AppEffectHost {
    /// One `svc.clipboard.<method>` call (the dispatch keeps one line for the namespace).
    pub(crate) fn clipboard_call(&self, method: &str, args: &[Value]) -> Result<Value, u32> {
        let int_arg = || args.first().and_then(int_of).ok_or(ERR_SVC_SHAPE);
        match method {
            "list" => self.clipboard_list(args.first().and_then(str_of).unwrap_or("")),
            "read" => self.clipboard_read(int_arg()?),
            "write" => self.clipboard_write(args.first().and_then(str_of).ok_or(ERR_SVC_SHAPE)?),
            "restore" => self.clipboard_restore(int_arg()?),
            "clear" => self.clipboard_clear(),
            _ => Err(ERR_SVC_UNKNOWN),
        }
    }

    fn clipboard_exchange(&self, req: &[u8], resp: &mut [u8]) -> Result<usize, u32> {
        let send_slot = Self::svc_send_slot("clipboard").ok_or(ERR_SVC_UNKNOWN)?;
        call_reply(send_slot, req, resp).ok_or_else(|| {
            raw_marker("apphost: dsl svc clipboard FAIL (clipboardd unreachable)");
            ERR_SVC_UNAVAILABLE
        })
    }

    /// `svc.clipboard.list(query)` → `List<ClipEntry>` newest first: `seq` (Int) and
    /// `text` (the preview, ≤ 120 bytes) — only the fields the page reads.
    fn clipboard_list(&self, query: &str) -> Result<Value, u32> {
        let mut req = [0u8; 4 + 1 + wire::QUERY_MAX_BYTES];
        // An over-long query is the user's typing, not a shape error: it matches nothing
        // longer than the bound, so the first QUERY_MAX_BYTES (on a char boundary) decide.
        let mut cut = query.len().min(wire::QUERY_MAX_BYTES);
        while !query.is_char_boundary(cut) {
            cut -= 1;
        }
        let n = wire::encode_list(&query[..cut], &mut req).ok_or(ERR_SVC_SHAPE)?;
        let mut resp = [0u8; wire::REPLY_MAX_BYTES];
        let len = self.clipboard_exchange(&req[..n], &mut resp)?;
        let Some((status, _count, packed)) = wire::decode_list_reply(wire::OP_LIST, &resp[..len])
        else {
            raw_marker("apphost: dsl svc clipboard.list FAIL (shape)");
            return Err(ERR_SVC_SHAPE);
        };
        if status == wire::STATUS_DENIED {
            raw_marker("apphost: dsl svc clipboard.list FAIL (denied)");
            return Err(ERR_SVC_DENIED);
        }
        let rows: Vec<Value> = wire::unpack_list_entries(packed)
            .map(|e| {
                let mut fields: Vec<(u32, Value)> = Vec::with_capacity(2);
                if let Some(sym) = self.seq_sym {
                    fields.push((sym, Value::Int(i64::try_from(e.seq).unwrap_or(i64::MAX))));
                }
                if let Some(sym) = self.text_sym {
                    fields.push((sym, Value::Str(String::from(e.preview))));
                }
                // `Value::Record` contract: FIELD-SORTED by symbol id.
                fields.sort_by_key(|(sym, _)| *sym);
                Value::Record(fields)
            })
            .collect();
        Ok(Value::List(rows))
    }

    /// `svc.clipboard.read(seq)` → the item's full text (`seq` 0 = the newest; "" = none).
    fn clipboard_read(&self, seq: i64) -> Result<Value, u32> {
        let req = wire::encode_read(u64::try_from(seq).map_err(|_| ERR_SVC_SHAPE)?);
        let mut resp = [0u8; wire::REPLY_MAX_BYTES];
        let len = self.clipboard_exchange(&req, &mut resp)?;
        match wire::decode_read_reply(wire::OP_READ, &resp[..len]) {
            Some((wire::STATUS_OK, _, text)) => Ok(Value::Str(String::from(text))),
            Some((wire::STATUS_DENIED, _, _)) => {
                raw_marker("apphost: dsl svc clipboard.read FAIL (denied)");
                Err(ERR_SVC_DENIED)
            }
            Some(_) => Ok(Value::Str(String::new())),
            None => Err(ERR_SVC_SHAPE),
        }
    }

    /// `svc.clipboard.write(text)` → `true` when stored.
    fn clipboard_write(&self, text: &str) -> Result<Value, u32> {
        let mut req = [0u8; wire::REQUEST_MAX_BYTES];
        let n = wire::encode_write(text, &mut req).ok_or(ERR_SVC_SHAPE)?;
        let mut resp = [0u8; 16];
        let len = self.clipboard_exchange(&req[..n], &mut resp)?;
        match wire::decode_seq_reply(wire::OP_WRITE, &resp[..len]) {
            Some((wire::STATUS_OK, _)) => Ok(Value::Bool(true)),
            Some((wire::STATUS_DENIED, _)) => Err(ERR_SVC_DENIED),
            Some(_) => Ok(Value::Bool(false)),
            None => Err(ERR_SVC_SHAPE),
        }
    }

    /// `svc.clipboard.restore(seq)` → `true` when the item is the newest again.
    fn clipboard_restore(&self, seq: i64) -> Result<Value, u32> {
        let req = wire::encode_restore(u64::try_from(seq).map_err(|_| ERR_SVC_SHAPE)?);
        let mut resp = [0u8; 16];
        let len = self.clipboard_exchange(&req, &mut resp)?;
        match wire::decode_seq_reply(wire::OP_RESTORE, &resp[..len]) {
            Some((wire::STATUS_OK, new_seq)) => {
                raw_marker(&alloc::format!(
                    "apphost: dsl svc clipboard.restore ok (seq={new_seq})"
                ));
                Ok(Value::Bool(true))
            }
            Some((wire::STATUS_DENIED, _)) => {
                raw_marker("apphost: dsl svc clipboard.restore FAIL (denied)");
                Err(ERR_SVC_DENIED)
            }
            Some(_) => Ok(Value::Bool(false)),
            None => Err(ERR_SVC_SHAPE),
        }
    }

    /// The text field's copy/cut (TASK-0067B): stores `text` (cut to the item bound on a char
    /// boundary — v1 is inline). `false` without the `CLIPBOARD` route or on a refusal.
    pub(crate) fn clipboard_copy(&self, text: &str) -> bool {
        matches!(self.clipboard_write(wire::item_text(text)), Ok(Value::Bool(true)))
    }

    /// The text field's paste (TASK-0067B): the newest item's text, if clipboardd lets this
    /// app read it (its window holds focus) and the history is not empty.
    pub(crate) fn clipboard_paste_text(&self) -> Option<String> {
        match self.clipboard_read(0) {
            Ok(Value::Str(text)) if !text.is_empty() => Some(text),
            _ => None,
        }
    }

    /// `svc.clipboard.clear()` → `true` when the history is empty.
    fn clipboard_clear(&self) -> Result<Value, u32> {
        let req = wire::encode_clear();
        let mut resp = [0u8; 16];
        let len = self.clipboard_exchange(&req, &mut resp)?;
        match wire::decode_status(wire::OP_CLEAR, &resp[..len]) {
            Some(wire::STATUS_OK) => Ok(Value::Bool(true)),
            Some(wire::STATUS_DENIED) => Err(ERR_SVC_DENIED),
            Some(_) => Ok(Value::Bool(false)),
            None => Err(ERR_SVC_SHAPE),
        }
    }
}
