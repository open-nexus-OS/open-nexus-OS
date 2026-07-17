// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The query engine: schema-checked writes with index maintenance +
//! index-driven QuerySpec execution with keyset paging (no post-sort, no
//! offset scans — pages resume from an order-preserving key).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 12 tests (unit + tests/engine_paging.rs integration walk)
//!
//! Key layout (all prefixes self-describing, byte-ordered):
//! - primary row:  `q <table u16 BE> r <pk_key>` → row bytes
//! - index entry:  `q <table u16 BE> i <col u16 BE> <col_key> <pk_key>` → pk_key
//!
//! The index VALUE is the pk_key bytes, so resuming/joining never parses the
//! composite index key apart. The v1 execution rule: the order column must be
//! indexed and the (single, optional) range predicate must be on the order
//! column — the scan itself streams results in final order.

use crate::encoding::{decode_row, encode_key, encode_row, QType, QVal};
use crate::kv::Kv;
use crate::spec::{Page, PageToken, QuerySpec};
use alloc::vec::Vec;

/// Stable, wire-mappable failure vocabulary (no stringly errors).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryError {
    UnknownTable,
    UnknownColumn,
    /// Value type doesn't match the column's schema type.
    TypeMismatch,
    /// Spec shape outside the v1 contract (range not on order col,
    /// order col not indexed, zero limit).
    Unsupported,
    /// Token malformed or minted by a DIFFERENT query (hash mismatch).
    BadToken,
    /// Stored bytes failed to decode (corruption — never silently skipped).
    Corrupt,
}

/// Table schema: column types, which column is the primary key, and which
/// columns carry a secondary index. v1: single-column pk + indices.
#[derive(Debug, Clone)]
pub struct TableDef {
    pub id: u16,
    pub columns: Vec<QType>,
    pub pk_col: usize,
    pub indexed: Vec<usize>,
}

impl TableDef {
    fn check_row(&self, row: &[QVal]) -> Result<(), QueryError> {
        if row.len() != self.columns.len() {
            return Err(QueryError::TypeMismatch);
        }
        for (value, ty) in row.iter().zip(&self.columns) {
            if value.kind() != *ty {
                return Err(QueryError::TypeMismatch);
            }
        }
        Ok(())
    }

    fn has_index(&self, col: usize) -> bool {
        col == self.pk_col || self.indexed.contains(&col)
    }
}

/// The engine: a schema catalog over any ordered [`Kv`].
pub struct Engine {
    tables: Vec<TableDef>,
}

const TAG_ROW: u8 = b'r';
const TAG_INDEX: u8 = b'i';
/// Hard cap on keys touched per query execution (hostile-limit bound).
const MAX_SCAN: usize = 4096;

fn table_prefix(table: u16, tag: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(5);
    out.push(b'q');
    out.extend_from_slice(&table.to_be_bytes());
    out.push(tag);
    out
}

fn row_key(table: u16, pk_key: &[u8]) -> Vec<u8> {
    let mut out = table_prefix(table, TAG_ROW);
    out.extend_from_slice(pk_key);
    out
}

fn index_prefix(table: u16, col: usize) -> Vec<u8> {
    let mut out = table_prefix(table, TAG_INDEX);
    out.extend_from_slice(&(col as u16).to_be_bytes());
    out
}

/// Smallest key strictly greater than `key` (append 0x00) — turns an
/// inclusive resume point into an exclusive scan start.
fn key_after(key: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(key.len() + 1);
    out.extend_from_slice(key);
    out.push(0x00);
    out
}

/// Upper bound of a prefix: prefix with the last byte bumped (carrying).
/// Prefixes here always contain a non-0xFF byte (the tag), so this is total.
fn prefix_end(prefix: &[u8]) -> Vec<u8> {
    let mut out = prefix.to_vec();
    while let Some(last) = out.pop() {
        if last != 0xFF {
            out.push(last + 1);
            return out;
        }
    }
    out
}

impl Engine {
    #[must_use]
    pub fn new(tables: Vec<TableDef>) -> Self {
        Self { tables }
    }

    fn table(&self, id: u16) -> Result<&TableDef, QueryError> {
        self.tables.iter().find(|t| t.id == id).ok_or(QueryError::UnknownTable)
    }

