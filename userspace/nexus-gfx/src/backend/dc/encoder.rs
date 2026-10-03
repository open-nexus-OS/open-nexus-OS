// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The board's HDMI encoder (TASK-0251 P2a): its first-light sequence for one
//! MEASURED pixel clock, as data. The order is the boot loader's splash driver's (read as an
//! order and name reference only, never ported): a default PLL configuration, the PHY bias,
//! the colour depth, the pixel clock's PLL words, the PHY reset then enable, the PLL lock, the
//! transmitter's control word. Every final word is the stock system's live dump at
//! 1920x1080@60 8 bpc (`docs/board/measurements/2026-09-29-display-regs/hdmi-regs-on.bin`). A
//! pixel clock without a measured row is refused, never derived here: the PLL's divider rules
//! are a measurement still to make (another mode on the stock system), not a guess.
//! OWNERS: @gpu
//! STATUS: Functional (one measured row: 148.5 MHz)
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below + `tests/dc_goldens.rs` (the sequence's final words equal
//!   the dump's)

use super::model::Write;

/// Length of the encoder block the tree names (`reg = <0xc0400500 0x200>`).
pub const WINDOW_LEN: u32 = 0x200;

/// The PHY status word: hot-plug detect in bit 12 (measured 2026-09-30: an unplug clears
/// bits 12 and 14 and nothing else).
pub const PHY_STATUS: u32 = 0x00c;
pub const PHY_STATUS_HPD: u32 = 1 << 12;
/// The PHY control word: reset (0) then enable (3); the hardware sets bit 16 once the PLL
/// locks (live `0x00010003`).
pub const PHY_CTRL: u32 = 0x0e4;
pub const PHY_CTRL_PLL_LOCKED: u32 = 1 << 16;

/// The pixel clocks with a measured sequence, in kHz.
pub const MEASURED_PIXEL_CLOCKS_KHZ: [u32; 1] = [148_500];

const fn w(offset: u32, value: u32) -> Write {
    Write { offset, value }
}

/// 1920x1080@60 (148.5 MHz) at 8 bits per colour, up to the PHY reset. Then [`PHY_ENABLE`],
/// the PLL lock (`PHY_CTRL_PLL_LOCKED`), and [`TRANSMITTER_ON`].
const SEQ_148_5_MHZ_8BPC: [Write; 9] = [
    // The PLL's default configuration before the PHY is biased (the splash driver's order).
    w(0x0e8, 0x2020_0000),
    w(0x0ec, 0x508d_425a),
    w(0x0f0, 0x0000_0861),
    // PHY bias: current 1, resistor 7, phase 0 — the dump's word.
    w(0x0e0, 0xae5c_010f),
    // Colour depth: 8 bits per component.
    w(0x034, 0x0000_004d),
    // The 148.5 MHz PLL: fraction, integer and dividers, post-dividers (the dump's words).
    w(0x0e8, 0x203f_0000),
    w(0x0ec, 0x509d_453e),
    w(0x0f0, 0x0000_0821),
    // PHY reset; the enable follows as the first write after this slice.
    w(PHY_CTRL, 0),
];

/// The PHY enable: written after [`sequence`], then the PLL lock is awaited.
pub const PHY_ENABLE: Write = w(PHY_CTRL, 3);
/// The transmitter's control word at 8 bits per colour, written once the PLL has locked.
pub const TRANSMITTER_ON: Write = w(0x028, 0x1c20_8000);

/// The encoder's sequence for `pixel_clock_khz` up to the PHY reset, or `None` when no row is
/// measured for it (the caller names the refusal).
pub fn sequence(pixel_clock_khz: u32) -> Option<&'static [Write]> {
    match pixel_clock_khz {
        148_500 => Some(&SEQ_148_5_MHZ_8BPC),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_measured_clock_has_a_sequence_that_ends_in_the_phy_reset() {
        let seq = sequence(148_500).expect("measured");
        assert_eq!(seq.last(), Some(&w(PHY_CTRL, 0)));
        assert_eq!(PHY_ENABLE.value & 0x3, 3);
    }

    #[test]
    fn test_reject_a_pixel_clock_without_a_measured_row() {
        assert_eq!(sequence(74_250), None, "720p60: no row until it is measured");
        assert_eq!(sequence(148_499), None, "no rounding to the nearest row");
        assert_eq!(sequence(0), None);
    }
}
