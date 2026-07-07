// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `@persist` snapshots (TASK-0080D lifecycle, docs/dev/dsl/state.md
//! "Durable small"): store fields marked `@persist` in the IR are encoded to
//! a compact, NAME-keyed snapshot (`NXPS` v1) and restored on mount. Names —
//! not indexes — key the entries, so a snapshot survives app updates that
//! add/remove/reorder fields; unknown or type-changed entries are skipped
//! per-entry (fail-closed restore, never poisoned state). The HOST decides
//! where bytes live (app-host: statefsd; tests: memory) — this module owns
//! only the codec and the store walk.
//! OWNERS: @ui @runtime
//! STATUS: Experimental (TASK-0080D)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: round-trip + skip/evolution unit tests (`persist::tests`)

use crate::store::Value;
use crate::Runtime;
use alloc::{string::String, vec::Vec};

/// Snapshot magic + version (`"NXPS", 1`).
const MAGIC: [u8; 4] = *b"NXPS";
const VERSION: u8 = 1;
/// Bounded nesting for encoded values (records/lists).
const MAX_DEPTH: usize = 16;

impl Runtime<'_> {
    /// True when the mounted program declares at least one `@persist` field.
    #[must_use]
    pub fn has_persist_fields(&self) -> bool {
        let Ok(root) = self.reader().root() else { return false };
        let Ok(stores) = root.get_stores() else { return false };
        stores
            .iter()
            .filter_map(|s| s.get_fields().ok())
            .any(|fields| fields.iter().any(|f| f.get_persist()))
    }

    /// Encodes every `@persist` field into a snapshot. `None` when the
    /// program has no persisted fields (hosts skip the write entirely).
    #[must_use]
    pub fn persist_snapshot(&self) -> Option<Vec<u8>> {
        let root = self.reader().root().ok()?;
        let ir_stores = root.get_stores().ok()?;
        let mut entries: u16 = 0;
        let mut body = Vec::new();
        for (si, store) in ir_stores.iter().enumerate() {
            let fields = store.get_fields().ok()?;
            let store_name = self.symbols().get(store.get_name() as usize)?;
            for (fi, field) in fields.iter().enumerate() {
                if !field.get_persist() {
                    continue;
                }
                let field_name = self.symbols().get(field.get_name() as usize)?;
                let value = self.stores().get(si)?.fields.get(fi)?;
                push_str8(&mut body, store_name);
                push_str8(&mut body, field_name);
                encode_value(&mut body, value, self.symbols(), 0)?;
                entries = entries.checked_add(1)?;
            }
        }
        if entries == 0 {
            return None;
        }
        let mut out = Vec::with_capacity(7 + body.len());
        out.extend_from_slice(&MAGIC);
        out.push(VERSION);
        out.extend_from_slice(&entries.to_le_bytes());
        out.extend_from_slice(&body);
        Some(out)
    }

    /// Restores a snapshot into the mounted stores. Per-entry tolerant:
    /// entries whose store/field no longer exists, is no longer `@persist`,
    /// or whose value type changed are SKIPPED. Returns the number of
    /// fields restored (0 for a malformed snapshot — never an error).
    pub fn persist_restore(&mut self, bytes: &[u8]) -> usize {
        let Some(rest) = bytes.strip_prefix(&MAGIC) else { return 0 };
        let Some((&version, rest)) = rest.split_first() else { return 0 };
        if version != VERSION || rest.len() < 2 {
            return 0;
        }
        let entries = u16::from_le_bytes([rest[0], rest[1]]);
        let mut cursor = &rest[2..];
        let mut restored = 0usize;
        for _ in 0..entries {
            let Some((store_name, r)) = take_str8(cursor) else { return restored };
            let Some((field_name, r)) = take_str8(r) else { return restored };
            let Some((value, r)) = decode_value(r, self.symbols(), 0) else {
                return restored;
            };
            cursor = r;
            if self.restore_field(&store_name, &field_name, value) {
                restored += 1;
            }
        }
        restored
    }

    /// Writes one restored value if the target still exists, is still
    /// `@persist`, and the value variant matches the current default.
    fn restore_field(&mut self, store_name: &str, field_name: &str, value: Value) -> bool {
        let Ok(root) = self.reader().root() else { return false };
        let Ok(ir_stores) = root.get_stores() else { return false };
        for (si, store) in ir_stores.iter().enumerate() {
            if self.symbols().get(store.get_name() as usize).map(String::as_str)
                != Some(store_name)
            {
                continue;
            }
            let Ok(fields) = store.get_fields() else { return false };
            for (fi, field) in fields.iter().enumerate() {
                if self.symbols().get(field.get_name() as usize).map(String::as_str)
                    != Some(field_name)
                {
                    continue;
                }
                if !field.get_persist() {
                    return false;
                }
                let Some(state) = self.stores_mut().get_mut(si) else { return false };
                let Some(slot) = state.fields.get_mut(fi) else { return false };
                if core::mem::discriminant(slot) != core::mem::discriminant(&value) {
                    return false; // type changed across versions — keep default
                }
                *slot = value;
                return true;
            }
            return false;
        }
        false
    }
}

