// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: OSK row DATA per layout (RFC-0075 Phase 8b) — the keymap is the
//! SSOT for how its on-screen keyboard is arranged. The ime-ui app renders
//! whatever rows arrive (`List` templates over `svc.ime.rows`); adding a
//! language is adding DATA here, never an `if` arm in any app. `label` is
//! what the key SHOWS, `key` is what it DISPATCHES (KR shows jamo, sends
//! the 2-set Latin key the engine maps); `action` non-empty marks a
//! control key (backspace) instead of a text key.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable (RFC-0075 Phase 8b seam)
//! TEST_COVERAGE: goldens in `tests/keymap_contract.rs` (row parity,
//! label/key alignment, bounded sizes).
//! RFC: docs/rfcs/RFC-0075-ime-v2-text-focus-composition-delivery.md

use crate::layout::LayoutId;

/// One on-screen key: display label, dispatched text key, optional action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OskKey {
    /// What the key cap SHOWS.
    pub label: &'static str,
    /// The character the tap DISPATCHES (`svc.ime.key`); empty for actions.
    pub key: &'static str,
    /// Non-empty = control key (`backspace`) dispatched via `svc.ime.action`.
    pub action: &'static str,
}

const fn t(label: &'static str, key: &'static str) -> OskKey {
    OskKey { label, key, action: "" }
}
const fn a(label: &'static str, action: &'static str) -> OskKey {
    OskKey { label, key: "", action }
}

/// Rows per keyboard (digits + three letter rows; the action row —
/// globe/space/enter — is the app's own static chrome).
pub const OSK_ROWS: usize = 4;

const DIGITS: &[OskKey] = &[
    t("1", "1"),
    t("2", "2"),
    t("3", "3"),
    t("4", "4"),
    t("5", "5"),
    t("6", "6"),
    t("7", "7"),
    t("8", "8"),
    t("9", "9"),
    t("0", "0"),
];

const US_R1: &[OskKey] = &[
    t("q", "q"),
    t("w", "w"),
    t("e", "e"),
    t("r", "r"),
    t("t", "t"),
    t("y", "y"),
    t("u", "u"),
    t("i", "i"),
    t("o", "o"),
    t("p", "p"),
];
const US_R2: &[OskKey] = &[
    t("a", "a"),
    t("s", "s"),
    t("d", "d"),
    t("f", "f"),
    t("g", "g"),
    t("h", "h"),
    t("j", "j"),
    t("k", "k"),
    t("l", "l"),
];
const US_R3: &[OskKey] = &[
    t("z", "z"),
    t("x", "x"),
    t("c", "c"),
    t("v", "v"),
    t("b", "b"),
    t("n", "n"),
    t("m", "m"),
    a("⌫", "backspace"),
];

const DE_R1: &[OskKey] = &[
    t("q", "q"),
    t("w", "w"),
    t("e", "e"),
    t("r", "r"),
    t("t", "t"),
    t("z", "z"),
    t("u", "u"),
    t("i", "i"),
    t("o", "o"),
    t("p", "p"),
    t("ü", "ü"),
];
const DE_R2: &[OskKey] = &[
    t("a", "a"),
    t("s", "s"),
    t("d", "d"),
    t("f", "f"),
    t("g", "g"),
    t("h", "h"),
    t("j", "j"),
    t("k", "k"),
    t("l", "l"),
    t("ö", "ö"),
    t("ä", "ä"),
];
const DE_R3: &[OskKey] = &[
    t("y", "y"),
    t("x", "x"),
    t("c", "c"),
    t("v", "v"),
    t("b", "b"),
    t("n", "n"),
    t("m", "m"),
    t("ß", "ß"),
    a("⌫", "backspace"),
];

/// KR 2-set: jamo LABELS over the Latin keys the engine maps (kr.rs table).
const KR_R1: &[OskKey] = &[
    t("ㅂ", "q"),
    t("ㅈ", "w"),
    t("ㄷ", "e"),
    t("ㄱ", "r"),
    t("ㅅ", "t"),
    t("ㅛ", "y"),
    t("ㅕ", "u"),
    t("ㅑ", "i"),
    t("ㅐ", "o"),
    t("ㅔ", "p"),
];
const KR_R2: &[OskKey] = &[
    t("ㅁ", "a"),
    t("ㄴ", "s"),
    t("ㅇ", "d"),
    t("ㄹ", "f"),
    t("ㅎ", "g"),
    t("ㅗ", "h"),
    t("ㅓ", "j"),
    t("ㅏ", "k"),
    t("ㅣ", "l"),
];
const KR_R3: &[OskKey] = &[
    t("ㅋ", "z"),
    t("ㅌ", "x"),
    t("ㅊ", "c"),
    t("ㅍ", "v"),
    t("ㅠ", "b"),
    t("ㅜ", "n"),
    t("ㅡ", "m"),
    a("⌫", "backspace"),
];

/// The OSK rows for `layout` (row 0 = digits, 1-3 = letter rows). JP and ZH
/// type romaji/pinyin — they SHARE the us rows (the engine converts); an
/// unknown row index is empty (the app's `List` renders nothing).
#[must_use]
pub fn osk_rows(layout: LayoutId, row: usize) -> &'static [OskKey] {
    let rows: [&[OskKey]; OSK_ROWS] = match layout {
        LayoutId::De => [DIGITS, DE_R1, DE_R2, DE_R3],
        LayoutId::Kr => [DIGITS, KR_R1, KR_R2, KR_R3],
        LayoutId::Us | LayoutId::Jp | LayoutId::Zh => [DIGITS, US_R1, US_R2, US_R3],
    };
    rows.get(row).copied().unwrap_or(&[])
}