    /// Inserts or replaces the row with the same primary key, keeping every
    /// secondary index in sync (stale entries for a replaced row are removed).
    pub fn put(&self, kv: &mut dyn Kv, table: u16, row: &[QVal]) -> Result<(), QueryError> {
        let def = self.table(table)?;
        def.check_row(row)?;
        let mut pk_key = Vec::new();
        encode_key(&row[def.pk_col], &mut pk_key);

        // Replace semantics: drop the old row's index entries first.
        self.unindex_existing(kv, def, &pk_key)?;

        let mut row_bytes = Vec::new();
        encode_row(row, &mut row_bytes);
        kv.put(&row_key(table, &pk_key), &row_bytes);

        for &col in &def.indexed {
            let mut key = index_prefix(table, col);
            encode_key(&row[col], &mut key);
            key.extend_from_slice(&pk_key);
            kv.put(&key, &pk_key);
        }
        Ok(())
    }

    /// Deletes by primary key value. Missing rows are a no-op.
    pub fn delete(&self, kv: &mut dyn Kv, table: u16, pk: &QVal) -> Result<(), QueryError> {
        let def = self.table(table)?;
        if pk.kind() != def.columns[def.pk_col] {
            return Err(QueryError::TypeMismatch);
        }
        let mut pk_key = Vec::new();
        encode_key(pk, &mut pk_key);
        self.unindex_existing(kv, def, &pk_key)?;
        kv.delete(&row_key(def.id, &pk_key));
        Ok(())
    }

    fn unindex_existing(
        &self,
        kv: &mut dyn Kv,
        def: &TableDef,
        pk_key: &[u8],
    ) -> Result<(), QueryError> {
        let Some(bytes) = kv.get(&row_key(def.id, pk_key)) else {
            return Ok(());
        };
        let old = decode_row(&bytes).ok_or(QueryError::Corrupt)?;
        if old.len() != def.columns.len() {
            return Err(QueryError::Corrupt);
        }
        for &col in &def.indexed {
            let mut key = index_prefix(def.id, col);
            encode_key(&old[col], &mut key);
            key.extend_from_slice(pk_key);
            kv.delete(&key);
        }
        Ok(())
    }

    /// Reads one row by primary key.
    pub fn get(&self, kv: &dyn Kv, table: u16, pk: &QVal) -> Result<Option<Vec<QVal>>, QueryError> {
        let def = self.table(table)?;
        if pk.kind() != def.columns[def.pk_col] {
            return Err(QueryError::TypeMismatch);
        }
        let mut pk_key = Vec::new();
        encode_key(pk, &mut pk_key);
        match kv.get(&row_key(table, &pk_key)) {
            None => Ok(None),
            Some(bytes) => decode_row(&bytes).map(Some).ok_or(QueryError::Corrupt),
        }
    }

    /// Executes a v1 [`QuerySpec`]: index-driven scan on the order column,
    /// equality predicates filtered on decoded rows, `limit` rows per page,
    /// keyset continuation token bound to the spec's canonical hash.
    pub fn query(
        &self,
        kv: &dyn Kv,
        spec: &QuerySpec,
        token: Option<&PageToken>,
    ) -> Result<Page, QueryError> {
        let def = self.table(spec.table)?;
        self.check_spec(def, spec)?;
        let query_hash = spec.hash();
        if let Some(t) = token {
            if t.query_hash() != query_hash {
                return Err(QueryError::BadToken);
            }
        }

        // Scan window on the order column's index (pk orders drive the row
        // space directly; both spaces resume identically via the full key).
        let on_pk = spec.order_col == def.pk_col;
        let base = if on_pk {
            table_prefix(spec.table, TAG_ROW)
        } else {
            index_prefix(spec.table, spec.order_col)
        };
        let mut low = base.clone();
        let mut high = prefix_end(&base);
        if let Some(range) = &spec.range {
            if let Some(v) = &range.low {
                let mut k = base.clone();
                encode_key(v, &mut k);
                low = k; // inclusive: composite keys with this prefix sort >= it
            }
            if let Some(v) = &range.high {
                let mut k = base.clone();
                encode_key(v, &mut k);
                high = prefix_end(&k); // inclusive: everything with this value prefix
            }
        }
        // Token narrows the window to strictly-after the last emitted key.
        if let Some(t) = token {
            let last = t.last_index_key();
            if last < base.as_slice() || !last.starts_with(&base) {
                return Err(QueryError::BadToken);
            }
            if spec.descending {
                high = last.to_vec(); // exclusive end
            } else {
                let resumed = key_after(last);
                if resumed > low {
                    low = resumed;
                }
            }
        }

        let limit = spec.limit as usize;
        let mut rows: Vec<Vec<QVal>> = Vec::new();
        let mut last_key: Option<Vec<u8>> = None;
        let mut cursor_low = low;
        let mut cursor_high = high;
        let mut scanned = 0usize;
        let mut has_more = false;

        'scan: while scanned < MAX_SCAN {
            let batch_cap = (limit + 1 - rows.len()).max(1).min(MAX_SCAN - scanned) * 2;
            let batch = if spec.descending {
                kv.scan_rev(&cursor_low, &cursor_high, batch_cap)
            } else {
                kv.scan(&cursor_low, &cursor_high, batch_cap)
            };
            let exhausted = batch.len() < batch_cap;
            for (key, value) in &batch {
                scanned += 1;
                let row = if on_pk {
                    decode_row(value).ok_or(QueryError::Corrupt)?
                } else {
                    // Index entry: value = pk_key; fetch the primary row.
                    let bytes = kv.get(&row_key(spec.table, value)).ok_or(QueryError::Corrupt)?;
                    decode_row(&bytes).ok_or(QueryError::Corrupt)?
                };
                if row.len() != def.columns.len() {
                    return Err(QueryError::Corrupt);
                }
                if spec.eq.iter().all(|(col, want)| row.get(*col) == Some(want)) {
                    if rows.len() == limit {
                        has_more = true;
                        break 'scan;
                    }
                    rows.push(row);
                    last_key = Some(key.clone());
                }
                if spec.descending {
                    cursor_high = key.clone();
                } else {
                    cursor_low = key_after(key);
                }
            }
            if exhausted {
                break;
            }
        }

