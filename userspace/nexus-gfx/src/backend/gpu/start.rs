// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The GPU's start and stop register sequences (TASK-0329 P1) as pure logic over a
//! register trait — the open driver's sequence for this core's feature set (two reset words,
//! rascal/dusts out, everything out but the firmware processor, the SLC decode mode, the MIPS
//! wrapper and its remaps, the interconnect word, reset released), with the values the stock
//! system's live dump confirms where both write (`SLC_CTRL_MISC 0x200003`, `MIPS_WRAPPER_CONFIG
//! 0x1CF80`, the remap pairs). The device driver implements [`Regs`] over its MMIO window with
//! real 64-bit accesses (a 64-bit reset word written as two halves would release every reset
//! between them); the host implements it as a recorder and compares.
//! OWNERS: @gpu
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below + `tests/gpu_goldens.rs`

use super::mips_boot::{self, BootSections};

/// The register offsets this sequence touches (relative to the window).
pub mod cr {
    pub const CLK_CTRL: u32 = 0x0000;
    pub const CLK_STATUS: u32 = 0x0008;
    pub const CORE_ID: u32 = 0x0018;
    pub const CORE_REVISION: u32 = 0x0020;
    pub const CORE_ID_PBVNC: u32 = 0x0020;
    pub const SOFT_RESET: u32 = 0x0100;
    pub const SOFT_RESET2: u32 = 0x0108;
    pub const EVENT_STATUS: u32 = 0x0130;
    pub const EVENT_CLEAR: u32 = 0x0138;
    pub const TIMER: u32 = 0x0160;
    pub const SIDEKICK_IDLE: u32 = 0x03C8;
    pub const MIPS_WRAPPER_CONFIG: u32 = 0x0810;
    pub const MIPS_ADDR_REMAP1_CONFIG1: u32 = 0x0818;
    pub const MIPS_ADDR_REMAP1_CONFIG2: u32 = 0x0820;
    pub const MIPS_ADDR_REMAP2_CONFIG1: u32 = 0x0828;
    pub const MIPS_ADDR_REMAP2_CONFIG2: u32 = 0x0830;
    pub const MIPS_ADDR_REMAP3_CONFIG1: u32 = 0x0838;
    pub const MIPS_ADDR_REMAP3_CONFIG2: u32 = 0x0840;
    pub const MIPS_ADDR_REMAP5_CONFIG1: u32 = 0x0858;
    pub const MIPS_ADDR_REMAP5_CONFIG2: u32 = 0x0860;
    pub const MIPS_WRAPPER_IRQ_ENABLE: u32 = 0x08A0;
    pub const MIPS_WRAPPER_IRQ_STATUS: u32 = 0x08A8;
    pub const MIPS_WRAPPER_IRQ_CLEAR: u32 = 0x08B0;
    pub const MIPS_WRAPPER_NMI_ENABLE: u32 = 0x08B8;
    pub const MIPS_WRAPPER_NMI_EVENT: u32 = 0x08C0;
    pub const MIPS_DEBUG_CONFIG: u32 = 0x08C8;
    pub const MIPS_EXCEPTION_STATUS: u32 = 0x08D0;
    pub const MARS_IDLE: u32 = 0x08F8;
    pub const MTS_SCHEDULE: u32 = 0x0B00;
    pub const MTS_BGCTX_THREAD0_DM_ASSOC: u32 = 0x0B30;
    pub const MTS_INTCTX_THREAD0_DM_ASSOC: u32 = 0x0B40;
    pub const MTS_GARTEN_WRAPPER_CONFIG: u32 = 0x0B50;
    pub const MTS_GPU_INT_STATUS: u32 = 0x0BB0;
    pub const BIF_FAULT_BANK0_MMU_STATUS: u32 = 0x12B0;
    pub const BIF_FAULT_BANK0_REQ_STATUS: u32 = 0x12B8;
    pub const BIF_READS_EXT_STATUS: u32 = 0x1320;
    pub const BIFPM_READS_EXT_STATUS: u32 = 0x1338;
    pub const BIFPM_STATUS_MMU: u32 = 0x1350;
    pub const BIF_STATUS_MMU: u32 = 0x1358;
    pub const SLC_CTRL_MISC: u32 = 0x3800;
    pub const SLC_STATUS1: u32 = 0x3870;
    pub const SLC_IDLE: u32 = 0x3898;
    pub const AXI_ACE_LITE_CONFIGURATION: u32 = 0x38C0;
    pub const SYS_BUS_SECURE: u32 = 0xA100;
    pub const MMU_FAULT_STATUS: u32 = 0xE150;
    pub const MULTICORE_GPU: u32 = 0xF300;
    pub const MULTICORE_SYSTEM: u32 = 0xF308;

