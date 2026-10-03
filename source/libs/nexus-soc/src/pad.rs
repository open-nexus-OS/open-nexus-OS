// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! One pad's 32-bit configuration register inside the pinctrl window (where it lies is the
//! provider table's pad map, `table::k1::pad_offset` — the pads are not laid out in pin order):
//! the mux function in bits 0..2, strong pull bit 3, edge-detect clear bit 6, slew enable bit 7,
//! schmitt 8..9, drive strength 10..12, pull 13..15 (down, up, enable).
//!
//! The pad step (TASK-0245B P3) owns the fields whose meaning the board's own words confirm:
//! the function, the pull and its direction, the strong pull (clear in every stock word read),
//! and edge-detect clear (set in every stock word read — a pad that is not a GPIO interrupt).
//! Drive strength and schmitt stay as the register holds them until their mA tables are
//! measured per IO domain; the bring-up marker shows the whole word before and after.
//! Measured 2026-10-03 (`docs/board/measurements/2026-10-03-first-light/`): the encoder's pads
//! `0xd041` (pull-up) and `0xb041` (pull-down), uart0 `0xd042`, mmc1 `0xc440`.

use nexus_fdt::Node;

use crate::field::Field;

pub const PAD_MUX: Field = Field::new(0, 3);
pub const PAD_STRONG_PULL: u32 = 1 << 3;
pub const PAD_EDGE_CLEAR: u32 = 1 << 6;
pub const PAD_SLEW_RATE_EN: u32 = 1 << 7;
pub const PAD_SCHMITT: Field = Field::new(8, 2);
pub const PAD_DRIVE: Field = Field::new(10, 3);
pub const PAD_PULLDOWN: u32 = 1 << 13;
pub const PAD_PULLUP: u32 = 1 << 14;
pub const PAD_PULL_EN: u32 = 1 << 15;

/// The fields the pad step writes; everything else in the word is left as found.
pub const PAD_OWNED: u32 =
    PAD_MUX.mask() | PAD_STRONG_PULL | PAD_EDGE_CLEAR | PAD_PULLDOWN | PAD_PULLUP | PAD_PULL_EN;

/// A pad group's pull (`bias-pull-up`, `bias-pull-down`, `bias-disable` or none of them).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bias {
    Disabled,
    PullUp,
    PullDown,
}

impl Bias {
    /// The bias a pin group node asks for. An argument to `bias-pull-*` (a strength) is not
    /// read: the stock words set the normal pull everywhere, never the strong one.
    pub fn of(group: Node<'_>) -> Bias {
        if group.prop("bias-pull-up").is_some() {
            Bias::PullUp
        } else if group.prop("bias-pull-down").is_some() {
            Bias::PullDown
        } else {
            Bias::Disabled
        }
    }
}

/// The owned fields' value for function `func` with `bias` (the rest of the word is the
/// register's).
pub const fn pad_bits(func: u32, bias: Bias) -> u32 {
    let pull = match bias {
        Bias::Disabled => 0,
        Bias::PullUp => PAD_PULL_EN | PAD_PULLUP,
        Bias::PullDown => PAD_PULL_EN | PAD_PULLDOWN,
    };
    PAD_MUX.set(0, func) | PAD_EDGE_CLEAR | pull
}

/// Split a `pinmux` cell (`(pin << 16) | func`) as the pad groups encode it.
pub const fn split_pinmux(cell: u32) -> (u32, u32) {
    (cell >> 16, cell & 0xffff)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The owned fields of the stock words read live on 2026-10-03.
    #[test]
    fn the_owned_fields_reproduce_the_stock_words() {
        assert_eq!(pad_bits(1, Bias::PullUp), 0xd041 & PAD_OWNED, "the encoder's DDC pads");
        assert_eq!(pad_bits(1, Bias::PullDown), 0xb041 & PAD_OWNED, "the encoder's status pads");
        assert_eq!(pad_bits(2, Bias::PullUp), 0xd042 & PAD_OWNED, "uart0");
        assert_eq!(pad_bits(0, Bias::PullUp), 0xc440 & PAD_OWNED, "mmc1 data");
        assert_eq!(0xd041 & !PAD_OWNED, 4 << 10, "the drive field is the register's, not ours");
        assert_eq!(split_pinmux(104 << 16), (104, 0));
        assert_eq!(split_pinmux((68 << 16) | 2), (68, 2));
    }
}
