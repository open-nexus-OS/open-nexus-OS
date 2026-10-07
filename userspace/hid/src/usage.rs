// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: USB-HID usage and axis/button constants used by the host input core.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: No direct tests (covered by the integration tests in `tests/input_v1_0_host/tests/hid_contract.rs`).
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyboardUsage(u8);

impl KeyboardUsage {
    pub const A: Self = Self(0x04);
    pub const B: Self = Self(0x05);
    pub const C: Self = Self(0x06);
    pub const D: Self = Self(0x07);
    pub const E: Self = Self(0x08);
    pub const F: Self = Self(0x09);
    pub const G: Self = Self(0x0a);
    pub const H: Self = Self(0x0b);
    pub const I: Self = Self(0x0c);
    pub const J: Self = Self(0x0d);
    pub const K: Self = Self(0x0e);
    pub const L: Self = Self(0x0f);
    pub const M: Self = Self(0x10);
    pub const N: Self = Self(0x11);
    pub const O: Self = Self(0x12);
    pub const P: Self = Self(0x13);
    pub const Q: Self = Self(0x14);
    pub const R: Self = Self(0x15);
    pub const S: Self = Self(0x16);
    pub const T: Self = Self(0x17);
    pub const U: Self = Self(0x18);
    pub const V: Self = Self(0x19);
    pub const W: Self = Self(0x1a);
    pub const X: Self = Self(0x1b);
    pub const Y: Self = Self(0x1c);
    pub const Z: Self = Self(0x1d);
    pub const DIGIT_1: Self = Self(0x1e);
    pub const DIGIT_2: Self = Self(0x1f);
    pub const DIGIT_3: Self = Self(0x20);
    pub const DIGIT_4: Self = Self(0x21);
    pub const DIGIT_5: Self = Self(0x22);
    pub const DIGIT_6: Self = Self(0x23);
    pub const DIGIT_7: Self = Self(0x24);
    pub const DIGIT_8: Self = Self(0x25);
    pub const DIGIT_9: Self = Self(0x26);
    pub const DIGIT_0: Self = Self(0x27);
    pub const ENTER: Self = Self(0x28);
    pub const ESCAPE: Self = Self(0x29);
    pub const BACKSPACE: Self = Self(0x2a);
    pub const TAB: Self = Self(0x2b);
    pub const SPACE: Self = Self(0x2c);
    pub const MINUS: Self = Self(0x2d);
    pub const EQUAL: Self = Self(0x2e);
    pub const LEFT_BRACKET: Self = Self(0x2f);
    pub const RIGHT_BRACKET: Self = Self(0x30);
    pub const BACKSLASH: Self = Self(0x31);
    pub const NON_US_HASH: Self = Self(0x32);
    pub const SEMICOLON: Self = Self(0x33);
    pub const APOSTROPHE: Self = Self(0x34);
    pub const GRAVE: Self = Self(0x35);
    pub const COMMA: Self = Self(0x36);
    pub const DOT: Self = Self(0x37);
    pub const SLASH: Self = Self(0x38);
    pub const CAPS_LOCK: Self = Self(0x39);
    pub const F1: Self = Self(0x3a);
    pub const HOME: Self = Self(0x4a);
    pub const DELETE_FORWARD: Self = Self(0x4c);
    pub const END: Self = Self(0x4d);
    pub const RIGHT_ARROW: Self = Self(0x4f);
    pub const LEFT_ARROW: Self = Self(0x50);
    pub const DOWN_ARROW: Self = Self(0x51);
    pub const UP_ARROW: Self = Self(0x52);
    pub const LEFT_CTRL: Self = Self(0xe0);
    pub const LEFT_SHIFT: Self = Self(0xe1);
    pub const LEFT_ALT: Self = Self(0xe2);
    pub const LEFT_GUI: Self = Self(0xe3);
    pub const RIGHT_CTRL: Self = Self(0xe4);
    pub const RIGHT_SHIFT: Self = Self(0xe5);
    pub const RIGHT_ALT: Self = Self(0xe6);
    pub const RIGHT_GUI: Self = Self(0xe7);

    #[must_use]
    pub const fn from_raw(raw: u8) -> Self {
        Self(raw)
    }

    #[must_use]
    pub const fn raw(self) -> u8 {
        self.0
    }

    #[must_use]
    pub const fn event_code(self) -> u16 {
        self.0 as u16
    }

    #[must_use]
    pub const fn modifier_from_bit(bit: u8) -> Self {
        Self(0xe0 + bit)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelativeAxis {
    X,
    Y,
    Wheel,
}

impl RelativeAxis {
    #[must_use]
    pub const fn event_code(self) -> u16 {
        match self {
            Self::X => 0,
            Self::Y => 1,
            Self::Wheel => 8,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbsoluteAxis {
    X,
    Y,
}

impl AbsoluteAxis {
    #[must_use]
    pub const fn event_code(self) -> u16 {
        match self {
            Self::X => 0,
            Self::Y => 1,
        }
    }
}

/// The eight buttons of a boot mouse report's first byte, bit 0 first. Side and extra (bits
/// 3 and 4) are the thumb buttons a measured receiver sends (2026-10-04,
/// `docs/board/measurements/2026-10-04-usb-boot-protocol/`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Side,
    Extra,
    Forward,
    Back,
    Task,
}

impl MouseButton {
    /// Bit order of the boot report's button byte.
    pub const ALL: [Self; 8] = [
        Self::Left,
        Self::Right,
        Self::Middle,
        Self::Side,
        Self::Extra,
        Self::Forward,
        Self::Back,
        Self::Task,
    ];

    #[must_use]
    pub const fn event_code(self) -> u16 {
        0x110 + self.bit() as u16
    }

    #[must_use]
    pub const fn mask(self) -> u8 {
        1 << self.bit()
    }

    const fn bit(self) -> u8 {
        match self {
            Self::Left => 0,
            Self::Right => 1,
            Self::Middle => 2,
            Self::Side => 3,
            Self::Extra => 4,
            Self::Forward => 5,
            Self::Back => 6,
            Self::Task => 7,
        }
    }
}