    /// The register window the tree grants (`reg` of the GPU node): the highest offset used
    /// fits inside.
    pub const WINDOW_LEN: u32 = 0x80000;

    pub const SOFT_RESET_PBE2_XE_MASKFULL: u64 = 0xFFEF_FFFF_FFFF_FC3D;
    pub const SOFT_RESET_MASKFULL: u64 = 0x00E7_FFFF_FFFF_FC3D;
    /// Rascal core (bit 31) and dusts A–H (bits 29, 30, 35..=40).
    pub const SOFT_RESET_RASCALDUSTS_EN: u64 = 0x0000_01F8_E000_0000;
    pub const SOFT_RESET_GARTEN_EN: u64 = 1 << 32;
    pub const SOFT_RESET2_MASKFULL: u64 = 0x001F_FFFF;
    pub const SLC_CTRL_MISC_ENABLE_PSG_HAZARD_CHECK_EN: u32 = 0x2;
    pub const SLC_CTRL_MISC_ADDR_DECODE_MODE_PVR_HASH1: u32 = 0x20_0000;
    pub const SLC_CTRL_MISC_BYPASS_BURST_COMBINER_EN: u32 = 0x1;
    pub const SLC_CTRL_MISC_LAZYWB_OVERRIDE_EN: u32 = 0x100_0000;
    pub const MTS_GARTEN_WRAPPER_CONFIG_IDLE_CTRL_META: u64 = 0;
    pub const MIPS_WRAPPER_IRQ_EVENT_EN: u32 = 1;
    pub const MARS_IDLE_CPU_EN: u32 = 0x2;
    pub const MARS_IDLE_MH_SYSARB0_EN: u32 = 0x4;
    pub const MTS_DM_ASSOC_MASKFULL: u32 = 0xFFFF;
    pub const MTS_DM_ASSOC_DM_ASSOC_CLRMSK: u32 = 0xFFFF_0000;
    pub const BIF_STATUS_MMU_MASKFULL: u32 = 0xFF;
    pub const BIFPM_STATUS_MMU_MASKFULL: u32 = 0xFF;
    pub const BIF_READS_EXT_STATUS_MASKFULL: u32 = 0x0FFF_FFFF;
    pub const BIFPM_READS_EXT_STATUS_MASKFULL: u32 = 0xFFFF;
    pub const SLC_STATUS1_MASKFULL: u64 = 0x8000_03FF_03FF_FFFF;
    pub const MIPS_EXCEPTION_STATUS_MASKFULL: u32 = 0x3F;
    /// `MTS_SCHEDULE` for a kick on the general-purpose data master: DM 0, nothing else.
    pub const MTS_SCHEDULE_DM_GP: u32 = 0;
}

/// The interconnect word the open driver writes before the first bus transaction ("set
/// everything to outer cache"): AW/AR domain non-snooping 3, AR domain cache maintenance 2,
/// AW/AR domain coherent 2, AW/AR cache coherent 2, AR cache maintenance 2.
pub const AXI_ACE_LITE_CONFIGURATION_WORD: u64 =
    (3 << 8) | (3 << 10) | (2 << 16) | (2 << 14) | (2 << 12) | (2 << 22) | (2 << 26) | (2 << 30);

/// What the sequences need to know about the core (from the firmware's device-info block).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CoreFeatures {
    pub pbe2_in_xe: bool,
    pub xe_tpu2: bool,
    pub sys_bus_secure_reset: bool,
    pub slc_cache_line_size_bits: u32,
    pub gpu_multicore_support: bool,
    pub layout_mars: bool,
    pub xt_top_infrastructure: bool,
    pub brn_63553: bool,
    pub brn_71242: bool,
}

