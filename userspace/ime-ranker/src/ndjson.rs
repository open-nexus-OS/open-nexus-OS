// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: TASK-0203 / RFC-0075 Phase 4 — NDJSON interchange for the
//! personalization store, written ONCE as free functions over `PersonalStore`
//! (so the statefsd backend in TASK-0204 implements only storage primitives).
//! A versioned header line then one JSON object per dict entry / bigram.
//! Candidate bytes are lowercase-hex inside the JSON string, so a line is pure
//! ASCII — no escaping edge cases for CJK, and the length bound is exact.
//! Import is fail-closed: bound the line BEFORE parsing, skip malformed lines
//! with a capped error count, and reject the whole file on a bad header.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable (goldens in tests/ are the contract — export bytes)
//! TEST_COVERAGE: tests/ndjson.rs (round-trip, reject matrix, quota)

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write as _;

use crate::store::{DictStat, PersonalStore, CAND_MAX};

/// NDJSON schema version (the header's `v`).
pub const NDJSON_VERSION: u8 = 1;

/// Maximum accepted import line length (bytes). Anything longer is skipped
/// before parsing — the bounded-input invariant.
pub const NDJSON_LINE_MAX: usize = 256;

/// Cap on skipped (malformed/oversized) lines before import gives up — bounds
/// the work and the error count for a hostile profile.
const MAX_IMPORT_ERRORS: usize = 256;

/// The exact header line an export writes and an import requires.
const HEADER: &str = "{\"v\":1,\"kind\":\"ime-personal\"}";

/// Outcome of a successful [`import_ndjson`]: how many records loaded and how
/// many lines were skipped fail-closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ImportReport {
    pub dict_loaded: usize,
    pub bigram_loaded: usize,
    pub skipped: usize,
}

/// Whole-file import rejections (a per-line problem is a `skipped`, not this).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportError {
    /// No header line at all.
    Empty,
    /// Header missing or not version 1 — the file is rejected untouched-onward.
    BadHeader,
}

/// Serializes the whole store to NDJSON. Deterministic: the store iterates in
/// sorted order, so identical state exports byte-identical output.
#[must_use]
pub fn export_ndjson<S: PersonalStore>(store: &S) -> String {
    let mut out = String::new();
    out.push_str(HEADER);
    out.push('\n');
    for (key, stat) in store.dict_entries() {
        out.push_str("{\"t\":\"d\",\"c\":\"");
        push_hex(&mut out, key.as_bytes());
        let _ = writeln!(out, "\",\"f\":{},\"b\":{}}}", stat.freq, stat.last_seen_bucket);
    }
    for (prev, cand, n) in store.bigram_entries() {
        out.push_str("{\"t\":\"g\",\"p\":\"");
        push_hex(&mut out, prev.as_bytes());
        out.push_str("\",\"c\":\"");
        push_hex(&mut out, cand.as_bytes());
        let _ = writeln!(out, "\",\"n\":{}}}", n);
    }
    out
}

/// Loads NDJSON into `store`, fail-closed. The header must match version 1
/// (else `Err`); each subsequent line is bounded before parsing and skipped
/// (counted) if oversized or malformed. Quotas are enforced by the store's
/// `upsert_*` (eviction), so a hostile profile cannot grow it past its cap.
pub fn import_ndjson<S: PersonalStore>(
    store: &mut S,
    data: &str,
) -> Result<ImportReport, ImportError> {
    if data.is_empty() {
        return Err(ImportError::Empty);
    }
    let mut lines = data.split('\n');
    // `split` always yields at least one item for non-empty input.
    let header = lines.next().unwrap_or("");
    if header != HEADER {
        return Err(ImportError::BadHeader);
    }
    let mut report = ImportReport::default();
    for line in lines {
        if line.is_empty() {
            continue; // trailing newline / blank separators
        }
        if report.skipped >= MAX_IMPORT_ERRORS {
            break; // too many bad lines — stop (bounded)
        }
        if line.len() > NDJSON_LINE_MAX {
            report.skipped += 1; // bound BEFORE parsing
            continue;
        }
        match parse_record(line) {
            Some(Record::Dict { cand, freq, bucket }) => {
                store.upsert_dict(&cand, DictStat { freq, last_seen_bucket: bucket });
                report.dict_loaded += 1;
            }
            Some(Record::Bigram { prev, cand, count }) => {
                store.upsert_bigram(&prev, &cand, count);
                report.bigram_loaded += 1;
            }
            None => report.skipped += 1,
        }
    }
    Ok(report)
}

enum Record {
    Dict { cand: Vec<u8>, freq: u16, bucket: u16 },
    Bigram { prev: Vec<u8>, cand: Vec<u8>, count: u16 },
}

/// Strict, fail-closed line parse: `{key:value,...}` where values are quoted
/// hex or bare `u16`. Any deviation (unknown field, bad hex, non-numeric,
/// missing field, extra field) returns `None` → the line is skipped.
fn parse_record(line: &str) -> Option<Record> {
    let inner = line.strip_prefix('{')?.strip_suffix('}')?;
    let (mut t, mut c, mut p, mut f, mut b, mut n) = (None, None, None, None, None, None);
    for field in inner.split(',') {
        let (key, val) = field.split_once(':')?;
        match unquote(key)? {
            "t" => t = Some(unquote(val)?),
            "c" => c = Some(parse_hex(unquote(val)?)?),
            "p" => p = Some(parse_hex(unquote(val)?)?),
            "f" => f = Some(val.parse::<u16>().ok()?),
            "b" => b = Some(val.parse::<u16>().ok()?),
            "n" => n = Some(val.parse::<u16>().ok()?),
            _ => return None,
        }
    }
    match t? {
        "d" => Some(Record::Dict { cand: c?, freq: f?, bucket: b? }),
        "g" => Some(Record::Bigram { prev: p?, cand: c?, count: n? }),
        _ => None,
    }
}

fn unquote(s: &str) -> Option<&str> {
    s.strip_prefix('"')?.strip_suffix('"')
}

fn push_hex(out: &mut String, bytes: &[u8]) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for &byte in bytes {
        out.push(HEX[usize::from(byte >> 4)] as char);
        out.push(HEX[usize::from(byte & 0x0f)] as char);
    }
}

fn parse_hex(s: &str) -> Option<Vec<u8>> {
    let bytes = s.as_bytes();
    // Bounded: a candidate is at most CAND_MAX bytes → CAND_MAX*2 hex digits.
    if bytes.is_empty() || bytes.len() % 2 != 0 || bytes.len() > CAND_MAX * 2 {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 2);
    let mut i = 0;
    while i < bytes.len() {
        out.push((hex_val(bytes[i])? << 4) | hex_val(bytes[i + 1])?);
        i += 2;
    }
    Some(out)
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    }
}
