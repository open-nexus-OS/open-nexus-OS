// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The modifier state inputd keeps from the key reports: the wire bits a tap carries
//! (TASK-0346 P2), the exact set the shortcut registry matches (RFC-0113 §7: the window and
//! capture chords become FACTS on the visible state's push, never keys for imed or an app), the
//! subset a layout resolves characters with. Split out of `service.rs` under the module-size
//! ratchet (TASK-0068).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `tests/key_facts.rs` (through `InputdService`)

use hid::KeyboardUsage;
use keymaps::Modifiers;

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ModifierState {
    shift: bool,
    control: bool,
    alt_gr: bool,
    /// Left Alt.
    alt: bool,
    /// Super / GUI.
    gui: bool,
}

impl ModifierState {
    pub(crate) fn apply_key(&mut self, usage: KeyboardUsage, pressed: bool) {
        match usage.raw() {
            0xe0 | 0xe4 => self.control = pressed,
            0xe1 | 0xe5 => self.shift = pressed,
            0xe2 => self.alt = pressed,
            0xe3 | 0xe7 => self.gui = pressed,
            0xe6 => self.alt_gr = pressed,
            _ => {}
        }
    }

    /// The modifiers held as the surface wire's bits (TASK-0346 P2: a click's Shift selects).
    #[must_use]
    pub(crate) fn bits(self) -> u8 {
        use nexus_display_proto::surface_input as wire;
        let mut bits = 0;
        for (held, bit) in [
            (self.shift, wire::MOD_SHIFT),
            (self.control, wire::MOD_CTRL),
            (self.alt, wire::MOD_ALT),
            (self.alt_gr, wire::MOD_ALTGR),
            (self.gui, wire::MOD_SUPER),
        ] {
            if held {
                bits |= bit;
            }
        }
        bits
    }

    /// The exact set the shortcut registry matches (its bits are the wire's — `tests/key_facts.rs`).
    #[must_use]
    pub(crate) fn chord_mods(self) -> shortcuts::Mods {
        shortcuts::Mods::from_bits(self.bits())
    }

    /// The set the keymap resolves with — all of it, so its registry chords match exactly.
    #[must_use]
    pub(crate) fn snapshot(self) -> Modifiers {
        let mut modifiers = Modifiers::default();
        for (held, with) in [
            (self.shift, Modifiers::with_shift as fn(Modifiers) -> Modifiers),
            (self.control, Modifiers::with_control),
            (self.alt_gr, Modifiers::with_alt_gr),
            (self.alt, Modifiers::with_alt),
            (self.gui, Modifiers::with_super),
        ] {
            if held {
                modifiers = with(modifiers);
            }
        }
        modifiers
    }
}

pub(crate) fn is_modifier(usage: KeyboardUsage) -> bool {
    matches!(usage.raw(), 0xe0..=0xe7)
}
