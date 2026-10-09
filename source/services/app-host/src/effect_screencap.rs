// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: the `svc.screencap.*` surface of the app-host DSL `EffectHost` (RFC-0095,
//! TASK-0068) — thin verbs over screencapd on the child slot the `SCREENCAP` permission landed
//! the route in (the packer grants it to the shell and settings only): `begin()` freezes the
//! screen and answers a `CaptureFrame { w, h, front, windows: List<CaptureWindow { id, x, y, w,
//! h }> }` — the windows BACK TO FRONT, so a page that draws them in order lets the front-most
//! win a click where they overlap, and `front` the front-most one (id 0: none) —
//! `shoot(kind, x, y, w, h, pointer, stem)` saves and thaws and answers the file name,
//! `cancel()` thaws, `shot(kind, pointer, stem)` saves the screen or the focused window without
//! a UI. The binding stamps the stem with the local time (` YYYY-MM-DD HH-MM-SS`, the zone the
//! region push named) — the page passes only the localized words. `kind` is `"area"`, `"screen"` or `"window"` (for a window,
//! `x` carries its id from `begin`). NO capture logic here: the freeze is windowd's, the crop,
//! the PNG and the file are screencapd's. A refusal (the greeter, a capture already running)
//! maps to `ERR_SVC_DENIED`; names and pixels never reach a marker.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: record shape mirrored by the shell's conformance fixtures; transport proven
//!   by `SELFTEST: ui v7 screenshot ok` on the usb-visible lane

#![cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]

use super::effect_host::{
    bool_of, call_reply, int_of, raw_marker, str_of, AppEffectHost, ERR_SVC_DENIED, ERR_SVC_SHAPE,
    ERR_SVC_UNAVAILABLE, ERR_SVC_UNKNOWN,
};
use alloc::string::String;
use alloc::vec::Vec;
use nexus_dsl_runtime::Value;
use nexus_wire::screencapd as wire;

/// The record field symbols a capture's answer needs (`id` is the host's shared one) —
/// interned only when a page reads them.
pub(crate) struct CaptureSyms {
    w: Option<u32>,
    h: Option<u32>,
    x: Option<u32>,
    y: Option<u32>,
    front: Option<u32>,
    windows: Option<u32>,
    /// The zone the region push named (the file name's local time); `None` until pushed.
    zone: Option<&'static tz_lite::Zone>,
}

impl CaptureSyms {
    pub(crate) fn new(symbols: &[String]) -> Self {
        let sym = |name: &str| symbols.iter().position(|s| s == name).map(|i| i as u32);
        Self {
            w: sym("w"),
            h: sym("h"),
            x: sym("x"),
            y: sym("y"),
            front: sym("front"),
            windows: sym("windows"),
            zone: None,
        }
    }

    /// The region push named a time zone (`probe/clock.rs`).
    pub(crate) fn set_zone(&mut self, tz: &str) {
        if let Some(zone) = tz_lite::zone(tz) {
            self.zone = Some(zone);
        }
    }
}

/// `stem` and the local time, ` YYYY-MM-DD HH-MM-SS`, when the wall clock and the zone are
/// known and the result fits the wire's bound; the bare stem otherwise (never a made-up time).
fn stamped(stem: &str, zone: Option<&'static tz_lite::Zone>) -> String {
    let (Some(zone), Some(epoch_ns)) = (zone, crate::time_client::walltime_now()) else {
        return String::from(stem);
    };
    let c = tz_lite::to_civil(epoch_ns, zone);
    let sec = (epoch_ns / 1_000_000_000) % 60;
    let out = alloc::format!(
        "{stem} {:04}-{:02}-{:02} {:02}-{:02}-{sec:02}",
        c.year,
        c.month,
        c.day,
        c.hour,
        c.minute
    );
    if out.len() <= wire::STEM_MAX_BYTES {
        out
    } else {
        String::from(stem)
    }
}

/// A record from `(symbol, value)` pairs whose symbol the page interned, FIELD-SORTED by
/// symbol id (the `Value::Record` contract).
fn record(fields: &[(Option<u32>, Value)]) -> Value {
    let mut out: Vec<(u32, Value)> =
        fields.iter().filter_map(|(sym, v)| sym.map(|s| (s, v.clone()))).collect();
    out.sort_by_key(|(sym, _)| *sym);
    Value::Record(out)
}

fn kind_code(kind: &str) -> Option<u8> {
    match kind {
        "area" => Some(wire::KIND_AREA),
        "screen" => Some(wire::KIND_SCREEN),
        "window" => Some(wire::KIND_WINDOW),
        _ => None,
    }
}