impl CoreFeatures {
    /// The board's core as its published firmware describes it (G0 research, parsed trailer).
    pub const BOARD: Self = Self {
        pbe2_in_xe: true,
        xe_tpu2: true,
        sys_bus_secure_reset: true,
        slc_cache_line_size_bits: 512,
        gpu_multicore_support: true,
        layout_mars: true,
        xt_top_infrastructure: false,
        brn_63553: true,
        brn_71242: false,
    };

    const fn soft_reset_mask(&self) -> u64 {
        if self.pbe2_in_xe {
            cr::SOFT_RESET_PBE2_XE_MASKFULL
        } else {
            cr::SOFT_RESET_MASKFULL
        }
    }
}

/// The register bus the sequences run on. Offsets are relative to the window.
pub trait Regs {
    fn write32(&mut self, offset: u32, value: u32);
    fn write64(&mut self, offset: u32, value: u64);
    fn read32(&mut self, offset: u32) -> u32;
    fn read64(&mut self, offset: u32) -> u64;
    /// At least `us` microseconds.
    fn delay_us(&mut self, us: u32);
    /// Reads `offset` (32-bit) until `(value & mask) == expected` or `timeout_us` passed.
    fn poll32(
        &mut self,
        offset: u32,
        mask: u32,
        expected: u32,
        timeout_us: u32,
    ) -> Result<(), Timeout>;
    /// The 64-bit form.
    fn poll64(
        &mut self,
        offset: u32,
        mask: u64,
        expected: u64,
        timeout_us: u32,
    ) -> Result<(), Timeout>;
}

/// A poll ran past its bound; carries the last value read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timeout {
    pub offset: u32,
    pub last: u64,
}

/// The MIPS wrapper and its remaps (the open driver's `wrapper_init`).
pub fn wrapper_init<R: Regs>(r: &mut R, boot: &BootSections, f: &CoreFeatures) {
    r.write32(cr::MIPS_WRAPPER_CONFIG, mips_boot::wrapper_config());
    let m1 = boot.remap1();
    r.write64(cr::MIPS_ADDR_REMAP1_CONFIG1, m1.config1);
    r.write64(cr::MIPS_ADDR_REMAP1_CONFIG2, m1.config2);
    if f.brn_63553 {
        let m5 = boot.remap5();
        r.write64(cr::MIPS_ADDR_REMAP5_CONFIG1, m5.config1);
        r.write64(cr::MIPS_ADDR_REMAP5_CONFIG2, m5.config2);
    }
    let m2 = boot.remap2();
    r.write64(cr::MIPS_ADDR_REMAP2_CONFIG1, m2.config1);
    r.write64(cr::MIPS_ADDR_REMAP2_CONFIG2, m2.config2);
    let m3 = boot.remap3();
    r.write64(cr::MIPS_ADDR_REMAP3_CONFIG1, m3.config1);
    r.write64(cr::MIPS_ADDR_REMAP3_CONFIG2, m3.config2);
    r.write64(cr::MTS_GARTEN_WRAPPER_CONFIG, cr::MTS_GARTEN_WRAPPER_CONFIG_IDLE_CTRL_META);
    r.write32(cr::MIPS_DEBUG_CONFIG, 0);
}

/// The SLC's decode mode (stock: `0x200003` — the hazard-check bit kept from reset).
pub fn slc_init<R: Regs>(r: &mut R, f: &CoreFeatures) -> u32 {
    let mut v = (r.read32(cr::SLC_CTRL_MISC) & cr::SLC_CTRL_MISC_ENABLE_PSG_HAZARD_CHECK_EN)
        | cr::SLC_CTRL_MISC_ADDR_DECODE_MODE_PVR_HASH1;
    if f.slc_cache_line_size_bits < 1024 {
        v |= cr::SLC_CTRL_MISC_BYPASS_BURST_COMBINER_EN;
    }
    if f.brn_71242 && !f.gpu_multicore_support {
        v |= cr::SLC_CTRL_MISC_LAZYWB_OVERRIDE_EN;
    }
    r.write32(cr::SLC_CTRL_MISC, v);
    v
}