fn push_str8(out: &mut Vec<u8>, s: &str) {
    let n = s.len().min(255);
    out.push(n as u8);
    out.extend_from_slice(&s.as_bytes()[..n]);
}

fn take_str8(bytes: &[u8]) -> Option<(String, &[u8])> {
    let (&n, rest) = bytes.split_first()?;
    let n = n as usize;
    if rest.len() < n {
        return None;
    }
    let s = core::str::from_utf8(&rest[..n]).ok()?;
    Some((String::from(s), &rest[n..]))
}

/// Value tags (persisted format — append-only).
const T_UNIT: u8 = 0;
const T_BOOL: u8 = 1;
const T_INT: u8 = 2;
const T_FX: u8 = 3;
const T_STR: u8 = 4;
const T_LIST: u8 = 5;
const T_ENUM: u8 = 6;
const T_RECORD: u8 = 7;

fn encode_value(out: &mut Vec<u8>, value: &Value, symbols: &[String], depth: usize) -> Option<()> {
    if depth > MAX_DEPTH {
        return None;
    }
    match value {
        Value::Unit => out.push(T_UNIT),
        Value::Bool(b) => {
            out.push(T_BOOL);
            out.push(u8::from(*b));
        }
        Value::Int(i) => {
            out.push(T_INT);
            out.extend_from_slice(&i.to_le_bytes());
        }
        Value::Fx(f) => {
            out.push(T_FX);
            out.extend_from_slice(&f.to_le_bytes());
        }
        Value::Str(s) => {
            out.push(T_STR);
            out.extend_from_slice(&(s.len() as u32).to_le_bytes());
            out.extend_from_slice(s.as_bytes());
        }
        Value::List(items) => {
            out.push(T_LIST);
            out.extend_from_slice(&(items.len() as u32).to_le_bytes());
            for item in items {
                encode_value(out, item, symbols, depth + 1)?;
            }
        }
        Value::Enum { event, case, payload } => {
            out.push(T_ENUM);
            out.extend_from_slice(&event.to_le_bytes());
            out.extend_from_slice(&case.to_le_bytes());
            out.push(payload.len().min(255) as u8);
            for item in payload {
                encode_value(out, item, symbols, depth + 1)?;
            }
        }
        Value::Record(fields) => {
            out.push(T_RECORD);
            out.extend_from_slice(&(fields.len() as u16).to_le_bytes());
            for (sym, item) in fields {
                // Record fields persist by NAME (symbol ids are compile-run
                // specific; names survive recompiles).
                push_str8(out, symbols.get(*sym as usize)?);
                encode_value(out, item, symbols, depth + 1)?;
            }
        }
    }
    Some(())
}