        let next = if has_more { last_key.map(|k| PageToken::new(query_hash, &k)) } else { None };
        Ok(Page { rows, next })
    }

    fn check_spec(&self, def: &TableDef, spec: &QuerySpec) -> Result<(), QueryError> {
        if spec.limit == 0 {
            return Err(QueryError::Unsupported);
        }
        if spec.order_col >= def.columns.len() {
            return Err(QueryError::UnknownColumn);
        }
        if !def.has_index(spec.order_col) {
            return Err(QueryError::Unsupported);
        }
        let order_ty = def.columns[spec.order_col];
        if let Some(range) = &spec.range {
            for bound in [&range.low, &range.high].into_iter().flatten() {
                if bound.kind() != order_ty {
                    return Err(QueryError::TypeMismatch);
                }
            }
        }
        for (col, value) in &spec.eq {
            let ty = def.columns.get(*col).ok_or(QueryError::UnknownColumn)?;
            if value.kind() != *ty {
                return Err(QueryError::TypeMismatch);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kv::MemKv;
    use alloc::string::String;
    use alloc::vec;

    fn users_table() -> TableDef {
        // (id Int pk, name Str, age Int[indexed], active Bool)
        TableDef {
            id: 1,
            columns: vec![QType::Int, QType::Str, QType::Int, QType::Bool],
            pk_col: 0,
            indexed: vec![2],
        }
    }

    fn user(id: i64, name: &str, age: i64, active: bool) -> Vec<QVal> {
        vec![QVal::Int(id), QVal::Str(String::from(name)), QVal::Int(age), QVal::Bool(active)]
    }

    fn seeded() -> (Engine, MemKv) {
        let engine = Engine::new(vec![users_table()]);
        let mut kv = MemKv::new();
        for (id, name, age, active) in [
            (3, "cara", 41, true),
            (1, "avery", 29, true),
            (5, "eli", 29, false),
            (2, "blair", 35, true),
            (4, "drew", 22, false),
        ] {
            engine.put(&mut kv, 1, &user(id, name, age, active)).unwrap();
        }
        (engine, kv)
    }

    #[test]
    fn get_put_replace_roundtrip() {
        let (engine, mut kv) = seeded();
        assert_eq!(engine.get(&kv, 1, &QVal::Int(3)).unwrap(), Some(user(3, "cara", 41, true)));
        engine.put(&mut kv, 1, &user(3, "cara", 42, true)).unwrap();
        assert_eq!(engine.get(&kv, 1, &QVal::Int(3)).unwrap(), Some(user(3, "cara", 42, true)));
    }

    #[test]
    fn replace_removes_stale_index_entries() {
        let (engine, mut kv) = seeded();
        let entries_before = kv.len();
        engine.put(&mut kv, 1, &user(3, "cara", 42, true)).unwrap();
        // Same row count: one row key + one index entry replaced, none leaked.
        assert_eq!(kv.len(), entries_before);
        // The old age=41 must no longer be reachable via the index.
        let spec = QuerySpec {
            table: 1,
            eq: vec![],
            range: Some(crate::spec::Range { low: Some(QVal::Int(41)), high: Some(QVal::Int(41)) }),
            order_col: 2,
            descending: false,
            limit: 10,
        };
        assert!(engine.query(&kv, &spec, None).unwrap().rows.is_empty());
    }

    #[test]
    fn delete_removes_row_and_index() {
        let (engine, mut kv) = seeded();
        engine.delete(&mut kv, 1, &QVal::Int(5)).unwrap();
        assert_eq!(engine.get(&kv, 1, &QVal::Int(5)).unwrap(), None);
        let spec = QuerySpec {
            table: 1,
            eq: vec![],
            range: None,
            order_col: 2,
            descending: false,
            limit: 10,
        };
        let page = engine.query(&kv, &spec, None).unwrap();
        assert_eq!(page.rows.len(), 4);
    }

    #[test]
    fn query_orders_by_indexed_column_with_pk_tiebreak() {
        let (engine, kv) = seeded();
        let spec = QuerySpec {
            table: 1,
            eq: vec![],
            range: None,
            order_col: 2,
            descending: false,
            limit: 10,
        };
        let page = engine.query(&kv, &spec, None).unwrap();
        let ids: Vec<i64> = page
            .rows
            .iter()
            .map(|r| match r[0] {
                QVal::Int(i) => i,
                _ => unreachable!(),
            })
            .collect();
        // ages: 22(drew=4), 29(avery=1, eli=5 — pk tiebreak), 35(blair=2), 41(cara=3)
        assert_eq!(ids, vec![4, 1, 5, 2, 3]);
        assert!(page.next.is_none());
    }

    #[test]
    fn descending_reverses_and_range_is_inclusive() {
        let (engine, kv) = seeded();
        let spec = QuerySpec {
            table: 1,
            eq: vec![],
            range: Some(crate::spec::Range { low: Some(QVal::Int(29)), high: Some(QVal::Int(35)) }),
            order_col: 2,
            descending: true,
            limit: 10,
        };
        let page = engine.query(&kv, &spec, None).unwrap();
        let ids: Vec<i64> = page
            .rows
            .iter()
            .map(|r| match r[0] {
                QVal::Int(i) => i,
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(ids, vec![2, 5, 1]); // 35, then 29 pk-desc
    }

    #[test]
    fn eq_filter_applies_on_decoded_rows() {
        let (engine, kv) = seeded();
        let spec = QuerySpec {
            table: 1,
            eq: vec![(3, QVal::Bool(true))],
            range: None,
            order_col: 0,
            descending: false,
            limit: 10,
        };
        let page = engine.query(&kv, &spec, None).unwrap();
        assert_eq!(page.rows.len(), 3);
        assert!(page.rows.iter().all(|r| r[3] == QVal::Bool(true)));
    }

    #[test]
    fn spec_violations_are_stable_errors() {
        let (engine, kv) = seeded();
        let base = QuerySpec {
            table: 1,
            eq: vec![],
            range: None,
            order_col: 0,
            descending: false,
            limit: 10,
        };
        let cases: [(QuerySpec, QueryError); 5] = [
            (QuerySpec { table: 9, ..base.clone() }, QueryError::UnknownTable),
            (QuerySpec { limit: 0, ..base.clone() }, QueryError::Unsupported),
            // order on an unindexed column (name = col 1)
            (QuerySpec { order_col: 1, ..base.clone() }, QueryError::Unsupported),
            (QuerySpec { order_col: 7, ..base.clone() }, QueryError::UnknownColumn),
            (QuerySpec { eq: vec![(1, QVal::Int(3))], ..base.clone() }, QueryError::TypeMismatch),
        ];
        for (spec, want) in cases {
            assert_eq!(engine.query(&kv, &spec, None).unwrap_err(), want);
        }
    }

    #[test]
    fn foreign_token_is_rejected() {
        let (engine, kv) = seeded();
        let spec_a = QuerySpec {
            table: 1,
            eq: vec![],
            range: None,
            order_col: 2,
            descending: false,
            limit: 2,
        };
        let spec_b = QuerySpec { limit: 3, ..spec_a.clone() };
        let page = engine.query(&kv, &spec_a, None).unwrap();
        let token = page.next.expect("has more");
        assert_eq!(engine.query(&kv, &spec_b, Some(&token)).unwrap_err(), QueryError::BadToken);
        // The minting query accepts it.
        assert!(engine.query(&kv, &spec_a, Some(&token)).is_ok());
    }
}
