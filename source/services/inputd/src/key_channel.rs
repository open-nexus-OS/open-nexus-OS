// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: What inputd's keys become on the key channel (RFC-0113 §2/§3, ADR-0090): the input
//! source decides the route. A LAYOUT source (us, de) is translated here — the keymap resolved
//! the key, the dead-key composer (ime-core, the machine imed ran before) joins an accent with
//! the next letter — and its text or editing action goes to windowd for the field it focused. A
//! COMPOSITION source's key (jp, kr, zh) goes as resolved, marked for the input method windowd
//! feeds behind its focus. Every entry keeps its key's sequence and first-seen time (`seq`,
//! `t_ns`). Pure and fixed-size: host-tested, nothing allocated per key.
//! OWNERS: @runtime @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below (dead keys, composition marking, actions, repeats, layout
//!   switch)

use ime_core::{Composer, ImeAction, ImeKey};
use input_live_protocol::{
    KeyInput, KEY_FLAG_REPEAT, KEY_INPUT_ACTION, KEY_INPUT_COMPOSE, KEY_INPUT_TEXT,
};
use keymaps::{KeyAction, KeyOutput, LayoutId};
use nexus_wire::imed as ime_wire;

/// How a layout's keys travel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeySource {
    /// Translated in inputd; text and actions go to the focused field.
    Layout,
    /// Composed by the input method behind windowd's focus.
    Composition,
}

impl KeySource {
    /// The route of `layout`'s keys.
    #[must_use]
    pub const fn of(layout: LayoutId) -> Self {
        match layout {
            LayoutId::Us | LayoutId::De => Self::Layout,
            LayoutId::Jp | LayoutId::Kr | LayoutId::Zh => Self::Composition,
        }
    }
}

/// One resolved key as the channel takes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelKey {
    pub output: KeyOutput,
    /// An auto-repeat of a held key.
    pub repeated: bool,
    /// The key event's device stream sequence.
    pub seq: u32,
    /// When the system first saw it (monotonic ns).
    pub t_ns: u64,
}

impl ChannelKey {
    fn entry(&self, kind: u8, sub: u8, value: u32) -> KeyInput {
        KeyInput {
            kind,
            sub,
            flags: if self.repeated { KEY_FLAG_REPEAT } else { 0 },
            modifiers: 0,
            seq: self.seq,
            t_ns: self.t_ns,
            value,
        }
    }
}

/// The key channel's translation state: a layout source's pending accent.
#[derive(Debug, Clone, Copy, Default)]
pub struct KeyTranslator {
    composer: Composer,
}

impl KeyTranslator {
    #[must_use]
    pub const fn new() -> Self {
        Self { composer: Composer::new() }
    }

    /// The layout changed: an accent armed under the old one is dropped with it.
    pub fn reset(&mut self) {
        self.composer.reset();
    }

    /// One key: its entries (none, one or two) into `emit`, in order.
    pub fn feed(&mut self, source: KeySource, key: ChannelKey, emit: &mut dyn FnMut(KeyInput)) {
        // Layout switching is inputd's own concern, never a field's.
        if key.output == KeyOutput::Action(KeyAction::ImeSwitch) {
            return;
        }
        match source {
            KeySource::Layout => self.feed_layout(key, emit),
            KeySource::Composition => {
                let (sub, value) = match key.output {
                    KeyOutput::Text(ch) => (ime_wire::KEY_KIND_TEXT, u32::from(ch)),
                    KeyOutput::Dead(ch) => (ime_wire::KEY_KIND_DEAD, u32::from(ch)),
                    KeyOutput::Action(action) => match action_code(action) {
                        Some(code) => (ime_wire::KEY_KIND_ACTION, u32::from(code)),
                        None => return,
                    },
                };
                emit(key.entry(KEY_INPUT_COMPOSE, sub, value));
            }
        }
    }

    /// A layout source's key through the dead-key composer (RFC-0075 semantics: an accent joins
    /// the next letter, a miss commits both, Escape/Backspace cancel it, another action commits
    /// it and passes). Editing commands pass composition untouched, as they did in imed.
    fn feed_layout(&mut self, key: ChannelKey, emit: &mut dyn FnMut(KeyInput)) {
        if let Some(ime_key) = ImeKey::from_key_output(key.output) {
            let outcome = self.composer.feed(ime_key);
            if outcome.handled {
                for ch in outcome.commit.chars() {
                    emit(key.entry(KEY_INPUT_TEXT, 0, u32::from(ch)));
                }
                if let Some(action) = outcome.pass_action {
                    emit(key.entry(KEY_INPUT_ACTION, 0, u32::from(ime_action_code(action))));
                }
                return;
            }
        }
        match key.output {
            KeyOutput::Text(ch) => emit(key.entry(KEY_INPUT_TEXT, 0, u32::from(ch))),
            // The composer takes every dead key (it arms or commits): never unhandled.
            KeyOutput::Dead(_) => {}
            KeyOutput::Action(action) => {
                if let Some(code) = action_code(action) {
                    emit(key.entry(KEY_INPUT_ACTION, 0, u32::from(code)));
                }
            }
        }
    }
}

/// The input method wire's `ACTION_*` code of a key action (`None`: the layout switch).
fn action_code(action: KeyAction) -> Option<u8> {
    Some(match action {
        KeyAction::Enter => ime_wire::ACTION_ENTER,
        KeyAction::Escape => ime_wire::ACTION_ESCAPE,
        KeyAction::Backspace => ime_wire::ACTION_BACKSPACE,
        KeyAction::Tab => ime_wire::ACTION_TAB,
        KeyAction::BackTab => ime_wire::ACTION_BACKTAB,
        KeyAction::Edit(key) => crate::edit_keys::action_code(key),
        KeyAction::ImeSwitch => return None,
    })
}

