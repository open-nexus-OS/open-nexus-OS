// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: TASK-0203 / RFC-0075 Phase 4 — deterministic adaptive IME ranking.
//! Learns from committed candidates (never raw field text) and reorders an
//! engine's table-order candidate list so the user's frequent, recent and
//! in-context picks surface first — WITHOUT ever becoming nondeterministic or
//! unbounded. Fixed-point Q8.8 scoring, saturating counters, a bounded
//! storage-agnostic store, and stable tie-breakers. Host-first: this crate is
//! pure logic; TASK-0204 binds the store to statefsd and adds the OS wiring.
//! Password fields must never reach [`train`] — that is caller-gated in `imed`.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable (the goldens in tests/ are the contract)
//! TEST_COVERAGE: tests/ranking.rs, tests/eviction.rs
//! RFC: docs/rfcs/RFC-0075-ime-v2-text-focus-composition-delivery.md

#![cfg_attr(all(nexus_env = "os", target_os = "none"), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

mod ndjson;
mod score;
mod store;

use alloc::vec::Vec;

pub use ndjson::{
    export_ndjson, import_ndjson, ImportError, ImportReport, NDJSON_LINE_MAX, NDJSON_VERSION,
};
pub use score::{score, Bucket, ScoreInput, Q8_8};
pub use store::{CandKey, DictStat, MemStore, PersonalStore, CAND_MAX, DEFAULT_QUOTA};

/// Records ONE committed candidate: bumps its frequency (saturating), refreshes
/// its recency bucket, and — when there is a preceding committed candidate —
/// bumps the `(prev, cand)` context bigram. `prev` is the previously committed
/// candidate's bytes, or `None` at the start of a run.
///
/// The caller MUST NOT invoke this for password/secret fields (RFC-0075:
/// `field_kind=password` never learns) — training takes only committed
/// candidate bytes, never raw field text.
pub fn train<S: PersonalStore>(store: &mut S, prev: Option<&[u8]>, cand: &[u8], bucket: Bucket) {
    if CandKey::new(cand).is_none() {
        return; // fail-closed on empty/oversized candidate
    }
    let prior = store.get_dict(cand).unwrap_or_default();
    store.upsert_dict(
        cand,
        DictStat { freq: prior.freq.saturating_add(1), last_seen_bucket: bucket },
    );
    if let Some(prev) = prev {
        if CandKey::new(prev).is_some() {
            let count = store.get_bigram(prev, cand).saturating_add(1);
            store.upsert_bigram(prev, cand, count);
        }
    }
}

/// Ranks `candidates` (given in the engine's stable table order) for the
/// current context, returning a permutation of their indices, best first.
///
/// Ordering is deterministic with stable tie-breakers: **higher score first**,
/// then **lower table index** (preserves engine order for equal scores — so
/// untrained candidates stay put), then **candidate bytes ascending** (a final
/// total order). `prev` is the previously committed candidate for bigram
/// context (or `None`); `now` is the current recency bucket.
#[must_use]
pub fn rank<S: PersonalStore>(
    store: &S,
    prev: Option<&[u8]>,
    candidates: &[&[u8]],
    now: Bucket,
) -> Vec<usize> {
    let mut scored: Vec<(usize, Q8_8)> = candidates
        .iter()
        .enumerate()
        .map(|(index, cand)| {
            let stat = store.get_dict(cand).unwrap_or_default();
            let bigram = prev.map_or(0, |p| store.get_bigram(p, cand));
            let len = cand.len().min(u8::MAX as usize) as u8;
            let input = ScoreInput {
                freq: stat.freq,
                bigram,
                len,
                last_seen_bucket: stat.last_seen_bucket,
            };
            (index, score(input, now))
        })
        .collect();

    scored.sort_by(|&(ai, asc), &(bi, bsc)| {
        // Higher score first.
        bsc.cmp(&asc)
            // Then lower table index (stable — keep engine order on ties).
            .then(ai.cmp(&bi))
            // Then candidate bytes ascending (total, deterministic).
            .then_with(|| candidates[ai].cmp(candidates[bi]))
    });

    scored.into_iter().map(|(index, _)| index).collect()
}
