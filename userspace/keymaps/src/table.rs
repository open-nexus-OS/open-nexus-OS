// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Table-driven base keymap definitions for `us`, `de`, `jp`, `kr`, and `zh`.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: No direct tests (covered by 4 integration tests in `tests/input_v1_0_host/tests/keymaps_contract.rs`).
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

use hid::KeyboardUsage;

use crate::{KeyAction, KeyOutput};

#[derive(Debug, Clone, Copy)]
pub struct MappingEntry {
    pub usage: KeyboardUsage,
    pub base: KeyOutput,
    pub shifted: Option<KeyOutput>,
    pub alt_gr: Option<KeyOutput>,
}

pub const fn text(ch: char) -> KeyOutput {
    KeyOutput::Text(ch)
}

pub const fn action(action: KeyAction) -> KeyOutput {
    KeyOutput::Action(action)
}

pub const fn entry(
    usage: KeyboardUsage,
    base: KeyOutput,
    shifted: Option<KeyOutput>,
    alt_gr: Option<KeyOutput>,
) -> MappingEntry {
    MappingEntry { usage, base, shifted, alt_gr }
}

pub fn lookup(entries: &[MappingEntry], usage: KeyboardUsage) -> Option<&MappingEntry> {
    entries.iter().find(|entry| entry.usage == usage)
}

pub mod us {
    use hid::KeyboardUsage;

    use super::{action, entry, text, MappingEntry};
    use crate::KeyAction;

    pub const TABLE: &[MappingEntry] = &[
        entry(KeyboardUsage::A, text('a'), Some(text('A')), None),
        entry(KeyboardUsage::B, text('b'), Some(text('B')), None),
        entry(KeyboardUsage::C, text('c'), Some(text('C')), None),
        entry(KeyboardUsage::D, text('d'), Some(text('D')), None),
        entry(KeyboardUsage::E, text('e'), Some(text('E')), None),
        entry(KeyboardUsage::F, text('f'), Some(text('F')), None),
        entry(KeyboardUsage::G, text('g'), Some(text('G')), None),
        entry(KeyboardUsage::H, text('h'), Some(text('H')), None),
        entry(KeyboardUsage::I, text('i'), Some(text('I')), None),
        entry(KeyboardUsage::J, text('j'), Some(text('J')), None),
        entry(KeyboardUsage::K, text('k'), Some(text('K')), None),
        entry(KeyboardUsage::L, text('l'), Some(text('L')), None),
        entry(KeyboardUsage::M, text('m'), Some(text('M')), None),
        entry(KeyboardUsage::N, text('n'), Some(text('N')), None),
        entry(KeyboardUsage::O, text('o'), Some(text('O')), None),
        entry(KeyboardUsage::P, text('p'), Some(text('P')), None),
        entry(KeyboardUsage::Q, text('q'), Some(text('Q')), None),
        entry(KeyboardUsage::R, text('r'), Some(text('R')), None),
        entry(KeyboardUsage::S, text('s'), Some(text('S')), None),
        entry(KeyboardUsage::T, text('t'), Some(text('T')), None),
        entry(KeyboardUsage::U, text('u'), Some(text('U')), None),
        entry(KeyboardUsage::V, text('v'), Some(text('V')), None),
        entry(KeyboardUsage::W, text('w'), Some(text('W')), None),
        entry(KeyboardUsage::X, text('x'), Some(text('X')), None),
        entry(KeyboardUsage::Y, text('y'), Some(text('Y')), None),
        entry(KeyboardUsage::Z, text('z'), Some(text('Z')), None),
        entry(KeyboardUsage::DIGIT_1, text('1'), Some(text('!')), None),
        entry(KeyboardUsage::DIGIT_2, text('2'), Some(text('@')), None),
        entry(KeyboardUsage::DIGIT_3, text('3'), Some(text('#')), None),
        entry(KeyboardUsage::DIGIT_4, text('4'), Some(text('$')), None),
        entry(KeyboardUsage::DIGIT_5, text('5'), Some(text('%')), None),
        entry(KeyboardUsage::DIGIT_6, text('6'), Some(text('^')), None),
        entry(KeyboardUsage::DIGIT_7, text('7'), Some(text('&')), None),
        entry(KeyboardUsage::DIGIT_8, text('8'), Some(text('*')), None),
        entry(KeyboardUsage::DIGIT_9, text('9'), Some(text('(')), None),
        entry(KeyboardUsage::DIGIT_0, text('0'), Some(text(')')), None),
        entry(KeyboardUsage::SPACE, text(' '), Some(text(' ')), None),
        entry(KeyboardUsage::MINUS, text('-'), Some(text('_')), None),
        entry(KeyboardUsage::EQUAL, text('='), Some(text('+')), None),
        entry(KeyboardUsage::LEFT_BRACKET, text('['), Some(text('{')), None),
        entry(KeyboardUsage::RIGHT_BRACKET, text(']'), Some(text('}')), None),
        entry(KeyboardUsage::BACKSLASH, text('\\'), Some(text('|')), None),
        entry(KeyboardUsage::SEMICOLON, text(';'), Some(text(':')), None),
        entry(KeyboardUsage::APOSTROPHE, text('\''), Some(text('"')), None),
        entry(KeyboardUsage::GRAVE, text('`'), Some(text('~')), None),
        entry(KeyboardUsage::COMMA, text(','), Some(text('<')), None),
        entry(KeyboardUsage::DOT, text('.'), Some(text('>')), None),
        entry(KeyboardUsage::SLASH, text('/'), Some(text('?')), None),
        entry(KeyboardUsage::ENTER, action(KeyAction::Enter), Some(action(KeyAction::Enter)), None),
        entry(
            KeyboardUsage::ESCAPE,
            action(KeyAction::Escape),
            Some(action(KeyAction::Escape)),
            None,
        ),
        entry(
            KeyboardUsage::BACKSPACE,
            action(KeyAction::Backspace),
            Some(action(KeyAction::Backspace)),
            None,
        ),
        entry(KeyboardUsage::TAB, action(KeyAction::Tab), Some(action(KeyAction::Tab)), None),
    ];
}

