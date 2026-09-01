// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: the `svc.updates.*` surface of the app-host DSL `EffectHost`
//! (RFC-0089 §8/§9, TASK-0140) — thin verbs over `updated`: status (ONE
//! UpdateStat record in a list; empty = engine unreachable), the offline
//! feed (names + candidate count), and the mutating verbs stage/switch/
//! rollback. NO update logic here: verify/stage/apply live in `updated`,
//! state in bootctld. A policy deny (`updates.manage`, deny-by-default)
//! maps to `ERR_SVC_DENIED` so the page renders the honest DENIED state —
//! never a fake success, never a generic failure.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: record shape mirrored by tests/dsl_apps_conformance
//! settings suite; transport proven via QEMU markers
//! (`apphost: dsl svc updates.*`).

#![cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]

use super::effect_host::{
    call_reply_within, raw_marker, AppEffectHost, ERR_SVC_DENIED, ERR_SVC_SHAPE,
    ERR_SVC_UNAVAILABLE, ERR_SVC_UNKNOWN, REPLY_BUF,
};
use alloc::string::String;
use alloc::vec::Vec;
use nexus_abi::updated as uw;
use nexus_dsl_runtime::Value;

/// Decoded updated reply: `(status, payload)`.
fn decode_reply(op: u8, resp: &[u8]) -> Option<(u8, &[u8])> {
    if resp.len() < 7 || resp[0] != uw::MAGIC0 || resp[1] != uw::MAGIC1 || resp[2] != uw::VERSION {
        return None;
    }
    if resp[3] != (op | 0x80) {
        return None;
    }
    let len = u16::from_le_bytes([resp[5], resp[6]]) as usize;
    let avail = len.min(resp.len().saturating_sub(7));
    Some((resp[4], &resp[7..7 + avail]))
}

impl AppEffectHost {
    fn updates_call(&self, req: &[u8], op: u8, verb: &str) -> Result<(u8, Vec<u8>), u32> {
        let send_slot = Self::svc_send_slot("updates").ok_or(ERR_SVC_UNKNOWN)?;
        let mut resp = [0u8; REPLY_BUF];
        let Some(len) = call_reply_within(send_slot, req, &mut resp, self.budget_ns) else {
            raw_marker("apphost: dsl svc updates FAIL (updated unreachable)");
            let _ = verb;
            return Err(ERR_SVC_UNAVAILABLE);
        };
        let Some((status, payload)) = decode_reply(op, &resp[..len]) else {
            raw_marker("apphost: dsl svc updates FAIL (shape)");
            return Err(ERR_SVC_SHAPE);
        };
        Ok((status, payload.to_vec()))
    }

    /// One mutating verb: OK → `Bool(true)`, machine reject → `Bool(false)`
    /// (the page shows the failure state), policy deny → `ERR_SVC_DENIED`.
    fn updates_mutate(&self, req: &[u8], op: u8, ok_marker: &str) -> Result<Value, u32> {
        let (status, _payload) = self.updates_call(req, op, ok_marker)?;
        match status {
            uw::STATUS_OK => {
                raw_marker(ok_marker);
                Ok(Value::Bool(true))
            }
            uw::STATUS_DENIED => {
                raw_marker("apphost: dsl svc updates FAIL (denied)");
                Err(ERR_SVC_DENIED)
            }
            _ => {
                raw_marker("apphost: dsl svc updates FAIL (status)");
                Ok(Value::Bool(false))
            }
        }
    }

    /// `svc.updates.status()` → `List<UpdateStat>` with ONE record (empty
    /// list = engine unreachable). All fields are display-ready `Str`s
    /// decoded from updated's OP_GET_STATUS: the pinned 17-byte bootctld
    /// prefix + the 8-byte staged-build tail (TASK-0140).
    pub(crate) fn updates_status(&self) -> Result<Value, u32> {
        let mut req = [0u8; 8];
        let n = uw::encode_get_status_req(&mut req).ok_or(ERR_SVC_SHAPE)?;
        let (status, p) = self.updates_call(&req[..n], uw::OP_GET_STATUS, "status")?;
        if status != uw::STATUS_OK || p.len() < 4 {
            raw_marker("apphost: dsl svc updates.status FAIL (status)");
            return Ok(Value::List(Vec::new()));
        }
        let slot_name = |b: u8| match b {
            1 => "a",
            2 => "b",
            _ => "-",
        };
        let yes_no = |b: u8| if b != 0 { "yes" } else { "no" };
        let floor =
            if p.len() >= 17 { u32::from_le_bytes([p[13], p[14], p[15], p[16]]) } else { 0 };
        let staged: String = if p.len() >= 25 {
            p[17..25].iter().take_while(|&&b| b != 0).map(|&b| b as char).collect()
        } else {
            String::new()
        };
        let mut tries = String::new();
        push_u32(&mut tries, u32::from(p[2]));
        let mut floor_text = String::new();
        push_u32(&mut floor_text, floor);
        let mut fields: Vec<(u32, Value)> = Vec::new();
        let mut put = |sym: Option<u32>, v: Value| {
            if let Some(sym) = sym {
                fields.push((sym, v));
            }
        };
        put(self.slot_sym, Value::Str(String::from(slot_name(p[0]))));
        put(self.pending_sym, Value::Str(String::from(slot_name(p[1]))));
        put(self.tries_sym, Value::Str(tries));
        put(self.committed_sym, Value::Str(String::from(yes_no(p[3]))));
        put(self.synced_sym, Value::Str(String::from(yes_no(p.get(4).copied().unwrap_or(0)))));
        put(self.floor_sym, Value::Str(floor_text));
        put(self.staged_sym, Value::Str(staged));
        fields.sort_by_key(|(s, _)| *s);
        raw_marker("apphost: dsl svc updates.status ok");
        Ok(Value::List(alloc::vec![Value::Record(fields)]))
    }

