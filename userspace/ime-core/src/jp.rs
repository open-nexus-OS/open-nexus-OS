// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: TASK-0149 — Japanese engine: incremental romaji→kana composition
//! (longest-match const table, っ sokuon doubling, ん rules) + a tiny const
//! kana→kanji lexicon feeding the candidate list. Deterministic and bounded;
//! sized for correctness proofs, not corpus coverage (real lexica ride
//! bundle assets in a later slice).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable (RFC-0075 Phase 3 seam)
//! TEST_COVERAGE: `tests/cjk_contract.rs` (romaji goldens, sokuon/ん edges,
//! candidate ordering, no-panic stream).
//! RFC: docs/rfcs/RFC-0075-ime-v2-text-focus-composition-delivery.md

use crate::engine::{page_of, CandidatePage, EngineOutcome, ImeEngine, TextRun};
use crate::outcome::{ImeAction, ImeKey};

/// Longest romaji chunk the matcher holds before it must resolve.
const ROMAJI_PENDING_MAX: usize = 4;

/// romaji → hiragana (exact match; the matcher asks longest-first). The
/// table stays FLAT + sorted by descending length at the call sites — order
/// inside the table is irrelevant (exact lookups only).
const ROMAJI_KANA: &[(&str, &str)] = &[
    ("a", "あ"),
    ("i", "い"),
    ("u", "う"),
    ("e", "え"),
    ("o", "お"),
    ("ka", "か"),
    ("ki", "き"),
    ("ku", "く"),
    ("ke", "け"),
    ("ko", "こ"),
    ("sa", "さ"),
    ("shi", "し"),
    ("si", "し"),
    ("su", "す"),
    ("se", "せ"),
    ("so", "そ"),
    ("ta", "た"),
    ("chi", "ち"),
    ("ti", "ち"),
    ("tsu", "つ"),
    ("tu", "つ"),
    ("te", "て"),
    ("to", "と"),
    ("na", "な"),
    ("ni", "に"),
    ("nu", "ぬ"),
    ("ne", "ね"),
    ("no", "の"),
    ("ha", "は"),
    ("hi", "ひ"),
    ("fu", "ふ"),
    ("hu", "ふ"),
    ("he", "へ"),
    ("ho", "ほ"),
    ("ma", "ま"),
    ("mi", "み"),
    ("mu", "む"),
    ("me", "め"),
    ("mo", "も"),
    ("ya", "や"),
    ("yu", "ゆ"),
    ("yo", "よ"),
    ("ra", "ら"),
    ("ri", "り"),
    ("ru", "る"),
    ("re", "れ"),
    ("ro", "ろ"),
    ("wa", "わ"),
    ("wo", "を"),
    ("ga", "が"),
    ("gi", "ぎ"),
    ("gu", "ぐ"),
    ("ge", "げ"),
    ("go", "ご"),
    ("za", "ざ"),
    ("ji", "じ"),
    ("zi", "じ"),
    ("zu", "ず"),
    ("ze", "ぜ"),
    ("zo", "ぞ"),
    ("da", "だ"),
    ("de", "で"),
    ("do", "ど"),
    ("ba", "ば"),
    ("bi", "び"),
    ("bu", "ぶ"),
    ("be", "べ"),
    ("bo", "ぼ"),
    ("pa", "ぱ"),
    ("pi", "ぴ"),
    ("pu", "ぷ"),
    ("pe", "ぺ"),
    ("po", "ぽ"),
    ("kya", "きゃ"),
    ("kyu", "きゅ"),
    ("kyo", "きょ"),
    ("sha", "しゃ"),
    ("shu", "しゅ"),
    ("sho", "しょ"),
    ("cha", "ちゃ"),
    ("chu", "ちゅ"),
    ("cho", "ちょ"),
    ("nya", "にゃ"),
    ("nyu", "にゅ"),
    ("nyo", "にょ"),
    ("hya", "ひゃ"),
    ("hyu", "ひゅ"),
    ("hyo", "ひょ"),
    ("mya", "みゃ"),
    ("myu", "みゅ"),
    ("myo", "みょ"),
    ("rya", "りゃ"),
    ("ryu", "りゅ"),
    ("ryo", "りょ"),
    ("gya", "ぎゃ"),
    ("gyu", "ぎゅ"),
    ("gyo", "ぎょ"),
    ("ja", "じゃ"),
    ("ju", "じゅ"),
    ("jo", "じょ"),
    ("bya", "びゃ"),
    ("byu", "びゅ"),
    ("byo", "びょ"),
    ("pya", "ぴゃ"),
    ("pyu", "ぴゅ"),
    ("pyo", "ぴょ"),
    ("-", "ー"),
];

