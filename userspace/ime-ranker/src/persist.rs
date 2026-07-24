// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: TASK-0204 Package 1 / RFC-0075 Phase 4 — binds the ranking store to
//! a byte-blob backend behind the `BlobIo` trait (imed backs it with statefsd
//! `state:/ime/<lang>/…`; host tests use an in-memory fake). One NDJSON blob per
//! locale holds the whole store (dict + bigrams — the format is self-describing,
//! so one file replaces the two the task sketched). Write-back is coalesced: a
//! mutation only marks the store dirty; `flush` exports to the blob just once,
//! and only when dirty, enabled, and under the size bound. Load is fail-closed:
//! a missing / oversized / corrupt blob yields an EMPTY store, never a failure.
//! The `enabled` gate is the `ime.personalization` toggle — off means NO reads,
//! NO writes, NO learning.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: tests/persist.rs (round-trip, toggle-off, corrupt/oversize)

use alloc::vec::Vec;

use crate::ndjson::{export_ndjson, import_ndjson};
use crate::score::Bucket;
use crate::store::{MemStore, DEFAULT_QUOTA};

/// Maximum personalization blob size (bytes): bounds both the write (a full
/// export must fit) and the read (a larger stored blob is rejected fail-closed).
pub const BLOB_MAX: usize = 64 * 1024;

/// Byte-blob storage for one locale's personalization file. Backends read/write
/// whole blobs by path; imed implements this over statefsd, host tests over a
/// map. Kept minimal so a backend has nothing to get wrong.
pub trait BlobIo {
    /// Reads the blob at `path`, or `None` when it does not exist.
    fn read(&self, path: &str) -> Option<Vec<u8>>;
    /// Writes `bytes` to `path`; returns whether the write succeeded.
    fn write(&mut self, path: &str, bytes: &[u8]) -> bool;
}

/// A ranking [`MemStore`] bound to a persistence blob, with coalesced
/// write-back and a personalization on/off gate. Mutations set a dirty flag;
/// [`flush`](Self::flush) is the only thing that writes.
#[derive(Debug, Clone)]
pub struct PersistentStore {
    store: MemStore,
    dirty: bool,
    enabled: bool,
}

impl PersistentStore {
    /// Builds a store with the default per-locale quota and the given
    /// personalization state (the `ime.personalization` toggle).
    #[must_use]
    pub fn new(enabled: bool) -> Self {
        Self { store: MemStore::with_quota(DEFAULT_QUOTA), dirty: false, enabled }
    }

    /// Whether personalization is on.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Applies the `ime.personalization` toggle. Turning it OFF drops all
    /// in-memory learning immediately (and marks dirty so the next `flush`
    /// truncates the blob) — off must mean nothing learned is retained or used.
    pub fn set_enabled(&mut self, on: bool) {
        if self.enabled && !on {
            self.store = MemStore::with_quota(DEFAULT_QUOTA);
            self.dirty = true;
        }
        self.enabled = on;
    }

    /// Read-only access to the underlying store (for ranking).
    #[must_use]
    pub fn store(&self) -> &MemStore {
        &self.store
    }

    /// Records one committed candidate (no-op when disabled). Only marks the
    /// store dirty — the blob is written later by `flush`.
    pub fn train(&mut self, prev: Option<&[u8]>, cand: &[u8], bucket: Bucket) {
        if !self.enabled {
            return;
        }
        crate::train(&mut self.store, prev, cand, bucket);
        self.dirty = true;
    }

    /// Ranks candidates against the learned store (see [`crate::rank`]); returns
    /// the identity order when disabled so callers need no separate gate.
    #[must_use]
    pub fn rank(&self, prev: Option<&[u8]>, candidates: &[&[u8]], now: Bucket) -> Vec<usize> {
        if !self.enabled {
            return (0..candidates.len()).collect();
        }
        crate::rank(&self.store, prev, candidates, now)
    }

    /// The "Forget learned words" action: clears all learning and marks dirty so
    /// the next `flush` truncates the blob.
    pub fn forget_all(&mut self) {
        self.store = MemStore::with_quota(DEFAULT_QUOTA);
        self.dirty = true;
    }

    /// Loads the store from `path` via `io`, fail-closed: disabled, missing,
    /// oversized, or corrupt input all leave an EMPTY store — never an error.
    /// Clears the dirty flag (the in-memory state now matches the blob).
    pub fn load<B: BlobIo>(&mut self, io: &B, path: &str) {
        self.store = MemStore::with_quota(DEFAULT_QUOTA);
        self.dirty = false;
        if !self.enabled {
            return;
        }
        let Some(bytes) = io.read(path) else {
            return; // no blob yet — empty store
        };
        if bytes.len() > BLOB_MAX {
            return; // oversized — reject fail-closed, stay empty
        }
        let Ok(text) = core::str::from_utf8(&bytes) else {
            return; // non-UTF-8 blob — reject
        };
        // A bad header rejects the whole file (store stays empty); skipped lines
        // are already dropped inside import. Either way, never a failure.
        let _ = import_ndjson(&mut self.store, text);
    }

    /// Writes the store back to `path` via `io` IF dirty and enabled and the
    /// export fits the size bound. Returns whether a write happened. Clears the
    /// dirty flag on a successful write (coalesced — safe to call on idle).
    pub fn flush<B: BlobIo>(&mut self, io: &mut B, path: &str) -> bool {
        if !self.enabled || !self.dirty {
            return false;
        }
        let text = export_ndjson(&self.store);
        if text.len() > BLOB_MAX {
            return false; // never write an over-bound blob (quota should prevent this)
        }
        if io.write(path, text.as_bytes()) {
            self.dirty = false;
            true
        } else {
            false
        }
    }
}
