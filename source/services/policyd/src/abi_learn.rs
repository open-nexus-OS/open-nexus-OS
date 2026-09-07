// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0091 §5/§6 learn state — the per-subject ABI mode table
//! (`Enforce` default every boot, `Learn` set only through the
//! authenticated `OP_SET_ABI_MODE`, never persisted) and the bounded learn
//! collector: a dedup ring of `MAX_LEARN_ENTRIES` (subject, class, arg)
//! keys (a repeated key emits nothing), a token bucket of
//! `LEARN_RATE_PER_SEC` with burst `LEARN_BURST`, and a drop counter
//! (`abi.learn.dropped`). Lock-free atomics (policyd is single-threaded,
//! `#![forbid(unsafe_code)]`), `core`-only so host tests drive it with a
//! fake clock. Emission itself goes through `EvalHost` (logd on the OS,
//! a recorder in tests) and NEVER changes a decision.
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below; abi_eval.rs tests; abi_learn_roundtrip_tests.rs
//! RFC: docs/rfcs/RFC-0091-policy-profile-v2-schema-wire-argument-matchers.md

use core::sync::atomic::{AtomicU32, AtomicU64, AtomicU8, AtomicUsize, Ordering};

/// Dedup ring size (RFC-0091 §5 `MAX_LEARN_ENTRIES`).
pub const MAX_LEARN_ENTRIES: usize = 64;
/// Token bucket refill rate.
pub const LEARN_RATE_PER_SEC: u32 = 8;
/// Token bucket burst.
pub const LEARN_BURST: u32 = 32;
/// Subjects the mode table can hold at once.
pub const MAX_MODE_ENTRIES: usize = 16;
/// logd scope of learn records.
pub const LEARN_SCOPE: &str = "policyd.learn";

const NS_PER_TOKEN: u64 = 1_000_000_000 / LEARN_RATE_PER_SEC as u64;

/// Per-subject evaluation mode (wire values `ABI_MODE_*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum AbiMode {
    /// Decisions apply, nothing is learned.
    Enforce = 0,
    /// Decisions apply unchanged; would-deny evaluations emit learn records.
    Learn = 1,
}

impl AbiMode {
    /// Decodes the wire mode byte.
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Enforce),
            1 => Some(Self::Learn),
            _ => None,
        }
    }
}

/// Outcome of offering a learn key to the collector.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admission {
    /// Emit the record.
    Admit,
    /// Already seen — emit nothing, not a drop.
    Duplicate,
    /// Bucket empty — counted as a drop.
    RateLimited,
}

/// Bounded learn collector (dedup ring + token bucket + drop counter).
pub struct LearnState {
    keys: [AtomicU64; MAX_LEARN_ENTRIES],
    next: AtomicUsize,
    tokens: AtomicU32,
    last_refill_ns: AtomicU64,
    dropped: AtomicU32,
    emitted: AtomicU32,
    admitted: AtomicU32,
}

impl LearnState {
    /// Fresh state: empty ring, full bucket.
    pub const fn new() -> Self {
        Self {
            keys: [const { AtomicU64::new(0) }; MAX_LEARN_ENTRIES],
            next: AtomicUsize::new(0),
            tokens: AtomicU32::new(LEARN_BURST),
            last_refill_ns: AtomicU64::new(0),
            dropped: AtomicU32::new(0),
            emitted: AtomicU32::new(0),
            admitted: AtomicU32::new(0),
        }
    }

    fn refill(&self, now_ns: u64) {
        let last = self.last_refill_ns.load(Ordering::Relaxed);
        if now_ns <= last {
            return;
        }
        let earned = (now_ns - last) / NS_PER_TOKEN;
        if earned == 0 {
            return;
        }
        let tokens = self.tokens.load(Ordering::Relaxed);
        let add = u32::try_from(earned).unwrap_or(u32::MAX);
        self.tokens.store(tokens.saturating_add(add).min(LEARN_BURST), Ordering::Relaxed);
        self.last_refill_ns.store(last + earned * NS_PER_TOKEN, Ordering::Relaxed);
    }

