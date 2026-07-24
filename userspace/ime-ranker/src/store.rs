// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: TASK-0203 / RFC-0075 Phase 4 — the storage-agnostic personalization
//! store. `PersonalStore` is the trait `imed`/TASK-0204 back with statefsd;
//! `MemStore` is the deterministic in-memory reference (BTree-ordered, bounded
//! by a per-locale quota with deterministic eviction). Keys are bounded
//! candidate bytes — never raw field text.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable (goldens in tests/ are the contract)
//! TEST_COVERAGE: tests/eviction.rs + tests/ranking.rs

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::score::Bucket;

/// Maximum candidate key length in UTF-8 bytes (matches `ime-core`'s
/// `CANDIDATE_MAX_BYTES`). Longer inputs are rejected fail-closed.
pub const CAND_MAX: usize = 32;

/// Default per-locale entry quota. Bounded so a session (or a hostile import)
/// can never grow the store without limit.
pub const DEFAULT_QUOTA: usize = 4096;

/// A bounded candidate key: the committed candidate's UTF-8 bytes. Ordered
/// lexicographically (zero-padded array order + length) so any `BTreeMap` keyed
/// by it iterates deterministically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct CandKey {
    bytes: [u8; CAND_MAX],
    len: u8,
}

impl CandKey {
    /// Builds a key from candidate bytes; rejects empty/oversized input
    /// (`None`, fail-closed) so nothing unbounded ever reaches the store.
    #[must_use]
    pub fn new(bytes: &[u8]) -> Option<Self> {
        if bytes.is_empty() || bytes.len() > CAND_MAX {
            return None;
        }
        let mut k = Self::default();
        k.bytes[..bytes.len()].copy_from_slice(bytes);
        k.len = bytes.len() as u8;
        Some(k)
    }

    /// The candidate bytes this key holds.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..usize::from(self.len)]
    }
}

/// Learned per-candidate statistics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DictStat {
    /// Commit count (saturating).
    pub freq: u16,
    /// Recency bucket of the last commit.
    pub last_seen_bucket: Bucket,
}

/// Storage-agnostic personalization store. Backends (in-memory here, statefsd
/// in TASK-0204) implement the primitives; ranking, training and NDJSON
/// interchange are written ONCE against this trait. Iteration MUST be
/// deterministic (sorted) so exports are byte-reproducible.
pub trait PersonalStore {
    /// Inserts or replaces a candidate's stats (may trigger eviction).
    fn upsert_dict(&mut self, cand: &[u8], stat: DictStat);
    /// Reads a candidate's stats, if learned.
    fn get_dict(&self, cand: &[u8]) -> Option<DictStat>;
    /// Inserts or replaces a `(prev, cand)` bigram count (may trigger eviction).
    fn upsert_bigram(&mut self, prev: &[u8], cand: &[u8], count: u16);
    /// Reads a `(prev, cand)` bigram count (0 when absent).
    fn get_bigram(&self, prev: &[u8], cand: &[u8]) -> u16;
    /// Removes a candidate (and its stats); returns whether it existed.
    fn forget(&mut self, cand: &[u8]) -> bool;
    /// Deterministic (sorted) snapshot of every dict entry.
    fn dict_entries(&self) -> Vec<(CandKey, DictStat)>;
    /// Deterministic (sorted) snapshot of every bigram.
    fn bigram_entries(&self) -> Vec<(CandKey, CandKey, u16)>;
}

/// The in-memory reference store: `BTreeMap`-backed (deterministic order),
/// bounded by `quota` for both dict and bigram tables, with deterministic
/// eviction of the least-valuable entry when a table would exceed it.
#[derive(Debug, Clone)]
pub struct MemStore {
    quota: usize,
    dict: BTreeMap<CandKey, DictStat>,
    bigrams: BTreeMap<(CandKey, CandKey), u16>,
}

impl Default for MemStore {
    fn default() -> Self {
        Self::with_quota(DEFAULT_QUOTA)
    }
}

impl MemStore {
    /// Builds a store with the given per-table entry quota.
    #[must_use]
    pub fn with_quota(quota: usize) -> Self {
        Self { quota: quota.max(1), dict: BTreeMap::new(), bigrams: BTreeMap::new() }
    }

    /// Current dict entry count.
    #[must_use]
    pub fn dict_len(&self) -> usize {
        self.dict.len()
    }

    /// Evicts the least-valuable dict entry: lowest `freq`, then oldest
    /// `last_seen_bucket`, then the largest key (stable, deterministic). Called
    /// when inserting a NEW key would exceed the quota.
    fn evict_one_dict(&mut self) {
        let victim = self
            .dict
            .iter()
            .min_by(|(ak, av), (bk, bv)| {
                av.freq
                    .cmp(&bv.freq)
                    .then(av.last_seen_bucket.cmp(&bv.last_seen_bucket))
                    // Largest key first as the final tiebreak: reverse key order.
                    .then(bk.cmp(ak))
            })
            .map(|(k, _)| *k);
        if let Some(k) = victim {
            self.dict.remove(&k);
        }
    }

    /// Evicts the least-valuable bigram: lowest count, then largest key pair.
    fn evict_one_bigram(&mut self) {
        let victim = self
            .bigrams
            .iter()
            .min_by(|(ak, ac), (bk, bc)| ac.cmp(bc).then(bk.cmp(ak)))
            .map(|(k, _)| *k);
        if let Some(k) = victim {
            self.bigrams.remove(&k);
        }
    }
}

impl PersonalStore for MemStore {
    fn upsert_dict(&mut self, cand: &[u8], stat: DictStat) {
        let Some(key) = CandKey::new(cand) else {
            return; // fail-closed on oversized input
        };
        // Only a NEW key can push us over quota; updating an existing one can't.
        if !self.dict.contains_key(&key) && self.dict.len() >= self.quota {
            self.evict_one_dict();
        }
        self.dict.insert(key, stat);
    }

    fn get_dict(&self, cand: &[u8]) -> Option<DictStat> {
        CandKey::new(cand).and_then(|k| self.dict.get(&k).copied())
    }

    fn upsert_bigram(&mut self, prev: &[u8], cand: &[u8], count: u16) {
        let (Some(p), Some(c)) = (CandKey::new(prev), CandKey::new(cand)) else {
            return;
        };
        let key = (p, c);
        if !self.bigrams.contains_key(&key) && self.bigrams.len() >= self.quota {
            self.evict_one_bigram();
        }
        self.bigrams.insert(key, count);
    }

    fn get_bigram(&self, prev: &[u8], cand: &[u8]) -> u16 {
        match (CandKey::new(prev), CandKey::new(cand)) {
            (Some(p), Some(c)) => self.bigrams.get(&(p, c)).copied().unwrap_or(0),
            _ => 0,
        }
    }

    fn forget(&mut self, cand: &[u8]) -> bool {
        let Some(key) = CandKey::new(cand) else {
            return false;
        };
        let existed = self.dict.remove(&key).is_some();
        // Drop any bigram that references the forgotten candidate (as prev or
        // cand), so "forget" leaves no learned trace of it.
        self.bigrams.retain(|(p, c), _| *p != key && *c != key);
        existed
    }

    fn dict_entries(&self) -> Vec<(CandKey, DictStat)> {
        self.dict.iter().map(|(k, v)| (*k, *v)).collect()
    }

    fn bigram_entries(&self) -> Vec<(CandKey, CandKey, u16)> {
        self.bigrams.iter().map(|((p, c), n)| (*p, *c, *n)).collect()
    }
}
