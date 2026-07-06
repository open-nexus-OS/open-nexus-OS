// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Ordered key-value storage seam (`Kv` trait) + in-memory host
//! backend — the SAME engine runs over statefsd (queryd) and `MemKv`.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered via engine integration tests
//!
//! OS: implemented over statefsd's journaled KV (queryd). Host: the in-memory
//! [`MemKv`] — the SAME engine runs against both, so host proofs transfer.

use alloc::vec::Vec;

/// Ordered KV storage. Keys sort byte-wise (the engine's key encoding makes
/// byte order == value order).
pub trait Kv {
    fn get(&self, key: &[u8]) -> Option<Vec<u8>>;
    fn put(&mut self, key: &[u8], value: &[u8]);
    fn delete(&mut self, key: &[u8]);
    /// Ascending scan of `start..end` (end exclusive), up to `limit` entries.
    fn scan(&self, start: &[u8], end: &[u8], limit: usize) -> Vec<(Vec<u8>, Vec<u8>)>;
    /// Descending scan of `start..end` (end exclusive), up to `limit` entries.
    fn scan_rev(&self, start: &[u8], end: &[u8], limit: usize) -> Vec<(Vec<u8>, Vec<u8>)>;
}

/// In-memory ordered map — the host/test backend.
#[derive(Default)]
pub struct MemKv {
    map: alloc::collections::BTreeMap<Vec<u8>, Vec<u8>>,
}

impl MemKv {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

impl Kv for MemKv {
    fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.map.get(key).cloned()
    }

    fn put(&mut self, key: &[u8], value: &[u8]) {
        self.map.insert(key.to_vec(), value.to_vec());
    }

    fn delete(&mut self, key: &[u8]) {
        self.map.remove(key);
    }

    fn scan(&self, start: &[u8], end: &[u8], limit: usize) -> Vec<(Vec<u8>, Vec<u8>)> {
        self.map
            .range(start.to_vec()..end.to_vec())
            .take(limit)
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    fn scan_rev(&self, start: &[u8], end: &[u8], limit: usize) -> Vec<(Vec<u8>, Vec<u8>)> {
        self.map
            .range(start.to_vec()..end.to_vec())
            .rev()
            .take(limit)
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }
}