fn decode_value<'b>(
    bytes: &'b [u8],
    symbols: &[String],
    depth: usize,
) -> Option<(Value, &'b [u8])> {
    if depth > MAX_DEPTH {
        return None;
    }
    let (&tag, rest) = bytes.split_first()?;
    match tag {
        T_UNIT => Some((Value::Unit, rest)),
        T_BOOL => {
            let (&b, rest) = rest.split_first()?;
            Some((Value::Bool(b != 0), rest))
        }
        T_INT => {
            let (v, rest) = take_i64(rest)?;
            Some((Value::Int(v), rest))
        }
        T_FX => {
            let (v, rest) = take_i64(rest)?;
            Some((Value::Fx(v), rest))
        }
        T_STR => {
            let (n, rest) = take_u32(rest)?;
            let n = n as usize;
            if rest.len() < n {
                return None;
            }
            let s = core::str::from_utf8(&rest[..n]).ok()?;
            Some((Value::Str(String::from(s)), &rest[n..]))
        }
        T_LIST => {
            let (n, mut rest) = take_u32(rest)?;
            let mut items = Vec::new();
            for _ in 0..n {
                let (item, r) = decode_value(rest, symbols, depth + 1)?;
                items.push(item);
                rest = r;
            }
            Some((Value::List(items), rest))
        }
        T_ENUM => {
            let (event, rest) = take_u32(rest)?;
            let (case, rest) = take_u32(rest)?;
            let (&n, mut rest) = rest.split_first()?;
            let mut payload = Vec::new();
            for _ in 0..n {
                let (item, r) = decode_value(rest, symbols, depth + 1)?;
                payload.push(item);
                rest = r;
            }
            Some((Value::Enum { event, case, payload }, rest))
        }
        T_RECORD => {
            if rest.len() < 2 {
                return None;
            }
            let n = u16::from_le_bytes([rest[0], rest[1]]);
            let mut rest = &rest[2..];
            let mut fields = Vec::new();
            for _ in 0..n {
                let (name, r) = take_str8(rest)?;
                let sym = symbols.iter().position(|s| *s == name)? as u32;
                let (item, r) = decode_value(r, symbols, depth + 1)?;
                fields.push((sym, item));
                rest = r;
            }
            Some((Value::Record(fields), rest))
        }
        _ => None,
    }
}

fn take_i64(bytes: &[u8]) -> Option<(i64, &[u8])> {
    if bytes.len() < 8 {
        return None;
    }
    let mut v = [0u8; 8];
    v.copy_from_slice(&bytes[..8]);
    Some((i64::from_le_bytes(v), &bytes[8..]))
}

fn take_u32(bytes: &[u8]) -> Option<(u32, &[u8])> {
    if bytes.len() < 4 {
        return None;
    }
    let mut v = [0u8; 4];
    v.copy_from_slice(&bytes[..4]);
    Some((u32::from_le_bytes(v), &bytes[4..]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    use alloc::vec;

    fn round_trip(value: &Value, symbols: &[String]) -> Value {
        let mut buf = Vec::new();
        encode_value(&mut buf, value, symbols, 0).expect("encode");
        let (decoded, rest) = decode_value(&buf, symbols, 0).expect("decode");
        assert!(rest.is_empty(), "trailing bytes");
        decoded
    }

    #[test]
    fn scalar_values_round_trip() {
        let symbols: Vec<String> = Vec::new();
        for v in [
            Value::Unit,
            Value::Bool(true),
            Value::Int(-42),
            Value::Fx(1 << 32),
            Value::Str("héllo".to_string()),
            Value::List(vec![Value::Int(1), Value::Str("x".to_string())]),
            Value::Enum { event: 3, case: 1, payload: vec![Value::Bool(false)] },
        ] {
            assert_eq!(round_trip(&v, &symbols), v);
        }
    }

    #[test]
    fn record_fields_persist_by_name() {
        let symbols = vec!["a".to_string(), "b".to_string()];
        let v = Value::Record(vec![(0, Value::Int(7)), (1, Value::Str("y".to_string()))]);
        assert_eq!(round_trip(&v, &symbols), v);
        // A record field whose name vanished from the symbol table fails
        // decode (entry-level skip upstream), not a garbage value.
        let mut buf = Vec::new();
        encode_value(&mut buf, &v, &symbols, 0).expect("encode");
        assert!(decode_value(&buf, &["a".to_string()], 0).is_none());
    }

    #[test]
    fn truncated_and_unknown_tags_rejected() {
        let symbols: Vec<String> = Vec::new();
        let mut buf = Vec::new();
        encode_value(&mut buf, &Value::Int(9), &symbols, 0).expect("encode");
        assert!(decode_value(&buf[..buf.len() - 1], &symbols, 0).is_none());
        assert!(decode_value(&[0xFF], &symbols, 0).is_none());
    }
}