pub mod de {
    use hid::KeyboardUsage;

    use super::{action, entry, text, MappingEntry};
    use crate::KeyAction;

    pub const TABLE: &[MappingEntry] = &[
        entry(KeyboardUsage::A, text('a'), Some(text('A')), None),
        entry(KeyboardUsage::B, text('b'), Some(text('B')), None),
        entry(KeyboardUsage::C, text('c'), Some(text('C')), None),
        entry(KeyboardUsage::D, text('d'), Some(text('D')), None),
        entry(KeyboardUsage::E, text('e'), Some(text('E')), Some(text('€'))),
        entry(KeyboardUsage::F, text('f'), Some(text('F')), None),
        entry(KeyboardUsage::G, text('g'), Some(text('G')), None),
        entry(KeyboardUsage::H, text('h'), Some(text('H')), None),
        entry(KeyboardUsage::I, text('i'), Some(text('I')), None),
        entry(KeyboardUsage::J, text('j'), Some(text('J')), None),
        entry(KeyboardUsage::K, text('k'), Some(text('K')), None),
        entry(KeyboardUsage::L, text('l'), Some(text('L')), None),
        entry(KeyboardUsage::M, text('m'), Some(text('M')), None),
        entry(KeyboardUsage::N, text('n'), Some(text('N')), None),
        entry(KeyboardUsage::O, text('o'), Some(text('O')), None),
        entry(KeyboardUsage::P, text('p'), Some(text('P')), None),
        entry(KeyboardUsage::Q, text('q'), Some(text('Q')), Some(text('@'))),
        entry(KeyboardUsage::R, text('r'), Some(text('R')), None),
        entry(KeyboardUsage::S, text('s'), Some(text('S')), None),
        entry(KeyboardUsage::T, text('t'), Some(text('T')), None),
        entry(KeyboardUsage::U, text('u'), Some(text('U')), None),
        entry(KeyboardUsage::V, text('v'), Some(text('V')), None),
        entry(KeyboardUsage::W, text('w'), Some(text('W')), None),
        entry(KeyboardUsage::X, text('x'), Some(text('X')), None),
        entry(KeyboardUsage::Y, text('z'), Some(text('Z')), None),
        entry(KeyboardUsage::Z, text('y'), Some(text('Y')), None),
        entry(KeyboardUsage::DIGIT_1, text('1'), Some(text('!')), None),
        entry(KeyboardUsage::DIGIT_2, text('2'), Some(text('"')), None),
        entry(KeyboardUsage::DIGIT_3, text('3'), Some(text('§')), None),
        entry(KeyboardUsage::DIGIT_4, text('4'), Some(text('$')), None),
        entry(KeyboardUsage::DIGIT_5, text('5'), Some(text('%')), None),
        entry(KeyboardUsage::DIGIT_6, text('6'), Some(text('&')), None),
        entry(KeyboardUsage::DIGIT_7, text('7'), Some(text('/')), Some(text('{'))),
        entry(KeyboardUsage::DIGIT_8, text('8'), Some(text('(')), Some(text('['))),
        entry(KeyboardUsage::DIGIT_9, text('9'), Some(text(')')), Some(text(']'))),
        entry(KeyboardUsage::DIGIT_0, text('0'), Some(text('=')), Some(text('}'))),
        entry(KeyboardUsage::SPACE, text(' '), Some(text(' ')), None),
        entry(KeyboardUsage::MINUS, text('ß'), Some(text('?')), Some(text('\\'))),
        entry(KeyboardUsage::EQUAL, text('´'), Some(text('`')), None),
        entry(KeyboardUsage::LEFT_BRACKET, text('ü'), Some(text('Ü')), None),
        entry(KeyboardUsage::RIGHT_BRACKET, text('+'), Some(text('*')), Some(text('~'))),
        entry(KeyboardUsage::BACKSLASH, text('#'), Some(text('\'')), None),
        entry(KeyboardUsage::SEMICOLON, text('ö'), Some(text('Ö')), None),
        entry(KeyboardUsage::APOSTROPHE, text('ä'), Some(text('Ä')), None),
        entry(KeyboardUsage::GRAVE, text('^'), Some(text('°')), None),
        entry(KeyboardUsage::COMMA, text(','), Some(text(';')), None),
        entry(KeyboardUsage::DOT, text('.'), Some(text(':')), None),
        entry(KeyboardUsage::SLASH, text('-'), Some(text('_')), None),
        entry(KeyboardUsage::ENTER, action(KeyAction::Enter), Some(action(KeyAction::Enter)), None),
        entry(
            KeyboardUsage::ESCAPE,
            action(KeyAction::Escape),
            Some(action(KeyAction::Escape)),
            None,
        ),
        entry(
            KeyboardUsage::BACKSPACE,
            action(KeyAction::Backspace),
            Some(action(KeyAction::Backspace)),
            None,
        ),
        entry(KeyboardUsage::TAB, action(KeyAction::Tab), Some(action(KeyAction::Tab)), None),
    ];
}