    /// Offers a (subject, class, arg) key hash; `key == 0` is reserved.
    pub fn offer(&self, key: u64, now_ns: u64) -> Admission {
        let key = if key == 0 { 1 } else { key };
        if self.keys.iter().any(|k| k.load(Ordering::Relaxed) == key) {
            return Admission::Duplicate;
        }
        self.refill(now_ns);
        let tokens = self.tokens.load(Ordering::Relaxed);
        if tokens == 0 {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return Admission::RateLimited;
        }
        self.tokens.store(tokens - 1, Ordering::Relaxed);
        let slot = self.next.fetch_add(1, Ordering::Relaxed) % MAX_LEARN_ENTRIES;
        self.keys[slot].store(key, Ordering::Relaxed);
        self.admitted.fetch_add(1, Ordering::Relaxed);
        Admission::Admit
    }

    /// Counts an emission the host could not deliver (logd down).
    pub fn note_emit_failed(&self) {
        self.dropped.fetch_add(1, Ordering::Relaxed);
    }

    /// Counts a delivered record.
    pub fn note_emitted(&self) {
        self.emitted.fetch_add(1, Ordering::Relaxed);
    }

    /// `abi.learn.dropped` counter.
    pub fn dropped(&self) -> u32 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// Records the collector admitted (new key + token) — the learn
    /// pipeline's own witness; delivery to logd is best-effort (`emitted`).
    pub fn admitted(&self) -> u32 {
        self.admitted.load(Ordering::Relaxed)
    }

    /// Delivered records.
    pub fn emitted(&self) -> u32 {
        self.emitted.load(Ordering::Relaxed)
    }
}

impl Default for LearnState {
    fn default() -> Self {
        Self::new()
    }
}

/// Per-subject mode table (default `Enforce`; unknown subjects are `Enforce`).
pub struct ModeTable {
    subjects: [AtomicU64; MAX_MODE_ENTRIES],
    modes: [AtomicU8; MAX_MODE_ENTRIES],
}

impl ModeTable {
    /// Empty table.
    pub const fn new() -> Self {
        Self {
            subjects: [const { AtomicU64::new(0) }; MAX_MODE_ENTRIES],
            modes: [const { AtomicU8::new(0) }; MAX_MODE_ENTRIES],
        }
    }

    /// Mode of `subject` (`Enforce` when never set).
    pub fn mode_of(&self, subject: u64) -> AbiMode {
        for (s, m) in self.subjects.iter().zip(self.modes.iter()) {
            if s.load(Ordering::Relaxed) == subject && subject != 0 {
                return AbiMode::from_u8(m.load(Ordering::Relaxed)).unwrap_or(AbiMode::Enforce);
            }
        }
        AbiMode::Enforce
    }

    /// Sets the mode; `false` when the table is full (fail closed: stays Enforce).
    pub fn set_mode(&self, subject: u64, mode: AbiMode) -> bool {
        if subject == 0 {
            return false;
        }
        for (s, m) in self.subjects.iter().zip(self.modes.iter()) {
            if s.load(Ordering::Relaxed) == subject {
                m.store(mode as u8, Ordering::Relaxed);
                return true;
            }
        }
        if mode == AbiMode::Enforce {
            return true; // default already
        }
        for (s, m) in self.subjects.iter().zip(self.modes.iter()) {
            if s.load(Ordering::Relaxed) == 0 {
                m.store(mode as u8, Ordering::Relaxed);
                s.store(subject, Ordering::Relaxed);
                return true;
            }
        }
        false
    }
}

impl Default for ModeTable {
    fn default() -> Self {
        Self::new()
    }
}

/// What an evaluation needs from its environment.
pub trait EvalHost {
    /// Monotonic time for the token bucket.
    fn now_ns(&self) -> u64;
    /// The subject's mode.
    fn mode_of(&self, subject: u64) -> AbiMode;
    /// The collector.
    fn learn_state(&self) -> &LearnState;
    /// Delivers one record line (scope `policyd.learn`); `false` = not recorded.
    fn emit_learn(&mut self, record: &[u8]) -> bool;
    /// Applies an authenticated, epoch-checked mode switch; `false` = cannot
    /// (table full or a host without a mode table) — the caller fails closed.
    fn set_mode(&mut self, subject: u64, mode: AbiMode) -> bool;
}