/// kana → kanji candidates (deterministic order = table order; the kana
/// reading itself is ALWAYS appended as the last candidate at lookup).
const KANA_KANJI: &[(&str, &[&str])] = &[
    ("にほんご", &["日本語"]),
    ("にほん", &["日本"]),
    ("かんじ", &["漢字", "感じ", "幹事"]),
    ("みず", &["水"]),
    ("ひと", &["人", "一"]),
    ("わたし", &["私"]),
    ("きょう", &["今日", "京"]),
    ("じかん", &["時間"]),
];

/// Maximum candidates a lookup can yield (table row ≤ 8 + the reading).
const JP_MATCH_MAX: usize = 9;

/// Every character the jp engine can OUTPUT (kana table + lexicon) — the
/// font bake consumes this so candidate/commit glyphs are always covered.
/// Host-only (the bake is a build-time consumer).
#[cfg(not(all(nexus_env = "os", target_os = "none")))]
pub(crate) fn output_chars(out: &mut Vec<char>) {
    for (_, kana) in ROMAJI_KANA {
        out.extend(kana.chars());
    }
    for (reading, kanji) in KANA_KANJI {
        out.extend(reading.chars());
        for k in *kanji {
            out.extend(k.chars());
        }
    }
    out.push('っ');
    out.push('ん');
}

fn kana_of(romaji: &str) -> Option<&'static str> {
    ROMAJI_KANA.iter().find(|(r, _)| *r == romaji).map(|(_, k)| *k)
}

/// True when `romaji` is a strict prefix of at least one table key.
fn is_prefix(romaji: &str) -> bool {
    ROMAJI_KANA.iter().any(|(r, _)| r.len() > romaji.len() && r.starts_with(romaji))
}

/// Deterministic Japanese composition engine.
#[derive(Debug, Clone, Copy)]
pub struct JpEngine {
    /// Unresolved romaji tail (bounded).
    romaji: [u8; ROMAJI_PENDING_MAX],
    romaji_len: u8,
    /// Composed kana preedit.
    kana: TextRun,
    /// Candidate page cursor (space cycles pages).
    page: u8,
    /// True while the candidate list is showing (space pressed).
    selecting: bool,
}

impl Default for JpEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl JpEngine {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            romaji: [0; ROMAJI_PENDING_MAX],
            romaji_len: 0,
            kana: TextRun::empty(),
            page: 0,
            selecting: false,
        }
    }

    fn romaji_str(&self) -> &str {
        core::str::from_utf8(&self.romaji[..usize::from(self.romaji_len)]).unwrap_or("")
    }

    fn romaji_clear(&mut self) {
        self.romaji_len = 0;
    }

    fn romaji_push(&mut self, ch: char) -> bool {
        if !ch.is_ascii() || usize::from(self.romaji_len) >= ROMAJI_PENDING_MAX {
            return false;
        }
        self.romaji[usize::from(self.romaji_len)] = ch as u8;
        self.romaji_len += 1;
        true
    }

    fn kana_push_str(&mut self, s: &str) {
        for ch in s.chars() {
            let _ = self.kana.push(ch);
        }
    }

    /// Resolves the romaji tail as far as it deterministically can.
    fn resolve(&mut self) {
        loop {
            let tail = self.romaji_str();
            if tail.is_empty() {
                return;
            }
            // Sokuon: a doubled consonant (kk, tt, pp, …; not nn) emits っ
            // and keeps the second consonant pending.
            let bytes = tail.as_bytes();
            if bytes.len() >= 2
                && bytes[0] == bytes[1]
                && bytes[0] != b'n'
                && !matches!(bytes[0], b'a' | b'i' | b'u' | b'e' | b'o')
            {
                let _ = self.kana.push('っ');
                self.romaji.copy_within(1..usize::from(self.romaji_len), 0);
                self.romaji_len -= 1;
                continue;
            }
            // ん: `n` followed by a consonant other than y (n' handled as nn).
            if bytes.len() >= 2
                && bytes[0] == b'n'
                && !matches!(bytes[1], b'a' | b'i' | b'u' | b'e' | b'o' | b'y')
            {
                let _ = self.kana.push('ん');
                self.romaji.copy_within(1..usize::from(self.romaji_len), 0);
                self.romaji_len -= 1;
                if self.romaji_str() == "n" {
                    // `nn` collapsed into ん — the tail is consumed.
                    self.romaji_len = 0;
                }
                continue;
            }
            if let Some(kana) = kana_of(tail) {
                let owned: &'static str = kana;
                self.romaji_clear();
                self.kana_push_str(owned);
                return;
            }
            if is_prefix(tail) {
                return; // wait for more input
            }
            // Impossible tail: flush its first byte verbatim (never swallow).
            let first = bytes[0] as char;
            let _ = self.kana.push(first);
            self.romaji.copy_within(1..usize::from(self.romaji_len), 0);
            self.romaji_len -= 1;
        }
    }

    /// The match list for the current kana preedit: lexicon row (if any)
    /// then the reading itself.
    fn matches(&self) -> ([&'static str; JP_MATCH_MAX], usize, TextRun) {
        let mut out = [""; JP_MATCH_MAX];
        let mut n = 0;
        let reading = self.kana;
        if let Some((_, kanji)) = KANA_KANJI.iter().find(|(k, _)| *k == reading.as_str()) {
            for k in kanji.iter().take(JP_MATCH_MAX - 1) {
                out[n] = k;
                n += 1;
            }
        }
        (out, n, reading)
    }

    fn snapshot(&self, handled: bool, commit: TextRun) -> EngineOutcome {
        let mut preedit = self.kana;
        for ch in self.romaji_str().chars() {
            let _ = preedit.push(ch);
        }
        let candidates = if self.selecting {
            let (list, n, reading) = self.matches();
            let mut refs: [&str; JP_MATCH_MAX] = [""; JP_MATCH_MAX];
            refs[..n].copy_from_slice(&list[..n]);
            refs[n] = reading.as_str();
            page_of(&refs[..n + 1], self.page)
        } else {
            CandidatePage::empty()
        };
        EngineOutcome { handled, commit, preedit, candidates, ..EngineOutcome::default() }
    }

    fn commit_all(&mut self) -> TextRun {
        self.resolve();
        let mut out = self.kana;
        // Final flush: a trailing lone `n` is ん (no more input can follow).
        if self.romaji_str() == "n" {
            let _ = out.push('ん');
        } else {
            for ch in self.romaji_str().chars() {
                let _ = out.push(ch);
            }
        }
        self.reset();
        out
    }
}