/// The start sequence: resets, SLC, wrapper, interconnect, reset released. After it the
/// firmware processor runs its boot loader; `firmware_started` in SYS init says when it is up.
pub fn start<R: Regs>(r: &mut R, boot: &BootSections, f: &CoreFeatures) {
    let mask = f.soft_reset_mask();
    if f.sys_bus_secure_reset {
        r.write32(cr::SYS_BUS_SECURE, 0);
        let _ = r.read32(cr::SYS_BUS_SECURE);
    }
    r.write64(cr::SOFT_RESET, mask);
    if f.xe_tpu2 {
        r.write64(cr::SOFT_RESET2, cr::SOFT_RESET2_MASKFULL);
    }
    let _ = r.read64(cr::SOFT_RESET);
    if f.xe_tpu2 {
        let _ = r.read64(cr::SOFT_RESET2);
    }
    r.write64(cr::SOFT_RESET, mask ^ cr::SOFT_RESET_RASCALDUSTS_EN);
    if f.xe_tpu2 {
        r.write64(cr::SOFT_RESET2, 0);
    }
    let _ = r.read64(cr::SOFT_RESET);
    if f.xe_tpu2 {
        let _ = r.read64(cr::SOFT_RESET2);
    }
    r.write64(cr::SOFT_RESET, cr::SOFT_RESET_GARTEN_EN);
    if f.xe_tpu2 {
        r.write64(cr::SOFT_RESET2, 0);
    }
    let _ = r.read64(cr::SOFT_RESET);
    if f.xe_tpu2 {
        let _ = r.read64(cr::SOFT_RESET2);
    }
    slc_init(r, f);
    wrapper_init(r, boot, f);
    r.write64(cr::AXI_ACE_LITE_CONFIGURATION, AXI_ACE_LITE_CONFIGURATION_WORD);
    r.delay_us(3);
    r.write64(cr::SOFT_RESET, 0);
    let _ = r.read64(cr::SOFT_RESET);
    r.delay_us(3);
}

/// The stop sequence for a MARS-layout MIPS core: the data masters disassociated, the bus
/// interfaces idle, the firmware processor idle, everything in reset.
pub fn stop<R: Regs>(r: &mut R, f: &CoreFeatures, poll_timeout_us: u32) -> Result<(), Timeout> {
    let clear = cr::MTS_DM_ASSOC_MASKFULL & cr::MTS_DM_ASSOC_DM_ASSOC_CLRMSK;
    r.write32(cr::MTS_INTCTX_THREAD0_DM_ASSOC, clear);
    r.write32(cr::MTS_BGCTX_THREAD0_DM_ASSOC, clear);
    r.poll32(cr::BIF_STATUS_MMU, cr::BIF_STATUS_MMU_MASKFULL, 0, poll_timeout_us)?;
    r.poll32(cr::BIFPM_STATUS_MMU, cr::BIFPM_STATUS_MMU_MASKFULL, 0, poll_timeout_us)?;
    if !f.xt_top_infrastructure {
        r.poll32(cr::BIF_READS_EXT_STATUS, cr::BIF_READS_EXT_STATUS_MASKFULL, 0, poll_timeout_us)?;
    }
    r.poll32(cr::BIFPM_READS_EXT_STATUS, cr::BIFPM_READS_EXT_STATUS_MASKFULL, 0, poll_timeout_us)?;
    r.poll64(cr::SLC_STATUS1, cr::SLC_STATUS1_MASKFULL, 0, poll_timeout_us)?;
    if f.layout_mars {
        let idle = cr::MARS_IDLE_CPU_EN | cr::MARS_IDLE_MH_SYSARB0_EN;
        r.poll32(cr::MARS_IDLE, idle, idle, poll_timeout_us)?;
    }
    r.write64(cr::SOFT_RESET, f.soft_reset_mask());
    Ok(())
}

/// Whether the firmware processor's interrupt is raised.
pub fn irq_pending<R: Regs>(r: &mut R) -> bool {
    r.read32(cr::MIPS_WRAPPER_IRQ_STATUS) & cr::MIPS_WRAPPER_IRQ_EVENT_EN != 0
}

/// Acknowledges the firmware processor's interrupt.
pub fn irq_clear<R: Regs>(r: &mut R) {
    r.write32(cr::MIPS_WRAPPER_IRQ_CLEAR, cr::MIPS_WRAPPER_IRQ_EVENT_EN);
}

