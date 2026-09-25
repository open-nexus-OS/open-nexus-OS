// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: From power-up to the operating mode.
//! OWNERS: @runtime @drivers

use storage_sdhci::k1::*;
use storage_sdhci::proto::{ext, switch_arg, OCR_ARG};
use storage_sdhci::regs::INT_USED;
use storage_sdhci::{Card, Ceiling, Error, Mode, Stage, Timing};

use crate::model::{self, commands, Event};

const RCA: u32 = 1 << 16;

/// JEDEC's identification: CMD0, CMD1 until ready (the model answers busy three times),
/// CID, address, CSD, select, status, then the EXT_CSD at legacy speed.
fn identification() -> Vec<(u8, u32)> {
    let mut seq = vec![(0, 0)];
    seq.extend([(1, OCR_ARG); 4]);
    seq.extend([(2, 0), (3, RCA), (9, RCA), (7, RCA), (13, RCA), (8, 0)]);
    seq
}

fn switch(at: u8, value: u8) -> (u8, u32) {
    (6, switch_arg(at, value))
}

#[test]
fn a_qemu_default_host_brings_the_card_to_hs52_on_4_bits() {
    let m = model::machine(model::qemu());
    let card = crate::card(&m, Ceiling::Hs400es);
    assert_eq!(card.mode(), Mode::Hs52 { width: 4 });
    assert_eq!(card.sectors(), 0x0080_0000);
    // Spec 2.00: 52 MHz undivided at high speed.
    assert_eq!(
        (card.host().timing(), card.host().clock_hz(), card.host().width()),
        (Timing::Hs, 52_000_000, 4)
    );
    let mut want = identification();
    want.extend([switch(ext::HS_TIMING, ext::TIMING_HS), (13, RCA), (13, RCA)]);
    want.extend([switch(ext::BUS_WIDTH, ext::WIDTH_4), (13, RCA), (8, 0)]);
    assert_eq!(commands(&m), want);
}

#[test]
fn the_lanes_8_bit_spec_3_host_runs_hs52_on_8_bits() {
    let m = model::machine(model::qemu_8bit());
    let card = crate::card(&m, Ceiling::Hs400es);
    assert_eq!(card.mode(), Mode::Hs52 { width: 8 });
    assert_eq!(card.host().clock_hz(), 52_000_000);
}

#[test]
fn the_k1_reaches_hs400_enhanced_strobe_at_187_5_mhz_with_the_measured_card() {
    let m = model::machine(model::k1());
    let card = crate::card(&m, Ceiling::Hs400es);
    assert_eq!(card.mode(), Mode::Hs400es);
    assert_eq!(card.sectors(), 30_535_680);
    assert_eq!(card.cid().name, *b"AJTD4R");
    // The stock system's operating point: 375 MHz io clock / 2.
    assert_eq!(
        (card.host().timing(), card.host().clock_hz(), card.host().width()),
        (Timing::Hs400, 187_500_000, 8)
    );
    // The stock kernel had switched the volatile cache on; CMD0 resets it (JEDEC E_P), so
    // no switch is needed here.
    let mut want = identification();
    want.extend([switch(ext::HS_TIMING, ext::TIMING_HS), (13, RCA), (13, RCA)]);
    want.extend([switch(ext::BUS_WIDTH, ext::WIDTH_8_DDR | ext::STROBE), (13, RCA)]);
    want.extend([switch(ext::HS_TIMING, ext::TIMING_HS400), (13, RCA), (8, 0)]);
    assert_eq!(commands(&m), want);
    assert_eq!(m.borrow().card.ext_csd[usize::from(ext::CACHE_CTRL)], 0);
}

#[test]
fn a_card_that_keeps_its_cache_across_cmd0_has_it_switched_off() {
    let m = model::machine(model::k1());
    m.borrow_mut().card.cache_survives_cmd0 = true;
    crate::card(&m, Ceiling::Hs400es);
    let issued = commands(&m);
    assert_eq!(&issued[11..13], &[switch(ext::CACHE_CTRL, 0), (13, RCA)]);
    assert_eq!(m.borrow().card.ext_csd[usize::from(ext::CACHE_CTRL)], 0);
}