impl ImeEngine for JpEngine {
    fn feed(&mut self, key: ImeKey) -> EngineOutcome {
        match key {
            ImeKey::Text(' ') => {
                if self.kana.is_empty() && self.romaji_len == 0 {
                    return EngineOutcome::pass();
                }
                self.resolve();
                if self.selecting {
                    self.page = self.page.wrapping_add(1);
                } else {
                    self.selecting = true;
                    self.page = 0;
                }
                self.snapshot(true, TextRun::empty())
            }
            ImeKey::Text(ch) if ch.is_ascii_lowercase() || ch == '-' => {
                self.selecting = false;
                if !self.romaji_push(ch) {
                    // Bound hit: flush what we have, then retry the push.
                    self.resolve();
                    let _ = self.romaji_push(ch);
                }
                self.resolve();
                self.snapshot(true, TextRun::empty())
            }
            ImeKey::Text(_) | ImeKey::Dead(_) => {
                // Non-romaji input commits the composition, passes the key.
                if self.kana.is_empty() && self.romaji_len == 0 {
                    return EngineOutcome::pass();
                }
                let commit = self.commit_all();
                EngineOutcome { commit, ..EngineOutcome::pass() }
            }
            ImeKey::Action(ImeAction::Backspace) => {
                if self.romaji_len > 0 {
                    self.romaji_len -= 1;
                } else if !self.kana.pop() {
                    return EngineOutcome::pass();
                }
                self.selecting = false;
                self.snapshot(true, TextRun::empty())
            }
            ImeKey::Action(ImeAction::Enter) => {
                if self.kana.is_empty() && self.romaji_len == 0 {
                    return EngineOutcome::pass();
                }
                let commit = self.commit_all();
                self.snapshot(true, commit)
            }
            ImeKey::Action(ImeAction::Escape) => {
                if self.kana.is_empty() && self.romaji_len == 0 {
                    return EngineOutcome::pass();
                }
                self.reset();
                self.snapshot(true, TextRun::empty())
            }
            ImeKey::Action(a) => {
                if self.kana.is_empty() && self.romaji_len == 0 {
                    return EngineOutcome::pass();
                }
                let commit = self.commit_all();
                EngineOutcome { handled: true, commit, pass_action: Some(a), ..Default::default() }
            }
        }
    }

    fn select(&mut self, index: usize) -> EngineOutcome {
        if !self.selecting {
            return EngineOutcome::pass();
        }
        let snapshot = self.snapshot(true, TextRun::empty());
        let Some(candidate) = snapshot.candidates.get(index) else {
            return EngineOutcome::pass();
        };
        let mut commit = TextRun::empty();
        for ch in candidate.as_str().chars() {
            let _ = commit.push(ch);
        }
        self.reset();
        self.snapshot(true, commit)
    }

    fn page_next(&mut self) -> EngineOutcome {
        if !self.selecting {
            return EngineOutcome::pass();
        }
        self.page = self.page.wrapping_add(1);
        self.snapshot(true, TextRun::empty())
    }

    fn reset(&mut self) {
        *self = Self::new();
    }
}