/// One kick: every kernel command is scheduled on the general-purpose data master.
pub fn kick<R: Regs>(r: &mut R) {
    r.write32(cr::MTS_SCHEDULE, cr::MTS_SCHEDULE_DM_GP);
}

/// One recorded operation (the host's `Regs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    W32(u32, u32),
    W64(u32, u64),
    R32(u32),
    R64(u32),
    Delay(u32),
    Poll32(u32, u32, u32),
    Poll64(u32, u64, u64),
}

/// Records every operation; reads answer from a small table (the stock words), else zero.
#[derive(Clone, Debug, Default)]
pub struct Recorder {
    pub ops: alloc::vec::Vec<Op>,
    pub reads: alloc::vec::Vec<(u32, u64)>,
}

impl Recorder {
    pub fn answering(reads: &[(u32, u64)]) -> Self {
        Self { ops: alloc::vec::Vec::new(), reads: reads.to_vec() }
    }

    fn answer(&self, offset: u32) -> u64 {
        self.reads.iter().find(|(o, _)| *o == offset).map_or(0, |(_, v)| *v)
    }

    /// The last value written to `offset` (32 or 64), if any.
    pub fn written(&self, offset: u32) -> Option<u64> {
        self.ops.iter().rev().find_map(|op| match op {
            Op::W32(o, v) if *o == offset => Some(*v as u64),
            Op::W64(o, v) if *o == offset => Some(*v),
            _ => None,
        })
    }
}