const fn ime_action_code(action: ImeAction) -> u8 {
    match action {
        ImeAction::Enter => ime_wire::ACTION_ENTER,
        ImeAction::Escape => ime_wire::ACTION_ESCAPE,
        ImeAction::Backspace => ime_wire::ACTION_BACKSPACE,
        ImeAction::Tab => ime_wire::ACTION_TAB,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use keymaps::EditKey;

    fn key(seq: u32, output: KeyOutput) -> ChannelKey {
        ChannelKey { output, repeated: false, seq, t_ns: 1_000 * u64::from(seq) }
    }

    fn feed(t: &mut KeyTranslator, source: KeySource, keys: &[ChannelKey]) -> Vec<KeyInput> {
        let mut out = Vec::new();
        for k in keys {
            t.feed(source, *k, &mut |entry| out.push(entry));
        }
        out
    }

    fn texts(entries: &[KeyInput]) -> String {
        entries
            .iter()
            .filter(|e| e.kind == KEY_INPUT_TEXT)
            .filter_map(|e| char::from_u32(e.value))
            .collect()
    }

    #[test]
    fn a_layout_source_types_text_stamped_with_its_keys() {
        let mut t = KeyTranslator::new();
        let out = feed(
            &mut t,
            KeySource::Layout,
            &[key(1, KeyOutput::Text('h')), key(2, KeyOutput::Text('ü'))],
        );
        assert_eq!(texts(&out), "hü");
        assert_eq!((out[0].seq, out[0].t_ns, out[1].seq), (1, 1_000, 2));
        assert!(out.iter().all(KeyInput::is_valid));
    }

    #[test]
    fn a_dead_key_joins_the_next_letter_or_falls_back_to_both() {
        let mut t = KeyTranslator::new();
        let out = feed(
            &mut t,
            KeySource::Layout,
            &[
                key(1, KeyOutput::Dead('´')),
                key(2, KeyOutput::Text('e')),
                key(3, KeyOutput::Dead('^')),
                key(4, KeyOutput::Text('x')),
            ],
        );
        assert_eq!(texts(&out), "é^x");
        assert_eq!(out[0].seq, 2, "the composed letter carries the key that completed it");
    }

    #[test]
    fn escape_cancels_an_accent_and_enter_commits_it_then_passes() {
        let mut t = KeyTranslator::new();
        let cancelled = feed(
            &mut t,
            KeySource::Layout,
            &[key(1, KeyOutput::Dead('`')), key(2, KeyOutput::Action(KeyAction::Escape))],
        );
        assert!(cancelled.is_empty(), "the Escape was consumed by the accent");
        let passed = feed(
            &mut t,
            KeySource::Layout,
            &[key(3, KeyOutput::Dead('`')), key(4, KeyOutput::Action(KeyAction::Enter))],
        );
        assert_eq!(texts(&passed), "`");
        assert_eq!(passed[1].kind, KEY_INPUT_ACTION);
        assert_eq!(passed[1].value, u32::from(ime_wire::ACTION_ENTER));
    }

    #[test]
    fn editing_commands_are_actions_and_the_layout_switch_is_nobodys() {
        let mut t = KeyTranslator::new();
        let out = feed(
            &mut t,
            KeySource::Layout,
            &[
                key(1, KeyOutput::Action(KeyAction::Edit(EditKey::SelectLeft))),
                key(2, KeyOutput::Action(KeyAction::ImeSwitch)),
                key(3, KeyOutput::Action(KeyAction::Backspace)),
                key(4, KeyOutput::Action(KeyAction::BackTab)),
            ],
        );
        let codes: Vec<(u8, u32)> = out.iter().map(|e| (e.kind, e.value)).collect();
        assert_eq!(
            codes,
            [
                (KEY_INPUT_ACTION, u32::from(ime_wire::ACTION_SELECT_LEFT)),
                (KEY_INPUT_ACTION, u32::from(ime_wire::ACTION_BACKSPACE)),
                (KEY_INPUT_ACTION, u32::from(ime_wire::ACTION_BACKTAB)),
            ]
        );
    }

    #[test]
    fn a_composition_source_marks_every_key_for_the_input_method() {
        let mut t = KeyTranslator::new();
        let out = feed(
            &mut t,
            KeySource::of(LayoutId::Jp),
            &[
                key(1, KeyOutput::Text('k')),
                key(2, KeyOutput::Dead('´')),
                key(3, KeyOutput::Action(KeyAction::Enter)),
            ],
        );
        let marked: Vec<(u8, u8)> = out.iter().map(|e| (e.kind, e.sub)).collect();
        assert_eq!(
            marked,
            [
                (KEY_INPUT_COMPOSE, ime_wire::KEY_KIND_TEXT),
                (KEY_INPUT_COMPOSE, ime_wire::KEY_KIND_DEAD),
                (KEY_INPUT_COMPOSE, ime_wire::KEY_KIND_ACTION),
            ]
        );
        assert_eq!(KeySource::of(LayoutId::De), KeySource::Layout);
    }

    #[test]
    fn a_repeat_is_flagged_and_a_layout_switch_drops_the_accent() {
        let mut t = KeyTranslator::new();
        let held = ChannelKey { repeated: true, ..key(5, KeyOutput::Text('a')) };
        let out = feed(&mut t, KeySource::Layout, &[held]);
        assert_eq!(out[0].flags, KEY_FLAG_REPEAT);
        feed(&mut t, KeySource::Layout, &[key(6, KeyOutput::Dead('´'))]);
        t.reset();
        let after = feed(&mut t, KeySource::Layout, &[key(7, KeyOutput::Text('e'))]);
        assert_eq!(texts(&after), "e", "the old layout's accent is gone");
    }
}
