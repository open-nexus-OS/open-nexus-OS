// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! TASK-0204 Package 1 goldens: the statefs-agnostic persistence binding —
//! coalesced write-back, fail-closed load, and the `ime.personalization` gate
//! (off = no reads, no writes, no learning) — proven against a fake blob store.

use std::cell::Cell;
use std::collections::BTreeMap;

use ime_ranker::{BlobIo, PersistentStore, PersonalStore, BLOB_MAX};

const PATH: &str = "state:/ime/ja/personal.ndjson";

/// In-memory `BlobIo` that counts reads/writes so the toggle-off and
/// coalescing invariants can be asserted directly.
struct FakeIo {
    blobs: BTreeMap<String, Vec<u8>>,
    reads: Cell<usize>,
    writes: usize,
}

impl FakeIo {
    fn new() -> Self {
        Self { blobs: BTreeMap::new(), reads: Cell::new(0), writes: 0 }
    }
    fn seed(&mut self, path: &str, bytes: Vec<u8>) {
        self.blobs.insert(path.to_string(), bytes);
    }
}

impl BlobIo for FakeIo {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.reads.set(self.reads.get() + 1);
        self.blobs.get(path).cloned()
    }
    fn write(&mut self, path: &str, bytes: &[u8]) -> bool {
        self.writes += 1;
        self.blobs.insert(path.to_string(), bytes.to_vec());
        true
    }
}

fn cands() -> [&'static [u8]; 3] {
    [b"aa".as_slice(), b"bb".as_slice(), b"cc".as_slice()]
}

#[test]
fn persist_round_trip_preserves_ranking() {
    let mut io = FakeIo::new();
    let mut s = PersistentStore::new(true);
    // "cc" is LAST in table order; one commit must lift it — and survive reload.
    s.train(None, b"cc", 5);
    assert!(s.flush(&mut io, PATH), "dirty store writes");

    let mut restored = PersistentStore::new(true);
    restored.load(&io, PATH);
    assert_eq!(restored.rank(None, &cands(), 5)[0], 2, "learned order survives reload");
}

#[test]
fn flush_is_coalesced() {
    let mut io = FakeIo::new();
    let mut s = PersistentStore::new(true);
    s.train(None, b"cc", 1);
    assert!(s.flush(&mut io, PATH));
    assert_eq!(io.writes, 1);
    // Nothing changed → no second write.
    assert!(!s.flush(&mut io, PATH));
    assert_eq!(io.writes, 1);
}

#[test]
fn toggle_off_does_no_store_io() {
    let mut io = FakeIo::new();
    io.seed(PATH, b"{\"v\":1,\"kind\":\"ime-personal\"}\n".to_vec());
    let mut s = PersistentStore::new(false); // disabled
    s.load(&io, PATH);
    assert_eq!(io.reads.get(), 0, "disabled load reads nothing");
    assert!(s.store().dict_entries().is_empty());
    s.train(None, b"cc", 1); // no-op
    assert!(!s.flush(&mut io, PATH), "disabled flush writes nothing");
    assert_eq!(io.writes, 0);
}

#[test]
fn disabling_drops_learning() {
    let mut s = PersistentStore::new(true);
    s.train(None, b"cc", 1);
    assert!(!s.store().dict_entries().is_empty());
    s.set_enabled(false);
    assert!(s.store().dict_entries().is_empty(), "off drops all learning");
    assert!(!s.is_enabled());
}

#[test]
fn corrupt_blob_loads_empty_fail_closed() {
    let mut io = FakeIo::new();
    io.seed(PATH, b"garbage not ndjson\n{bad line}\n".to_vec());
    let mut s = PersistentStore::new(true);
    s.load(&io, PATH); // must not panic
    assert!(s.store().dict_entries().is_empty(), "corrupt blob → empty store");
}

#[test]
fn oversize_blob_rejected() {
    let mut io = FakeIo::new();
    io.seed(PATH, vec![b'x'; BLOB_MAX + 1]);
    let mut s = PersistentStore::new(true);
    s.load(&io, PATH);
    assert!(s.store().dict_entries().is_empty(), "oversized blob rejected");
}

#[test]
fn forget_all_truncates_on_flush() {
    let mut io = FakeIo::new();
    let mut s = PersistentStore::new(true);
    s.train(None, b"cc", 1);
    assert!(s.flush(&mut io, PATH));
    // Forget → flush truncates the blob to an empty store.
    s.forget_all();
    assert!(s.flush(&mut io, PATH));
    let mut restored = PersistentStore::new(true);
    restored.load(&io, PATH);
    assert!(restored.store().dict_entries().is_empty());
}