impl AppEffectHost {
    /// One `svc.screencap.<method>` call (the dispatch keeps one line for the namespace).
    pub(crate) fn screencap_call(&self, method: &str, args: &[Value]) -> Result<Value, u32> {
        let str_at = |i: usize| args.get(i).and_then(str_of).ok_or(ERR_SVC_SHAPE);
        let int_at = |i: usize| args.get(i).and_then(int_of).ok_or(ERR_SVC_SHAPE);
        let bool_at = |i: usize| args.get(i).and_then(bool_of).ok_or(ERR_SVC_SHAPE);
        match method {
            "begin" => self.screencap_begin(),
            "cancel" => self.screencap_cancel(),
            "shoot" => {
                let kind = kind_code(str_at(0)?).ok_or(ERR_SVC_SHAPE)?;
                let mode = wire::mode(kind, bool_at(5)?);
                let clamp16 = |v: i64| u16::try_from(v.max(0)).unwrap_or(u16::MAX);
                let x = u32::try_from(int_at(1)?.max(0)).unwrap_or(u32::MAX);
                let (y, w, h) = (clamp16(int_at(2)?), clamp16(int_at(3)?), clamp16(int_at(4)?));
                let stem = stamped(str_at(6)?, self.screencap.zone);
                let mut req = [0u8; wire::REQUEST_MAX_BYTES];
                let n =
                    wire::encode_shoot(mode, x, y, w, h, &stem, &mut req).ok_or(ERR_SVC_SHAPE)?;
                self.screencap_saved(wire::OP_SHOOT, &req[..n])
            }
            "shot" => {
                let kind = kind_code(str_at(0)?).ok_or(ERR_SVC_SHAPE)?;
                let stem = stamped(str_at(2)?, self.screencap.zone);
                let mut req = [0u8; wire::REQUEST_MAX_BYTES];
                let n = wire::encode_shot(wire::mode(kind, bool_at(1)?), &stem, &mut req)
                    .ok_or(ERR_SVC_SHAPE)?;
                self.screencap_saved(wire::OP_SHOT, &req[..n])
            }
            _ => Err(ERR_SVC_UNKNOWN),
        }
    }

    fn screencap_exchange(&self, req: &[u8], resp: &mut [u8]) -> Result<usize, u32> {
        let send_slot = Self::svc_send_slot("screencap").ok_or(ERR_SVC_UNKNOWN)?;
        call_reply(send_slot, req, resp).ok_or_else(|| {
            raw_marker("apphost: dsl svc screencap FAIL (screencapd unreachable)");
            ERR_SVC_UNAVAILABLE
        })
    }

    /// `svc.screencap.begin()` → `CaptureFrame { w, h, windows }`.
    fn screencap_begin(&self) -> Result<Value, u32> {
        let mut resp = [0u8; wire::REPLY_MAX_BYTES];
        let len = self.screencap_exchange(&wire::encode_begin(), &mut resp)?;
        let Some((status, w, h, _count, packed)) =
            wire::decode_begin_reply(wire::OP_BEGIN, &resp[..len])
        else {
            return Err(ERR_SVC_SHAPE);
        };
        match status {
            wire::STATUS_OK => {}
            wire::STATUS_DENIED | wire::STATUS_BUSY => return Err(ERR_SVC_DENIED),
            _ => return Err(ERR_SVC_UNAVAILABLE),
        }
        let s = &self.screencap;
        let window = |win: &wire::Window| {
            record(&[
                (self.id_sym, Value::Int(i64::from(win.id))),
                (s.x, Value::Int(i64::from(win.x))),
                (s.y, Value::Int(i64::from(win.y))),
                (s.w, Value::Int(i64::from(win.w))),
                (s.h, Value::Int(i64::from(win.h))),
            ])
        };
        // The wire is front to back; the page draws back to front (the front-most last).
        let front = wire::unpack_windows(packed).next().unwrap_or_default();
        let mut windows: Vec<Value> = wire::unpack_windows(packed).map(|w| window(&w)).collect();
        windows.reverse();
        Ok(record(&[
            (s.w, Value::Int(i64::from(w))),
            (s.h, Value::Int(i64::from(h))),
            (s.front, window(&front)),
            (s.windows, Value::List(windows)),
        ]))
    }

    /// `svc.screencap.cancel()` → `true` when a freeze ended.
    fn screencap_cancel(&self) -> Result<Value, u32> {
        let mut resp = [0u8; 16];
        let len = self.screencap_exchange(&wire::encode_cancel(), &mut resp)?;
        match wire::decode_status(wire::OP_CANCEL, &resp[..len]) {
            Some(status) => Ok(Value::Bool(status == wire::STATUS_OK)),
            None => Err(ERR_SVC_SHAPE),
        }
    }

    /// `shoot` / `shot` → the saved file's name.
    fn screencap_saved(&self, op: u8, req: &[u8]) -> Result<Value, u32> {
        let mut resp = [0u8; wire::REPLY_MAX_BYTES];
        let len = self.screencap_exchange(req, &mut resp)?;
        match wire::decode_saved_reply(op, &resp[..len]) {
            Some((wire::STATUS_OK, name)) => Ok(Value::Str(String::from(name))),
            Some((wire::STATUS_DENIED | wire::STATUS_BUSY, _)) => Err(ERR_SVC_DENIED),
            Some((wire::STATUS_MALFORMED, _)) => Err(ERR_SVC_SHAPE),
            Some(_) => Err(ERR_SVC_UNAVAILABLE),
            None => Err(ERR_SVC_SHAPE),
        }
    }
}