pub mod jp {
    use hid::KeyboardUsage;

    use super::{action, entry, text, us, MappingEntry};
    use crate::KeyAction;

    pub const TABLE: &[MappingEntry] = &[
        entry(KeyboardUsage::DIGIT_2, text('2'), Some(text('"')), None),
        entry(KeyboardUsage::DIGIT_6, text('6'), Some(text('&')), None),
        entry(KeyboardUsage::DIGIT_7, text('7'), Some(text('\'')), None),
        entry(KeyboardUsage::LEFT_BRACKET, text('@'), Some(text('`')), None),
        entry(KeyboardUsage::RIGHT_BRACKET, text('['), Some(text('{')), None),
        entry(KeyboardUsage::BACKSLASH, text(']'), Some(text('}')), None),
        entry(KeyboardUsage::APOSTROPHE, text(':'), Some(text('*')), None),
        entry(KeyboardUsage::GRAVE, text('^'), Some(text('~')), None),
        entry(KeyboardUsage::ENTER, action(KeyAction::Enter), Some(action(KeyAction::Enter)), None),
        entry(
            KeyboardUsage::ESCAPE,
            action(KeyAction::Escape),
            Some(action(KeyAction::Escape)),
            None,
        ),
        entry(
            KeyboardUsage::BACKSPACE,
            action(KeyAction::Backspace),
            Some(action(KeyAction::Backspace)),
            None,
        ),
        entry(KeyboardUsage::TAB, action(KeyAction::Tab), Some(action(KeyAction::Tab)), None),
    ];

    pub fn merged() -> Vec<MappingEntry> {
        let mut out = us::TABLE.to_vec();
        for replacement in TABLE {
            if let Some(slot) = out.iter_mut().find(|entry| entry.usage == replacement.usage) {
                *slot = *replacement;
            } else {
                out.push(*replacement);
            }
        }
        out
    }
}

pub mod kr {
    use hid::KeyboardUsage;

    use super::{entry, text, us, MappingEntry};

    pub const TABLE: &[MappingEntry] =
        &[entry(KeyboardUsage::BACKSLASH, text('₩'), Some(text('|')), None)];

    pub fn merged() -> Vec<MappingEntry> {
        us::TABLE
            .iter()
            .copied()
            .map(|entry_candidate| {
                TABLE
                    .iter()
                    .find(|replacement| replacement.usage == entry_candidate.usage)
                    .copied()
                    .unwrap_or(entry_candidate)
            })
            .chain(TABLE.iter().copied().filter(|replacement| {
                !us::TABLE.iter().any(|entry_candidate| entry_candidate.usage == replacement.usage)
            }))
            .collect()
    }
}

pub mod zh {
    use hid::KeyboardUsage;

    use super::{entry, text, us, MappingEntry};

    pub const TABLE: &[MappingEntry] =
        &[entry(KeyboardUsage::NON_US_HASH, text('￥'), Some(text('|')), None)];

    pub fn merged() -> Vec<MappingEntry> {
        us::TABLE
            .iter()
            .copied()
            .map(|entry_candidate| {
                TABLE
                    .iter()
                    .find(|replacement| replacement.usage == entry_candidate.usage)
                    .copied()
                    .unwrap_or(entry_candidate)
            })
            .chain(TABLE.iter().copied().filter(|replacement| {
                !us::TABLE.iter().any(|entry_candidate| entry_candidate.usage == replacement.usage)
            }))
            .collect()
    }
}
