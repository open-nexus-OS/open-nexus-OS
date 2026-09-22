// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A bit field inside a 32-bit register: `shift` and `width` as the binding
//! documentation states them.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Field {
    pub shift: u8,
    pub width: u8,
}

impl Field {
    pub const fn new(shift: u8, width: u8) -> Self {
        Field { shift, width }
    }

    pub const fn mask(self) -> u32 {
        if self.width >= 32 {
            u32::MAX
        } else {
            ((1u32 << self.width) - 1) << self.shift
        }
    }

    pub const fn get(self, word: u32) -> u32 {
        (word & self.mask()) >> self.shift
    }

    /// `word` with the field replaced by `value` (truncated to the width).
    pub const fn set(self, word: u32, value: u32) -> u32 {
        (word & !self.mask()) | ((value << self.shift) & self.mask())
    }
}

#[cfg(test)]
mod tests {
    use super::Field;

    #[test]
    fn fields_mask_get_and_set() {
        let f = Field::new(5, 3);
        assert_eq!(f.mask(), 0b1110_0000);
        assert_eq!(f.get(0x52), 2, "the eMMC mux on the stock system");
        assert_eq!(f.set(0x52, 3), 0x72);
        assert_eq!(f.set(0x52, 9), 0x32, "truncated to the width");
    }
}
