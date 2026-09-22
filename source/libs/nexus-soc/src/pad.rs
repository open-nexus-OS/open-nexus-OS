// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! One pad's 32-bit configuration register (`pad * 4` inside the pinctrl
//! window): the mux function in bits 0..2, pull in 13..15 (enable, up, down),
//! strong pull bit 3, drive strength field 10..12, schmitt 8..9, slew enable
//! bit 7 — as the pad controller documentation states them. The mA → field
//! mapping of `drive-strength` is TASK-0245B P3's (measured on the stock pads);
//! until then a group carries the raw field.

use crate::field::Field;

pub const PAD_MUX: Field = Field::new(0, 3);
pub const PAD_STRONG_PULL: u32 = 1 << 3;
pub const PAD_SLEW_RATE_EN: u32 = 1 << 7;
pub const PAD_SCHMITT: Field = Field::new(8, 2);
pub const PAD_DRIVE: Field = Field::new(10, 3);
pub const PAD_PULLDOWN: u32 = 1 << 13;
pub const PAD_PULLUP: u32 = 1 << 14;
pub const PAD_PULL_EN: u32 = 1 << 15;

/// What a pad group asks for besides the function.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PadConf {
    pub pull_up: bool,
    pub pull_down: bool,
    /// Raw drive-strength field (0..7).
    pub drive: u8,
    pub schmitt: u8,
}

/// The register word for `func` + `conf` (everything else zero).
pub fn pad_word(func: u32, conf: PadConf) -> u32 {
    let mut w = PAD_MUX.set(0, func);
    if conf.pull_up {
        w |= PAD_PULL_EN | PAD_PULLUP;
    }
    if conf.pull_down {
        w |= PAD_PULL_EN | PAD_PULLDOWN;
    }
    w = PAD_DRIVE.set(w, u32::from(conf.drive));
    PAD_SCHMITT.set(w, u32::from(conf.schmitt))
}

/// Split a `pinmux` cell (`(pad << 16) | func`) as the pad groups encode it.
pub const fn split_pinmux(cell: u32) -> (u32, u32) {
    (cell >> 16, cell & 0xffff)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pulled_up_sd_data_pad_encodes_as_documented() {
        let w = pad_word(0, PadConf { pull_up: true, drive: 3, ..PadConf::default() });
        assert_eq!(w, PAD_PULL_EN | PAD_PULLUP | (3 << 10));
        assert_eq!(split_pinmux(104 << 16), (104, 0));
        assert_eq!(split_pinmux((68 << 16) | 2), (68, 2));
    }
}