    /// `svc.updates.feed()` → `List<Str>` of `.nxs` candidates (§9 feed).
    pub(crate) fn updates_feed(&self) -> Result<Value, u32> {
        let mut req = [0u8; 8];
        let n = uw::encode_feed_list_req(&mut req).ok_or(ERR_SVC_SHAPE)?;
        let (status, p) = self.updates_call(&req[..n], uw::OP_FEED_LIST, "feed")?;
        if status != uw::STATUS_OK || p.is_empty() {
            raw_marker("apphost: dsl svc updates.feed FAIL (status)");
            return Ok(Value::List(Vec::new()));
        }
        let mut names = Vec::new();
        let mut at = 1usize;
        while at < p.len() && names.len() < 8 {
            let len = p[at] as usize;
            at += 1;
            if len == 0 || at + len > p.len() {
                break;
            }
            if let Ok(name) = core::str::from_utf8(&p[at..at + len]) {
                names.push(Value::Str(String::from(name)));
            }
            at += len;
        }
        raw_marker("apphost: dsl svc updates.feed ok");
        Ok(Value::List(names))
    }

    /// `svc.updates.check()` → `Int` candidate count (§9 OP_CHECK).
    pub(crate) fn updates_check(&self) -> Result<Value, u32> {
        let mut req = [0u8; 8];
        let n = uw::encode_check_req(&mut req).ok_or(ERR_SVC_SHAPE)?;
        let (status, p) = self.updates_call(&req[..n], uw::OP_CHECK, "check")?;
        if status != uw::STATUS_OK || p.is_empty() {
            raw_marker("apphost: dsl svc updates.check FAIL (status)");
            return Ok(Value::Int(0));
        }
        raw_marker("apphost: dsl svc updates.check ok");
        Ok(Value::Int(i64::from(p[0])))
    }

    /// `svc.updates.stage(name)` → stage `/updates/<name>` (gated).
    pub(crate) fn updates_stage(&self, name: &str) -> Result<Value, u32> {
        if name.is_empty()
            || name.len() > 64
            || !name.ends_with(".nxs")
            || !name.bytes().all(|b| b.is_ascii_graphic() && b != b'/')
        {
            return Err(ERR_SVC_SHAPE);
        }
        let mut path = String::from("/updates/");
        path.push_str(name);
        let mut req = [0u8; 256];
        let n = uw::encode_stage_source_req(path.as_bytes(), &mut req).ok_or(ERR_SVC_SHAPE)?;
        self.updates_mutate(&req[..n], uw::OP_STAGE_SOURCE, "apphost: dsl svc updates.stage ok")
    }

    /// `svc.updates.switch()` → arm the staged slot (tries=2, gated).
    pub(crate) fn updates_switch(&self) -> Result<Value, u32> {
        let mut req = [0u8; 8];
        let n = uw::encode_switch_req(2, &mut req).ok_or(ERR_SVC_SHAPE)?;
        self.updates_mutate(&req[..n], uw::OP_SWITCH, "apphost: dsl svc updates.switch ok")
    }

    /// `svc.updates.rollback()` → clear the pending trial (gated).
    pub(crate) fn updates_rollback(&self) -> Result<Value, u32> {
        let mut req = [0u8; 8];
        let n = uw::encode_rollback_req(&mut req).ok_or(ERR_SVC_SHAPE)?;
        self.updates_mutate(&req[..n], uw::OP_ROLLBACK, "apphost: dsl svc updates.rollback ok")
    }
}

/// Decimal render without `format!` (bump-heap discipline).
fn push_u32(out: &mut String, mut v: u32) {
    if v == 0 {
        out.push('0');
        return;
    }
    let mut digits = [0u8; 10];
    let mut n = 0;
    while v > 0 {
        digits[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    for i in (0..n).rev() {
        out.push(digits[i] as char);
    }
}
