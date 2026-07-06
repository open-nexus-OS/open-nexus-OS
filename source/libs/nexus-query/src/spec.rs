// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: QuerySpec value (typed predicates, order, limit) + canonical
//! hash + opaque keyset page tokens bound to the query identity.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered via engine integration tests
//!
//! v1 shape (docs/dev/dsl/db-queries.md): conjunction of equality predicates,
//! at most one range — and it must be on the order column (the index that
//! drives the scan, so results stream in order with no post-sort), one
//! `orderBy` column, a mandatory limit, keyset paging via an opaque token
//! bound to the query's canonical hash.

use crate::encoding::QVal;
use alloc::vec::Vec;

/// Inclusive bound of the (optional) range predicate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Range {
    pub low: Option<QVal>,
    pub high: Option<QVal>,
}

/// The v1 query value. Column references are schema column indices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuerySpec {
    pub table: u16,
    /// Equality predicates (column, value) — canonicalized by column index.
    pub eq: Vec<(usize, QVal)>,
    /// Optional range on the ORDER column (v1 rule).
    pub range: Option<Range>,
    /// The column driving order + the scan index.
    pub order_col: usize,
    pub descending: bool,
    /// Mandatory result cap per page.
    pub limit: u32,
}

impl QuerySpec {
    /// Canonical bytes: table, eq sorted by column, range, order, limit —
    /// independent of construction order.
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut eq = self.eq.clone();
        eq.sort_by_key(|(col, _)| *col);
        let mut out = Vec::new();
        out.extend_from_slice(&self.table.to_le_bytes());
        out.push(eq.len() as u8);
        for (col, value) in &eq {
            out.extend_from_slice(&(*col as u32).to_le_bytes());
            crate::encoding::encode_row(core::slice::from_ref(value), &mut out);
        }
        match &self.range {
            None => out.push(0),
            Some(range) => {
                out.push(1);
                for bound in [&range.low, &range.high] {
                    match bound {
                        None => out.push(0),
                        Some(value) => {
                            out.push(1);
                            crate::encoding::encode_row(core::slice::from_ref(value), &mut out);
                        }
                    }
                }
            }
        }
        out.extend_from_slice(&(self.order_col as u32).to_le_bytes());
        out.push(u8::from(self.descending));
        out.extend_from_slice(&self.limit.to_le_bytes());
        out
    }

    /// The query identity (FNV-1a 64 over the canonical bytes) — binds page
    /// tokens and keys caches. Identity, not integrity.
    #[must_use]
    pub fn hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for &b in &self.canonical_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }
}

/// One result page: decoded rows + the continuation (None = exhausted).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub rows: Vec<Vec<QVal>>,
    pub next: Option<PageToken>,
}

/// Opaque keyset continuation, bound to the issuing query's hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageToken {
    bytes: Vec<u8>,
}

impl PageToken {
    pub(crate) fn new(query_hash: u64, last_index_key: &[u8]) -> Self {
        let mut bytes = Vec::with_capacity(8 + last_index_key.len());
        bytes.extend_from_slice(&query_hash.to_le_bytes());
        bytes.extend_from_slice(last_index_key);
        Self { bytes }
    }

    /// Wire form (opaque to apps — pass through unchanged).
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Parses a wire token. `None` = malformed.
    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 8 {
            return None;
        }
        Some(Self { bytes: bytes.to_vec() })
    }

    pub(crate) fn query_hash(&self) -> u64 {
        let mut raw = [0u8; 8];
        raw.copy_from_slice(&self.bytes[..8]);
        u64::from_le_bytes(raw)
    }

    pub(crate) fn last_index_key(&self) -> &[u8] {
        &self.bytes[8..]
    }
}