#[test]
fn the_k1_vendor_registers_follow_the_upstream_sequence() {
    let m = model::machine(model::k1());
    crate::card(&m, Ceiling::Hs400es);
    let vendor: Vec<(usize, u32)> = m
        .borrow()
        .log
        .iter()
        .filter_map(|e| if let Event::Vendor(o, v) = *e { Some((o, v)) } else { None })
        .collect();
    let legacy = [(MMC_CTRL, MMC_CARD_MODE), (TX_CFG, TX_INT_CLK_SEL)];
    let mut want = vec![
        // After the full reset: PHY + PLL lock, drive 4 with the receive bias, MMC card
        // mode, the pad clock.
        (PHY_CTRL, PHY_FUNC_EN | PHY_PLL_LOCK),
        (PHY_PADCFG, PHY_DRIVE | RX_BIAS_CTRL),
        (MMC_CTRL, MMC_CARD_MODE),
        (LEGACY_CTRL, GEN_PAD_CLK_ON),
    ];
    // Identification, legacy, high speed: the internal transmit clock, no HS400 bits.
    want.extend(legacy);
    want.extend(legacy);
    want.extend(legacy);
    want.extend([
        // HS400: the mode bit, the transmit clock off the internal clock, then the strobe
        // and the DLL (pre-delay, range, regulator at 1; register 1 = 0x92; enable).
        (MMC_CTRL, MMC_CARD_MODE | MMC_HS400),
        (TX_CFG, 0),
        (MMC_CTRL, MMC_CARD_MODE | MMC_HS400 | ENHANCE_STROBE_EN),
        (PHY_DLLCFG, DLL_FIELDS_1),
        (PHY_DLLCFG1, DLL_REG1),
        (PHY_DLLCFG, DLL_FIELDS_1 | DLL_ENABLE),
    ]);
    assert_eq!(vendor, want);
}

#[test]
fn a_k1_card_without_the_strobe_stays_at_hs52_on_8_bits() {
    let mut config = model::k1();
    config.ext_csd[ext::STROBE_SUPPORT] = 0;
    let m = model::machine(config);
    assert_eq!(crate::card(&m, Ceiling::Hs400es).mode(), Mode::Hs52 { width: 8 });
}

#[test]
fn a_dll_that_never_locks_costs_hs400es_and_the_fallback_reports_it() {
    let m = model::machine(model::k1());
    m.borrow_mut().faults.dll_never_locks = true;
    let (card, lost) =
        Card::init_best(model::host(&m, true)).map_err(|f| f.error).expect("fallback");
    assert_eq!(card.mode(), Mode::Hs52 { width: 8 });
    assert_eq!(lost, Some(Error::Timeout(Stage::DllLock)));
    assert_eq!(card.host().clock_hz(), 46_875_000);
}

#[test]
fn a_legacy_ceiling_keeps_the_identification_path_and_the_widest_bus() {
    let m = model::machine(model::k1());
    let card = crate::card(&m, Ceiling::Legacy);
    assert_eq!(card.mode(), Mode::Legacy { width: 8 });
    assert_eq!(card.host().clock_hz(), 23_437_500);
}

#[test]
fn an_irq_platform_signals_the_status_on_the_line_and_a_polling_one_does_not() {
    for (irq, signal) in [(true, INT_USED), (false, 0)] {
        let m = model::machine(model::qemu_8bit());
        Card::init(model::host(&m, irq), Ceiling::Hs52).map_err(|f| f.error).expect("init");
        assert_eq!(m.borrow().ctrl.signalled(), signal);
    }
}

#[test]
fn init_is_deterministic_to_the_microsecond() {
    let run = || {
        let m = model::machine(model::k1());
        crate::card(&m, Ceiling::Hs400es);
        let m = m.borrow();
        (m.log.clone(), m.now)
    };
    assert_eq!(run(), run());
}
