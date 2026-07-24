// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! TASK-0203 store goldens: quota eviction is bounded + deterministic (least
//! valuable entry leaves first), and `forget` erases a candidate and every
//! bigram that referenced it.

use ime_ranker::{train, DictStat, MemStore, PersonalStore};

#[test]
fn eviction_is_bounded_and_deterministic() {
    let build = || {
        let mut s = MemStore::with_quota(2);
        s.upsert_dict(b"aa", DictStat { freq: 5, last_seen_bucket: 10 });
        // "bb" is the least valuable (lowest freq) entry.
        s.upsert_dict(b"bb", DictStat { freq: 1, last_seen_bucket: 3 });
        // Inserting a 3rd NEW key at quota 2 evicts the lowest-freq entry (bb).
        s.upsert_dict(b"cc", DictStat { freq: 3, last_seen_bucket: 8 });
        s.dict_entries()
    };
    let entries = build();
    assert_eq!(entries.len(), 2, "quota never exceeded");
    let keys: Vec<Vec<u8>> = entries.iter().map(|(k, _)| k.as_bytes().to_vec()).collect();
    assert!(keys.contains(&b"aa".to_vec()));
    assert!(keys.contains(&b"cc".to_vec()));
    assert!(!keys.contains(&b"bb".to_vec()), "lowest-freq entry evicted");
    // Same inserts → same surviving set, every time.
    assert_eq!(build(), entries);
}

#[test]
fn updating_existing_key_never_evicts() {
    let mut s = MemStore::with_quota(2);
    s.upsert_dict(b"aa", DictStat { freq: 1, last_seen_bucket: 1 });
    s.upsert_dict(b"bb", DictStat { freq: 1, last_seen_bucket: 1 });
    // Re-upserting an existing key at quota keeps both (no spurious eviction).
    s.upsert_dict(b"aa", DictStat { freq: 9, last_seen_bucket: 9 });
    assert_eq!(s.dict_len(), 2);
    assert_eq!(s.get_dict(b"aa").map(|d| d.freq), Some(9));
}

#[test]
fn forget_removes_candidate_and_its_bigrams() {
    let mut s = MemStore::default();
    train(&mut s, None, b"aa", 1);
    train(&mut s, Some(b"aa"), b"bb", 2); // dict bb + bigram (aa,bb)
    assert!(s.get_dict(b"bb").is_some());
    assert_eq!(s.get_bigram(b"aa", b"bb"), 1);

    assert!(s.forget(b"bb"));
    assert!(s.get_dict(b"bb").is_none());
    assert_eq!(s.get_bigram(b"aa", b"bb"), 0, "referencing bigram erased too");
    assert!(!s.forget(b"bb"), "forget is idempotent");
}
