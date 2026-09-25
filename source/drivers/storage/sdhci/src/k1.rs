// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The K1 layer: the vendor registers beyond the standard map (offsets and bits are facts
//! from the mainline driver and binding, `docs/board/measurements/2026-09-24-emmc-sdhci`;
//! no code is copied) and the steps they take — after a full reset the PHY and the pads, the
//! transmit clock per timing, the HS400 mode bit, and the enhanced strobe with the PHY DLL
//! locked (no delay-line tuning: HS400ES needs none, and HS200 is not driven). The eMMC host
//! only (its node has no `no-mmc`); an SD-only host's clock override is not driven here.
//! OWNERS: @runtime @drivers

use nexus_hal::Bus;

use crate::host::{poll, rmw, Timing};
use crate::{Error, Platform, Stage};

/// Pad clock generation (`LEGACY_CTRL`).
pub const LEGACY_CTRL: usize = 0x10C;
/// `LEGACY_CTRL`: generate the pad clock.
pub const GEN_PAD_CLK_ON: u32 = 1 << 6;
/// The MMC mode register.
pub const MMC_CTRL: usize = 0x114;
/// `MMC_CTRL`: the enhanced strobe.
pub const ENHANCE_STROBE_EN: u32 = 1 << 8;
/// `MMC_CTRL`: HS400.
pub const MMC_HS400: u32 = 1 << 9;
/// `MMC_CTRL`: HS200.
pub const MMC_HS200: u32 = 1 << 10;
/// `MMC_CTRL`: an MMC card is attached.
pub const MMC_CARD_MODE: u32 = 1 << 12;
/// The transmit configuration.
pub const TX_CFG: usize = 0x11C;
/// `TX_CFG`: transmit on the internal clock (timings up to SDR50).
pub const TX_INT_CLK_SEL: u32 = 1 << 30;
/// The PHY control.
pub const PHY_CTRL: usize = 0x160;
/// `PHY_CTRL`: the PHY function.
pub const PHY_FUNC_EN: u32 = 1 << 0;
/// `PHY_CTRL`: the PHY PLL lock function.
pub const PHY_PLL_LOCK: u32 = 1 << 1;
/// The PHY DLL configuration.
pub const PHY_DLLCFG: usize = 0x168;
/// `PHY_DLLCFG`: pre-delay [3:2], full delay range [5:4], regulator [7:6].
pub const DLL_FIELDS: u32 = 0xFC;
/// `PHY_DLLCFG`: each of the three fields at 1.
pub const DLL_FIELDS_1: u32 = (1 << 2) | (1 << 4) | (1 << 6);
/// `PHY_DLLCFG`: the DLL runs.
pub const DLL_ENABLE: u32 = 1 << 31;
/// The PHY DLL configuration, second word.
pub const PHY_DLLCFG1: usize = 0x16C;
/// `PHY_DLLCFG1`: control register 1 [7:0].
pub const DLL_REG1_MASK: u32 = 0xFF;
/// `PHY_DLLCFG1`: the value control register 1 takes.
pub const DLL_REG1: u32 = 0x92;
/// The PHY DLL status.
pub const PHY_DLLSTS: usize = 0x170;
/// `PHY_DLLSTS`: locked.
pub const DLL_LOCKED: u32 = 1 << 0;
/// The PHY pad configuration.
pub const PHY_PADCFG: usize = 0x178;
/// `PHY_PADCFG`: the drive strength [2:0].
pub const PHY_DRIVE_MASK: u32 = 0x7;
/// `PHY_PADCFG`: the drive strength the board runs.
pub const PHY_DRIVE: u32 = 4;
/// `PHY_PADCFG`: the receive bias.
pub const RX_BIAS_CTRL: u32 = 1 << 5;
/// The DLL locks within this (upstream bound).
pub const DLL_LOCK_TIMEOUT_US: u64 = 100;
/// Between two reads of the DLL status.
pub const DLL_POLL_US: u64 = 2;

/// After a full reset: the PHY function and its PLL lock, drive 4 with the receive bias,
/// MMC card mode, the pad clock.
pub(crate) fn after_reset<B: Bus>(bus: &B) {
    rmw(bus, PHY_CTRL, 0, PHY_FUNC_EN | PHY_PLL_LOCK);
    rmw(bus, PHY_PADCFG, PHY_DRIVE_MASK, PHY_DRIVE | RX_BIAS_CTRL);
    rmw(bus, MMC_CTRL, 0, MMC_CARD_MODE);
    rmw(bus, LEGACY_CTRL, 0, GEN_PAD_CLK_ON);
}

/// Before the clock changes: timings up to SDR50 transmit on the internal clock, faster
/// ones do not.
pub(crate) fn before_clock<B: Bus>(bus: &B, timing: Timing) {
    match timing {
        Timing::Legacy | Timing::Hs => rmw(bus, TX_CFG, 0, TX_INT_CLK_SEL),
        Timing::Hs400 => rmw(bus, TX_CFG, TX_INT_CLK_SEL, 0),
    }
}

/// At a timing change: the MMC mode bits follow the timing.
pub(crate) fn set_timing<B: Bus>(bus: &B, timing: Timing) {
    match timing {
        Timing::Hs400 => rmw(bus, MMC_CTRL, MMC_HS200, MMC_HS400),
        Timing::Legacy | Timing::Hs => {
            rmw(bus, MMC_CTRL, MMC_HS200 | MMC_HS400 | ENHANCE_STROBE_EN, 0)
        }
    }
}

/// The enhanced strobe: enable it, configure the PHY DLL, run it, wait for its lock.
pub(crate) fn enhanced_strobe<B: Bus, P: Platform>(bus: &B, platform: &mut P) -> Result<(), Error> {
    rmw(bus, MMC_CTRL, 0, ENHANCE_STROBE_EN);
    rmw(bus, PHY_DLLCFG, DLL_FIELDS, DLL_FIELDS_1);
    rmw(bus, PHY_DLLCFG1, DLL_REG1_MASK, DLL_REG1);
    rmw(bus, PHY_DLLCFG, 0, DLL_ENABLE);
    poll(bus, platform, Stage::DllLock, DLL_LOCK_TIMEOUT_US, DLL_POLL_US, |b| {
        b.read(PHY_DLLSTS) & DLL_LOCKED != 0
    })
}
