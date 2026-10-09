// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The modifier state and the key FACTS inputd recognizes instead of delivering — the
//! window-tiling chords (TASK-0066) and the capture keys (RFC-0095). A fact rides the visible
//! state's push to windowd and never reaches imed or an app. Split out of `service.rs` under the
//! module-size ratchet when the capture keys joined the chords (TASK-0068).
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
    /// Left Alt (TASK-0066 chords: the bottom quarters).
    alt: bool,
    /// Super / GUI (TASK-0066 chords: the window-tiling modifier).
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

    #[must_use]
    pub(crate) fn snapshot(self) -> Modifiers {
        let mut modifiers = Modifiers::default();
        if self.shift {
            modifiers = modifiers.with_shift();
        }
        if self.control {
            modifiers = modifiers.with_control();
        }
        if self.alt_gr {
            modifiers = modifiers.with_alt_gr();
        }
        modifiers
    }
}

pub(crate) fn is_modifier(usage: KeyboardUsage) -> bool {
    matches!(usage.raw(), 0xe0..=0xe7)
}

/// The fixed tiling chord table (TASK-0066, v1): Super+Ctrl + ←/→ = halves, ↑ = Fill,
/// ↓ = Return, F = Fill, R = Return; + Shift ←/→ = top quarters; + Alt ←/→ = bottom
/// quarters. Codes are windowd's `zones::CODE_*`. `None` = not a chord.
pub(crate) fn wm_chord_for(usage: KeyboardUsage, mods: &ModifierState) -> Option<u8> {
    if !(mods.gui && mods.control) {
        return None;
    }
    let (left, right) = if mods.shift {
        (5u8, 6u8)
    } else if mods.alt {
        (7, 8)
    } else {
        (1, 2)
    };
    match usage {
        KeyboardUsage::LEFT_ARROW => Some(left),
        KeyboardUsage::RIGHT_ARROW => Some(right),
        KeyboardUsage::UP_ARROW | KeyboardUsage::F => Some(9),
        KeyboardUsage::DOWN_ARROW | KeyboardUsage::R => Some(10),
        _ => None,
    }
}

/// The capture keys (RFC-0095): Print opens the screenshot UI (1), Shift+Print saves the
/// screen (2), Alt+Print the focused window (3). With Ctrl or Super held Print is no capture
/// key (`None`: those chords stay free).
pub(crate) fn capture_key_for(usage: KeyboardUsage, mods: &ModifierState) -> Option<u8> {
    if usage != KeyboardUsage::PRINT_SCREEN || mods.control || mods.gui {
        return None;
    }
    Some(if mods.shift {
        2
    } else if mods.alt {
        3
    } else {
        1
    })
}
