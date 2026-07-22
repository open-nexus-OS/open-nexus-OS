// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: TASK-0149 — the bounded in-memory user dictionary API
//! (`train`/`lookup`/`forget`): per-language personalization the engines
//! consult BEFORE their const tables. Storage-agnostic by design —
//! persistence (statefsd `state:/ime/…`) and adaptive ranking land with
//! TASK-0203/0204; this slice fixes the deterministic semantics: frequency
//! ranking with insertion-order tie-breaks and lowest-frequency-first,
//! oldest-first eviction. Password fields must never reach `train` — the
//! caller (imed) gates on `field_kind`, and the API keeps training an
//! EXPLICIT separate call so that gate cannot be bypassed by lookups.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable (RFC-0075 Phase 3 seam)
//! TEST_COVERAGE: `tests/cjk_contract.rs` (train/lookup/forget/eviction).
//! RFC: docs/rfcs/RFC-0075-ime-v2-text-focus-composition-delivery.md

/// Maximum bytes per reading key (kana/pinyin/jamo string).
pub const USERDICT_KEY_MAX: usize = 24;
/// Maximum bytes per stored text.
pub const USERDICT_TEXT_MAX: usize = 24;
/// In-memory entry cap per language (RFC-0075 bound).
pub const USERDICT_CAP: usize = 1024;

#[derive(Debug, Clone, Copy)]
struct Entry {
    key: [u8; USERDICT_KEY_MAX],
    key_len: u8,
    text: [u8; USERDICT_TEXT_MAX],
    text_len: u8,
    freq: u16,
    /// Monotonic insertion stamp — the deterministic tie-breaker.
    seq: u32,
}

impl Entry {
    fn key_str(&self) -> &str {
        core::str::from_utf8(&self.key[..usize::from(self.key_len)]).unwrap_or("")
    }
    fn text_str(&self) -> &str {
        core::str::from_utf8(&self.text[..usize::from(self.text_len)]).unwrap_or("")
    }
}

/// Bounded, deterministic user dictionary (one per language).
///
/// `N` is the entry cap (defaults to [`USERDICT_CAP`]; tests shrink it to
/// prove eviction without a thousand inserts).
#[derive(Debug)]
pub struct UserDict<const N: usize = USERDICT_CAP> {
    entries: [Option<Entry>; N],
    len: usize,
    next_seq: u32,
}

impl<const N: usize> Default for UserDict<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> UserDict<N> {
    #[must_use]
    pub const fn new() -> Self {
        Self { entries: [None; N], len: 0, next_seq: 0 }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn position(&self, key: &str, text: &str) -> Option<usize> {
        self.entries
            .iter()
            .position(|e| e.as_ref().is_some_and(|e| e.key_str() == key && e.text_str() == text))
    }

    /// Trains `(key → text)`: bumps an existing pair's frequency (saturating)
    /// or inserts it. A full dict evicts the LOWEST-frequency entry, oldest
    /// first (deterministic). Oversized inputs are rejected (fail-closed).
    pub fn train(&mut self, key: &str, text: &str) -> bool {
        if key.is_empty()
            || text.is_empty()
            || key.len() > USERDICT_KEY_MAX
            || text.len() > USERDICT_TEXT_MAX
        {
            return false;
        }
        if let Some(i) = self.position(key, text) {
            if let Some(e) = self.entries[i].as_mut() {
                e.freq = e.freq.saturating_add(1);
            }
            return true;
        }
        let slot = if self.len < N {
            self.entries.iter().position(Option::is_none)
        } else {
            // Evict: lowest frequency, then oldest insertion stamp.
            self.entries
                .iter()
                .enumerate()
                .filter_map(|(i, e)| e.as_ref().map(|e| (i, e.freq, e.seq)))
                .min_by_key(|&(_, freq, seq)| (freq, seq))
                .map(|(i, _, _)| i)
        };
        let Some(slot) = slot else {
            return false; // N == 0 — a degenerate cap stays a no-op
        };
        if self.entries[slot].is_none() {
            self.len += 1;
        }
        let mut e = Entry {
            key: [0; USERDICT_KEY_MAX],
            key_len: key.len() as u8,
            text: [0; USERDICT_TEXT_MAX],
            text_len: text.len() as u8,
            freq: 1,
            seq: self.next_seq,
        };
        e.key[..key.len()].copy_from_slice(key.as_bytes());
        e.text[..text.len()].copy_from_slice(text.as_bytes());
        self.entries[slot] = Some(e);
        self.next_seq = self.next_seq.wrapping_add(1);
        true
    }

    /// Removes one trained pair ("Vorschlag vergessen"). True when found.
    pub fn forget(&mut self, key: &str, text: &str) -> bool {
        if let Some(i) = self.position(key, text) {
            self.entries[i] = None;
            self.len -= 1;
            return true;
        }
        false
    }

    /// Fills `out` with the texts trained for `key`, ranked by frequency
    /// (desc), ties by insertion stamp (asc — earlier training wins).
    /// Returns the match count (≤ `out.len()`).
    pub fn lookup<'a>(&'a self, key: &str, out: &mut [&'a str]) -> usize {
        let mut n = 0;
        // Selection by rank without allocation: repeatedly take the best
        // not-yet-emitted entry (bounded: out.len() × N comparisons).
        let mut last: Option<(u16, u32)> = None;
        while n < out.len() {
            let best = self
                .entries
                .iter()
                .filter_map(|e| e.as_ref())
                .filter(|e| e.key_str() == key)
                .filter(|e| match last {
                    // Strictly after the previously emitted rank position.
                    Some((freq, seq)) => (u16::MAX - e.freq, e.seq) > (u16::MAX - freq, seq),
                    None => true,
                })
                .min_by_key(|e| (u16::MAX - e.freq, e.seq));
            let Some(e) = best else { break };
            out[n] = e.text_str();
            last = Some((e.freq, e.seq));
            n += 1;
        }
        n
    }
}
