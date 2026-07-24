// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! TASK-0203 ranking goldens: these pin the deterministic contract —
//! untrained candidates keep table order, a trained candidate overtakes it, the
//! bigram lifts only in context, recency favors the recently seen, and identical
//! training is byte-reproducible.

use ime_ranker::{rank, train, MemStore, PersonalStore};

fn cands<'a>(v: &'a [&'a str]) -> Vec<&'a [u8]> {
    v.iter().map(|s| s.as_bytes()).collect()
}

#[test]
fn untrained_same_length_preserves_table_order() {
    let store = MemStore::default();
    let c = cands(&["aa", "bb", "cc"]);
    // No learning + equal length → pure engine order (score ties broken by index).
    assert_eq!(rank(&store, None, &c, 0), [0, 1, 2]);
}

#[test]
fn trained_candidate_overtakes_table_order() {
    let mut store = MemStore::default();
    let c = cands(&["aa", "bb", "cc"]);
    // "cc" sits LAST in table order; one commit must lift it to the front.
    train(&mut store, None, b"cc", 5);
    assert_eq!(rank(&store, None, &c, 5)[0], 2);
}

#[test]
fn bigram_lifts_in_context_only() {
    let mut store = MemStore::default();
    // Equal frequency for both, but "bb" was committed after "zz".
    train(&mut store, None, b"aa", 1);
    train(&mut store, Some(b"zz"), b"bb", 1);
    let c = cands(&["aa", "bb"]);
    // No context: equal freq/recency/length → table order.
    assert_eq!(rank(&store, None, &c, 1), [0, 1]);
    // Context "zz": the (zz,bb) bigram lifts "bb" to the front.
    assert_eq!(rank(&store, Some(b"zz"), &c, 1)[0], 1);
    // A DIFFERENT context has no matching bigram → back to table order.
    assert_eq!(rank(&store, Some(b"qq"), &c, 1), [0, 1]);
}

#[test]
fn recency_favors_recently_seen() {
    let mut store = MemStore::default();
    // Equal frequency, but "bb" was seen much more recently.
    train(&mut store, None, b"aa", 1);
    train(&mut store, None, b"bb", 50);
    let c = cands(&["aa", "bb"]);
    assert_eq!(rank(&store, None, &c, 50)[0], 1);
}

#[test]
fn training_snapshot_is_deterministic() {
    let build = || {
        let mut s = MemStore::default();
        for (prev, cand, bucket) in
            [(None, "aa", 1u16), (Some("aa"), "bb", 2), (None, "aa", 3), (Some("bb"), "cc", 4)]
        {
            train(&mut s, prev.map(str::as_bytes), cand.as_bytes(), bucket);
        }
        (s.dict_entries(), s.bigram_entries())
    };
    // Identical training → identical (sorted) snapshot, twice over.
    assert_eq!(build(), build());
}

#[test]
fn oversized_candidate_is_ignored_fail_closed() {
    let mut store = MemStore::default();
    let big = [b'x'; 64]; // > CAND_MAX (32)
    train(&mut store, None, &big, 1); // must be a no-op, not a panic
    assert!(store.get_dict(&big).is_none());
    assert!(store.dict_entries().is_empty());
}
