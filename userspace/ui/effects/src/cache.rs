// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! LRU effect cache. Stores pre-computed blur/shadow results keyed by
//! (node_id, effect_type, parameters). Fixed-size, allocation-free after construction.

use alloc::vec::Vec;

/// Maximum entries in the effect cache. Fixed at construction time.
const DEFAULT_CACHE_CAPACITY: usize = 16;

/// A cached effect entry.
#[derive(Debug, Clone)]
pub struct CachedEffect {
    /// Unique key: (node_id, effect_kind tag, param hash)
    pub key: u64,
    /// Cached pixel data (RGBA8888)
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// LRU age: incremented on access, reset on insertion
    pub age: u64,
}

/// LRU cache for effect outputs.
#[derive(Debug, Clone)]
pub struct EffectCache {
    entries: Vec<CachedEffect>,
    capacity: usize,
    generation: u64,
}

impl EffectCache {
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CACHE_CAPACITY)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self { entries: Vec::with_capacity(capacity), capacity, generation: 0 }
    }

    /// Look up a cached effect by key. Returns the cached pixel data if found.
    /// Updates LRU age on hit.
    pub fn get(&mut self, key: u64) -> Option<&[u8]> {
        self.generation += 1;
        for entry in &mut self.entries {
            if entry.key == key {
                entry.age = self.generation;
                return Some(&entry.data);
            }
        }
        None
    }

    /// Insert (or replace) a cached effect. Evicts LRU entry if cache is full.
    pub fn insert(&mut self, key: u64, data: Vec<u8>, width: u32, height: u32) {
        self.generation += 1;

        // Update existing entry if key matches
        for entry in &mut self.entries {
            if entry.key == key {
                entry.data = data;
                entry.width = width;
                entry.height = height;
                entry.age = self.generation;
                return;
            }
        }

        // Evict LRU if full
        if self.entries.len() >= self.capacity {
            let lru_idx = self
                .entries
                .iter()
                .enumerate()
                .min_by_key(|(_, e)| e.age)
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.entries.swap_remove(lru_idx);
        }

        self.entries.push(CachedEffect {
            key,
            data,
            width,
            height,
            age: self.generation,
        });
    }

    /// Number of cached entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True if cache is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Clear all cached entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

impl Default for EffectCache {
    fn default() -> Self {
        Self::new()
    }
}