impl Regs for Recorder {
    fn write32(&mut self, offset: u32, value: u32) {
        self.ops.push(Op::W32(offset, value));
    }
    fn write64(&mut self, offset: u32, value: u64) {
        self.ops.push(Op::W64(offset, value));
    }
    fn read32(&mut self, offset: u32) -> u32 {
        self.ops.push(Op::R32(offset));
        self.answer(offset) as u32
    }
    fn read64(&mut self, offset: u32) -> u64 {
        self.ops.push(Op::R64(offset));
        self.answer(offset)
    }
    fn delay_us(&mut self, us: u32) {
        self.ops.push(Op::Delay(us));
    }
    fn poll32(
        &mut self,
        offset: u32,
        mask: u32,
        expected: u32,
        _timeout_us: u32,
    ) -> Result<(), Timeout> {
        self.ops.push(Op::Poll32(offset, mask, expected));
        let v = self.answer(offset) as u32;
        if v & mask == expected {
            Ok(())
        } else {
            Err(Timeout { offset, last: v as u64 })
        }
    }
    fn poll64(
        &mut self,
        offset: u32,
        mask: u64,
        expected: u64,
        _timeout_us: u32,
    ) -> Result<(), Timeout> {
        self.ops.push(Op::Poll64(offset, mask, expected));
        let v = self.answer(offset);
        if v & mask == expected {
            Ok(())
        } else {
            Err(Timeout { offset, last: v })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STOCK_BOOT: BootSections = BootSections {
        boot_code_phys: 0x8979_8000,
        boot_data_phys: 0x8BDC_8000,
        exception_code_phys: 0x878F_7000,
    };

    #[test]
    fn the_start_sequence_writes_the_stock_words_where_both_write() {
        // The SLC word reads back as the stock system left it (hazard-check bit set).
        let mut r = Recorder::answering(&[(cr::SLC_CTRL_MISC, 0x0020_0003)]);
        start(&mut r, &STOCK_BOOT, &CoreFeatures::BOARD);
        assert_eq!(r.written(cr::SLC_CTRL_MISC), Some(0x0020_0003));
        assert_eq!(r.written(cr::MIPS_WRAPPER_CONFIG), Some(0x1CF80));
        assert_eq!(r.written(cr::MIPS_ADDR_REMAP1_CONFIG1), Some(0x1FC0_0001));
        assert_eq!(r.written(cr::MIPS_ADDR_REMAP1_CONFIG2), Some(0x8979_800C));
        assert_eq!(r.written(cr::MIPS_ADDR_REMAP2_CONFIG2), Some(0x8BDC_800C));
        assert_eq!(r.written(cr::MIPS_ADDR_REMAP3_CONFIG2), Some(0x878F_700C));
        assert_eq!(r.written(cr::MIPS_ADDR_REMAP5_CONFIG1), Some(0x1));
        assert_eq!(r.written(cr::MIPS_ADDR_REMAP5_CONFIG2), Some(0x8979_800C));
        assert_eq!(r.written(cr::AXI_ACE_LITE_CONFIGURATION), Some(0x8882_AF00));
        assert_eq!(r.written(cr::SOFT_RESET), Some(0), "released last");
        assert_eq!(r.written(cr::SYS_BUS_SECURE), Some(0));
    }

    #[test]
    fn the_reset_words_go_in_the_open_order_as_64_bit_writes() {
        let mut r = Recorder::default();
        start(&mut r, &STOCK_BOOT, &CoreFeatures::BOARD);
        let resets: alloc::vec::Vec<u64> = r
            .ops
            .iter()
            .filter_map(|op| match op {
                Op::W64(o, v) if *o == cr::SOFT_RESET => Some(*v),
                _ => None,
            })
            .collect();
        assert_eq!(
            resets,
            alloc::vec![
                0xFFEF_FFFF_FFFF_FC3D,
                0xFFEF_FE07_1FFF_FC3D, // rascal and dusts out
                0x0000_0001_0000_0000, // everything out but the firmware processor
                0,
            ]
        );
        assert!(
            !r.ops.iter().any(
                |op| matches!(op, Op::W32(o, _) if *o == cr::SOFT_RESET || *o == cr::SOFT_RESET + 4)
            ),
            "a reset word is never written as halves"
        );
        let delays = r.ops.iter().filter(|op| matches!(op, Op::Delay(3))).count();
        assert_eq!(delays, 2);
        // The interconnect word precedes the release.
        let axi = r
            .ops
            .iter()
            .position(|op| matches!(op, Op::W64(o, _) if *o == cr::AXI_ACE_LITE_CONFIGURATION))
            .unwrap();
        let release = r
            .ops
            .iter()
            .rposition(|op| matches!(op, Op::W64(o, 0) if *o == cr::SOFT_RESET))
            .unwrap();
        assert!(axi < release);
    }

    #[test]
    fn a_core_without_the_quirk_skips_remap_5() {
        let mut r = Recorder::default();
        let f = CoreFeatures { brn_63553: false, ..CoreFeatures::BOARD };
        wrapper_init(&mut r, &STOCK_BOOT, &f);
        assert_eq!(r.written(cr::MIPS_ADDR_REMAP5_CONFIG1), None);
        assert_eq!(r.written(cr::MIPS_ADDR_REMAP1_CONFIG1), Some(0x1FC0_0001));
    }

    #[test]
    fn stop_polls_the_bus_idle_and_ends_in_full_reset() {
        let mut r = Recorder::answering(&[(cr::MARS_IDLE, 0x7)]);
        stop(&mut r, &CoreFeatures::BOARD, 1000).unwrap();
        assert_eq!(r.written(cr::SOFT_RESET), Some(0xFFEF_FFFF_FFFF_FC3D));
        assert!(r.ops.contains(&Op::Poll32(cr::MARS_IDLE, 0x6, 0x6)));
        assert!(r.ops.contains(&Op::Poll64(cr::SLC_STATUS1, cr::SLC_STATUS1_MASKFULL, 0)));
        // A processor that never idles is a named timeout, not a reset.
        let mut r = Recorder::answering(&[(cr::MARS_IDLE, 0x0)]);
        assert_eq!(
            stop(&mut r, &CoreFeatures::BOARD, 1000),
            Err(Timeout { offset: cr::MARS_IDLE, last: 0 })
        );
        assert_eq!(r.written(cr::SOFT_RESET), None);
    }

    #[test]
    fn the_interrupt_is_one_bit_and_the_kick_is_one_word() {
        let mut r = Recorder::answering(&[(cr::MIPS_WRAPPER_IRQ_STATUS, 1)]);
        assert!(irq_pending(&mut r));
        irq_clear(&mut r);
        kick(&mut r);
        assert_eq!(r.written(cr::MIPS_WRAPPER_IRQ_CLEAR), Some(1));
        assert_eq!(r.written(cr::MTS_SCHEDULE), Some(0));
    }
}