/// A host that never learns (Enforce for everyone) — the side-effect-free path.
pub struct EnforceOnlyHost;

static ENFORCE_ONLY_STATE: LearnState = LearnState::new();

impl EvalHost for EnforceOnlyHost {
    fn now_ns(&self) -> u64 {
        0
    }
    fn mode_of(&self, _subject: u64) -> AbiMode {
        AbiMode::Enforce
    }
    fn learn_state(&self) -> &LearnState {
        &ENFORCE_ONLY_STATE
    }
    fn emit_learn(&mut self, _record: &[u8]) -> bool {
        false
    }
    fn set_mode(&mut self, _subject: u64, _mode: AbiMode) -> bool {
        false
    }
}

/// FNV-1a over the record's identity bytes (subject, class token, arg text).
pub fn learn_key(subject: u64, class: u8, arg: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in subject.to_le_bytes().iter().chain(core::iter::once(&class)).chain(arg.iter()) {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedup_then_rate_limit_then_refill() {
        let st = LearnState::new();
        assert_eq!(st.offer(1, 0), Admission::Admit);
        assert_eq!(st.offer(1, 0), Admission::Duplicate);
        // Burst of 32 admits (one used), then rate limited.
        for k in 2..=LEARN_BURST as u64 {
            assert_eq!(st.offer(k, 0), Admission::Admit, "key {k}");
        }
        assert_eq!(st.offer(1000, 0), Admission::RateLimited);
        assert_eq!(st.dropped(), 1);
        // 125 ms later one token is back.
        assert_eq!(st.offer(1000, NS_PER_TOKEN), Admission::Admit);
        assert_eq!(st.offer(1001, NS_PER_TOKEN), Admission::RateLimited);
        // A full second refills to the burst cap, never beyond.
        assert_eq!(st.offer(2000, 60_000_000_000), Admission::Admit);
        assert_eq!(st.tokens.load(Ordering::Relaxed), LEARN_BURST - 1);
        assert_eq!(st.dropped(), 2);
    }

    #[test]
    fn ring_forgets_oldest_key() {
        let st = LearnState::new();
        for k in 1..=(MAX_LEARN_ENTRIES as u64 + 1) {
            let _ = st.offer(k, k * 1_000_000_000);
        }
        // key 1 was evicted by key 65 → admitted again (not a duplicate);
        // that re-admission evicts key 2, while key 3 is still remembered.
        assert_eq!(st.offer(1, 200_000_000_000), Admission::Admit);
        assert_eq!(st.offer(3, 200_000_000_000), Admission::Duplicate);
        assert_eq!(st.offer(2, 200_000_000_000), Admission::Admit);
    }

    #[test]
    fn mode_table_defaults_to_enforce_and_bounds() {
        let t = ModeTable::new();
        assert_eq!(t.mode_of(7), AbiMode::Enforce);
        assert!(t.set_mode(7, AbiMode::Learn));
        assert_eq!(t.mode_of(7), AbiMode::Learn);
        assert!(t.set_mode(7, AbiMode::Enforce));
        assert_eq!(t.mode_of(7), AbiMode::Enforce);
        assert!(!t.set_mode(0, AbiMode::Learn));
        for s in 100..(100 + MAX_MODE_ENTRIES as u64 - 1) {
            assert!(t.set_mode(s, AbiMode::Learn));
        }
        // Table full (7 still occupies a slot) — fail closed: the new subject stays Enforce.
        assert!(!t.set_mode(999, AbiMode::Learn));
        assert_eq!(t.mode_of(999), AbiMode::Enforce);
    }

    #[test]
    fn learn_key_separates_subject_class_and_arg() {
        let a = learn_key(1, 1, b"/x/");
        assert_ne!(a, learn_key(2, 1, b"/x/"));
        assert_ne!(a, learn_key(1, 2, b"/x/"));
        assert_ne!(a, learn_key(1, 1, b"/y/"));
        assert_eq!(a, learn_key(1, 1, b"/x/"));
    }
}
