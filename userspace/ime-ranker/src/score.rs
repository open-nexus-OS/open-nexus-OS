// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: TASK-0203 / RFC-0075 Phase 4 — the deterministic candidate score as
//! a pure fixed-point function. Q8.8 (value × 256, `i32`) so the result is
//! bit-reproducible: no floats, no RNG, no raw timestamps. Four additive
//! signals — personal frequency, context bigram, a mild length prior, and a
//! coarse recency bucket — each `count_capped × unit`, so the unit IS the
//! weight. Higher score = ranked earlier.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable (goldens in tests/ are the contract)
//! TEST_COVERAGE: tests/ranking.rs (score ordering + determinism)

/// A fixed-point Q8.8 value: the logical number × 256, held in an `i32`
/// (`1.0` == `256`). All scoring is integer arithmetic for reproducibility.
pub type Q8_8 = i32;

/// A coarse, monotonically non-decreasing recency counter supplied by the
/// caller (e.g. one tick per session or per hour) — NEVER a raw timestamp, so
/// the store leaks no wall-clock and stays reproducible.
pub type Bucket = u16;

// ——— Signal weights (Q8.8 units). Training dominates: one commit (freq unit)
// outweighs the whole length-prior range, so a trained candidate overtakes the
// static table order, while untrained candidates keep ~table order (the length
// prior only mildly reshuffles them, broken by the base-rank tiebreak). ———

/// Per-commit frequency contribution; capped so a runaway counter can't swamp
/// the other signals.
const FREQ_UNIT: Q8_8 = 64;
const FREQ_CAP: u16 = 64;

/// Per-occurrence contribution of a matched `(prev, cand)` context bigram — the
/// strongest signal, since it means "you picked this right after that before".
const BIGRAM_UNIT: Q8_8 = 96;
const BIGRAM_CAP: u16 = 64;

/// Mild bias toward shorter candidates (`(LEN_REF - len) × LEN_UNIT`), capped at
/// `LEN_REF`. Small on purpose: a tie-breaker between untrained candidates, not
/// a driver.
const LEN_UNIT: Q8_8 = 4;
const LEN_REF: u16 = 16;

/// Recency: candidates seen within the last `RECENCY_SPAN` buckets get a bonus
/// that decays linearly to zero (`(RECENCY_SPAN - delta) × RECENCY_UNIT`).
const RECENCY_UNIT: Q8_8 = 16;
const RECENCY_SPAN: u16 = 64;

/// The inputs to [`score`] for one candidate: its learned stats plus the
/// in-context bigram count and its byte length.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScoreInput {
    /// How many times this candidate was committed (saturating).
    pub freq: u16,
    /// Occurrences of the `(prev, cand)` bigram for the current `prev`
    /// (0 when there is no preceding context).
    pub bigram: u16,
    /// Candidate length in UTF-8 bytes.
    pub len: u8,
    /// Recency bucket of the last commit (see [`Bucket`]).
    pub last_seen_bucket: Bucket,
}

/// Computes the Q8.8 rank score for one candidate at recency bucket `now`.
/// Pure and total: every input saturates, so it never panics or overflows.
#[must_use]
pub fn score(input: ScoreInput, now: Bucket) -> Q8_8 {
    let freq_pts = i32::from(input.freq.min(FREQ_CAP)) * FREQ_UNIT;
    let bigram_pts = i32::from(input.bigram.min(BIGRAM_CAP)) * BIGRAM_UNIT;

    let len_ref = LEN_REF;
    let len = u16::from(input.len).min(len_ref);
    let len_pts = i32::from(len_ref - len) * LEN_UNIT;

    // Delta is saturating: a `last_seen` ahead of `now` (should not happen, but
    // is defended) yields 0 delta, i.e. maximum recency, never a negative.
    let delta = now.saturating_sub(input.last_seen_bucket).min(RECENCY_SPAN);
    // Untrained candidates (last_seen_bucket == 0, freq == 0) get no recency
    // bonus unless `now` is also near 0 — that first-boot window is harmless.
    let recency_pts =
        if input.freq == 0 { 0 } else { i32::from(RECENCY_SPAN - delta) * RECENCY_UNIT };

    freq_pts + bigram_pts + len_pts + recency_pts
}
