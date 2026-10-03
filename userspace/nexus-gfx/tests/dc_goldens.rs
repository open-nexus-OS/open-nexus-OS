// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: End-to-end golden of the `dc` host half (TASK-0250 P1–P2): the reference monitor's
//! archived EDID → `pick_mode` under the SoC's maximum → the bring-up sequence for the stock
//! system's plane → the words the live register dump holds at 1920x1080@60. If this test
//! moves, either the measurement changed or the model did — never silently.
//! OWNERS: @gpu
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: this file

use nexus_gfx::backend::dc::{bring_up, parse_edid, pick_mode, Plane, Sequence};

const EDID: &[u8] = include_bytes!(
    "../../../docs/board/measurements/2026-09-29-display-regs/edid-hdmi-2026-09-29.bin"
);

#[test]
fn edid_to_mode_to_the_measured_registers() {
    let edid = parse_edid(EDID).expect("the archived EDID parses");
    let mode = pick_mode(edid.modes.as_slice(), (1920, 1080), edid.screen_cm)
        .expect("the SoC's maximum fits the monitor");
    assert_eq!((mode.h_active, mode.v_active), (1920, 1080));
    assert_eq!(mode.refresh_mhz(), 60_000);
    assert!(mode.detailed, "the monitor's own timing, not the standard table");

    let plane = Plane { bus_addr: 0x0fb7_c000, stride: 7680, width: 1920, height: 1080 };
    let mut seq = Sequence::new();
    bring_up(&mut seq, &mode, &plane, 1);

    // The live dump, picture on (docs/board/measurements/2026-09-29-display-regs/
    // dpu-regs-on-annotated.txt): output control 2 timing, RDMA channel 1, composer 2.
    for (offset, value) in [
        (0x18080u32, 0x0094_0058u32),
        (0x18084, 0x0026_0002),
        (0x18088, 0x1005_102c),
        (0x1808c, 0x0438_0780),
        (0xbb8, 0x1e00),
        (0xbbc, 0x0438_0780),
        (0xbc4, 0x0437_077f),
        (0x4c00, 0x0007_8001),
        (0x4c04, 0x438),
        (0x560, 0x0004_0002),
    ] {
        assert_eq!(seq.value_at(offset), Some(value), "offset 0x{offset:x}");
    }
}

/// The encoder's first-light sequence (TASK-0251 P2a) leaves exactly the words the stock
/// system's live dump holds at 1080p60 8 bpc — except the PLL-lock bit, which the hardware sets.
#[test]
fn the_encoder_sequence_leaves_the_measured_words() {
    use nexus_gfx::backend::dc::encoder;
    const DUMP: &[u8] =
        include_bytes!("../../../docs/board/measurements/2026-09-29-display-regs/hdmi-regs-on.bin");
    let word = |off: u32| {
        let i = off as usize;
        u32::from_le_bytes([DUMP[i], DUMP[i + 1], DUMP[i + 2], DUMP[i + 3]])
    };
    let mut seq = Sequence::new();
    for w in encoder::sequence(148_500).expect("measured") {
        nexus_gfx::backend::dc::RegWriter::write(&mut seq, w.offset, w.value);
    }
    for w in [encoder::PHY_ENABLE, encoder::TRANSMITTER_ON] {
        nexus_gfx::backend::dc::RegWriter::write(&mut seq, w.offset, w.value);
    }
    for off in [0x028u32, 0x034, 0x0e0, 0x0e8, 0x0ec, 0x0f0] {
        assert_eq!(seq.value_at(off), Some(word(off)), "encoder word 0x{off:03x}");
    }
    assert_eq!(
        seq.value_at(encoder::PHY_CTRL),
        Some(word(encoder::PHY_CTRL) & !encoder::PHY_CTRL_PLL_LOCKED)
    );
    assert_eq!(
        word(encoder::PHY_CTRL) & encoder::PHY_CTRL_PLL_LOCKED,
        encoder::PHY_CTRL_PLL_LOCKED
    );
    assert_eq!(word(encoder::PHY_STATUS) & encoder::PHY_STATUS_HPD, encoder::PHY_STATUS_HPD);
}
