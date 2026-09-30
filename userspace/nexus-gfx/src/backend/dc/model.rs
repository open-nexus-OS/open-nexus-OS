// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The display controller's bring-up, plane and flush sequence as pure logic over a
//! register-writer trait (TASK-0250 P2). The sequence is the boot loader's splash path
//! (about thirty direct writes, no command list, no display MMU) with the values the live dump
//! shows at 1920x1080@60 (`docs/board/measurements/2026-09-29-display-regs/README.md`
//! §"first-light sequence"); the goldens below reproduce the measured timing, plane and
//! composer words exactly. The OS driver (TASK-0251 P2) implements [`RegWriter`] over its MMIO
//! window and calls the same three functions; the host implements it as a recorder.
//! OWNERS: @gpu
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: goldens below + `tests/dc_goldens.rs`

use super::edid::Mode;
use super::regs;

/// Where the controller's registers are written. `offset` is relative to the window
/// (`reg = <0xc0440000 …>`); the driver adds its base, the recorder keeps the pair.
pub trait RegWriter {
    fn write(&mut self, offset: u32, value: u32);
}

/// One recorded write (host recorder, and the type the goldens compare).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Write {
    pub offset: u32,
    pub value: u32,
}

/// A bounded recorder of writes — the host's [`RegWriter`].
#[derive(Clone, Copy, Debug)]
pub struct Sequence {
    writes: [Write; Sequence::CAP],
    len: usize,
    /// Writes beyond the capacity (a sequence that grows past it is a bug the test sees).
    pub dropped: usize,
}

impl Sequence {
    pub const CAP: usize = 64;
    pub const fn new() -> Self {
        Self { writes: [Write { offset: 0, value: 0 }; Self::CAP], len: 0, dropped: 0 }
    }
    pub fn as_slice(&self) -> &[Write] {
        &self.writes[..self.len]
    }
    /// The last value written at `offset`, if any.
    pub fn value_at(&self, offset: u32) -> Option<u32> {
        self.as_slice().iter().rev().find(|w| w.offset == offset).map(|w| w.value)
    }
}

impl Default for Sequence {
    fn default() -> Self {
        Self::new()
    }
}

impl RegWriter for Sequence {
    fn write(&mut self, offset: u32, value: u32) {
        if self.len < Self::CAP {
            self.writes[self.len] = Write { offset, value };
            self.len += 1;
        } else {
            self.dropped += 1;
        }
    }
}

/// The one scanout plane: a contiguous XRGB8888 buffer the controller reads by its BUS
/// address (CPU − the bus's `dma-ranges` base; bank 0 on the reference board), `stride`
/// bytes per row, `width`x`height` pixels — the layout maximum's stride is expected
/// (`nexus_display_proto::layout::STRIDE_BYTES`), the visible size is the mode's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plane {
    pub bus_addr: u64,
    pub stride: u32,
    pub width: u16,
    pub height: u16,
}

/// Brings pipeline 2 up for `mode` scanning `plane` through RDMA channel `rdma`
/// (0..12; the splash uses 3, the stock kernel 0 and 1) — the first-light sequence. Ends with
/// config-ready and software start, so the next vertical blank shows the plane.
pub fn bring_up<W: RegWriter>(w: &mut W, mode: &Mode, plane: &Plane, rdma: u32) {
    let (h, v) = (mode.h_active as u32, mode.v_active as u32);
    w.write(regs::DMA_TOP_QOS, regs::DMA_TOP_QOS_VALUE);

    let oc = regs::outctrl_base(regs::PIPELINE);
    w.write(oc + regs::OUTCTRL_IN_SIZE, (v << 16) | h);
    w.write(oc + regs::OUTCTRL_FRAME_TIMING, regs::OUTCTRL_FRAME_TIMING_ON);
    w.write(oc + regs::OUTCTRL_MODE, regs::OUTCTRL_MODE_VALUE);
    w.write(oc + regs::OUTCTRL_H_PORCH, ((mode.hbp as u32) << 16) | mode.hfp as u32);
    w.write(oc + regs::OUTCTRL_V_PORCH, ((mode.vbp as u32) << 16) | mode.vfp as u32);
    w.write(
        oc + regs::OUTCTRL_SYNC,
        ((mode.vsync_positive as u32) << 28)
            | ((mode.vsw as u32) << 16)
            | ((mode.hsync_positive as u32) << 12)
            | mode.hsw as u32,
    );
    w.write(oc + regs::OUTCTRL_ACTIVE, (v << 16) | h);
    w.write(oc + regs::OUTCTRL_FORMAT, regs::OUTCTRL_FORMAT_RGB888);

    let ch = regs::rdma_base(rdma);
    w.write(ch + regs::RDMA_CTRL, regs::RDMA_CTRL_VALUE);
    write_plane_address(w, ch, plane.bus_addr);
    w.write(ch + regs::RDMA_STRIDE_BYTES, plane.stride);
    let (pw, ph) = (plane.width as u32, plane.height as u32);
    w.write(ch + regs::RDMA_SIZE, (ph << 16) | pw);
    w.write(ch + regs::RDMA_BBOX_START, 0);
    w.write(ch + regs::RDMA_BBOX_END, (ph.saturating_sub(1) << 16) | pw.saturating_sub(1));
    w.write(ch + regs::RDMA_FORMAT, regs::RDMA_FORMAT_XRGB8888);

    let cm = regs::cmps_base(regs::PIPELINE);
    w.write(cm + regs::CMPS_ENABLE_WIDTH, (h << 8) | 1);
    w.write(cm + regs::CMPS_HEIGHT, v);
    w.write(cm + regs::CMPS_BG_G, regs::CMPS_BG_VALUE);
    w.write(cm + regs::CMPS_BG_A, regs::CMPS_BG_ALPHA);
    w.write(cm + regs::CMPS_LAYER_EN, regs::CMPS_LAYER_EN_VALUE);
    w.write(cm + regs::CMPS_LAYER_LT, 0);
    w.write(cm + regs::CMPS_LAYER_RIGHT, h.saturating_sub(1) << 16);
    w.write(cm + regs::CMPS_LAYER_BOTTOM, v.saturating_sub(1));
    w.write(cm + regs::CMPS_LAYER_ALPHA, regs::CMPS_LAYER_ALPHA_VALUE);

    w.write(regs::CTL2_ENABLE, (1 << rdma) | regs::CTL_OUTCTL_EN);
    w.write(regs::CTL2_MODE, regs::CTL2_MODE_VALUE);
    flush(w);
}

/// Latches the shadowed configuration and starts (or restarts) scanning: what every change
/// ends with — the two writes the splash ends with.
pub fn flush<W: RegWriter>(w: &mut W) {
    w.write(regs::CTL2_CFG_READY, 1);
    w.write(regs::CTL2_SW_START, 1);
}

/// Points RDMA channel `rdma` at a new plane address (the frame-ring flip) and flushes.
pub fn set_plane_address<W: RegWriter>(w: &mut W, rdma: u32, bus_addr: u64) {
    write_plane_address(w, regs::rdma_base(rdma), bus_addr);
    flush(w);
}

fn write_plane_address<W: RegWriter>(w: &mut W, channel: u32, bus_addr: u64) {
    w.write(channel + regs::RDMA_ADDR_LO, bus_addr as u32);
    w.write(channel + regs::RDMA_ADDR_HI, (bus_addr >> 32) as u32);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The 1080p60 timing the reference monitor asked for and the stock system drove.
    fn mode_1080p60() -> Mode {
        Mode {
            h_active: 1920,
            v_active: 1080,
            hfp: 88,
            hsw: 44,
            hbp: 148,
            vfp: 2,
            vsw: 5,
            vbp: 38,
            hsync_positive: true,
            vsync_positive: true,
            pixel_clock_khz: 148_500,
            detailed: true,
        }
    }

    /// The stock system's plane: CMA at CPU 0x8fb7c000 = bus 0x0fb7c000, pitch 7680.
    fn plane_1080p() -> Plane {
        Plane { bus_addr: 0x0fb7_c000, stride: 7680, width: 1920, height: 1080 }
    }

    /// The timing words come out exactly as the live dump holds them at 1920x1080@60
    /// (`dpu-regs-on-annotated.txt`: 0x18080/84/88/8c), and the plane words as the stock
    /// kernel's RDMA channels held them (stride 0x1e00, size 0x04380780, bbox 0x0437077f).
    #[test]
    fn bring_up_reproduces_the_measured_words() {
        let mut s = Sequence::new();
        bring_up(&mut s, &mode_1080p60(), &plane_1080p(), 1);
        assert_eq!(s.dropped, 0);
        assert_eq!(s.value_at(0x18080), Some(0x0094_0058));
        assert_eq!(s.value_at(0x18084), Some(0x0026_0002));
        assert_eq!(s.value_at(0x18088), Some(0x1005_102c));
        assert_eq!(s.value_at(0x1808c), Some(0x0438_0780));
        assert_eq!(s.value_at(0x18000), Some(0x0438_0780));
        assert_eq!(s.value_at(0x18018), Some(0x20));
        assert_eq!(s.value_at(0x18090), Some(2));
        // RDMA channel 1 at 0xb80 (the stock kernel's active channel).
        assert_eq!(s.value_at(0xbb8), Some(0x1e00));
        assert_eq!(s.value_at(0xbbc), Some(0x0438_0780));
        assert_eq!(s.value_at(0xbc4), Some(0x0437_077f));
        assert_eq!(s.value_at(0xba0), Some(0x0fb7_c000));
        assert_eq!(s.value_at(0xba4), Some(0));
        assert_eq!(s.value_at(0xbf0), Some(8));
        // Composer 2 (measured 0x00078001 / 0x438) and the ctl2 enable word (measured
        // 0x00040002 with channel 1).
        assert_eq!(s.value_at(0x4c00), Some(0x0007_8001));
        assert_eq!(s.value_at(0x4c04), Some(0x438));
        assert_eq!(s.value_at(0x560), Some(0x0004_0002));
        assert_eq!(s.value_at(0x588), Some(0x821));
        assert_eq!(s.value_at(0xa1c), Some(0x2223));
    }

    /// The splash's channel (3) gives the splash's words: 0xda0 for the address,
    /// 0x40008 for the enable word — and the sequence ends with cfg-ready then sw-start.
    #[test]
    fn the_splash_channel_matches_the_boot_loaders_offsets() {
        let mut s = Sequence::new();
        bring_up(&mut s, &mode_1080p60(), &plane_1080p(), 3);
        assert_eq!(s.value_at(0xda0), Some(0x0fb7_c000));
        assert_eq!(s.value_at(0xdb8), Some(7680));
        assert_eq!(s.value_at(0x560), Some(0x0004_0008));
        let n = s.as_slice().len();
        assert_eq!(s.as_slice()[n - 2], Write { offset: 0x56c, value: 1 });
        assert_eq!(s.as_slice()[n - 1], Write { offset: 0x58c, value: 1 });
        assert!(n <= 40, "the first-light sequence is about thirty writes, got {n}");
    }

    /// A flip writes only the address pair and the two latch words, in that order.
    #[test]
    fn a_plane_flip_is_four_writes() {
        let mut s = Sequence::new();
        set_plane_address(&mut s, 1, 0x1_0000_0000 + 0x8fb7_c000);
        assert_eq!(
            s.as_slice(),
            &[
                Write { offset: 0xba0, value: 0x8fb7_c000 },
                Write { offset: 0xba4, value: 1 },
                Write { offset: 0x56c, value: 1 },
                Write { offset: 0x58c, value: 1 },
            ]
        );
    }

    /// Every offset the sequence touches lies inside the controller window the tree names.
    #[test]
    fn test_reject_writes_outside_the_window() {
        let mut s = Sequence::new();
        bring_up(&mut s, &mode_1080p60(), &plane_1080p(), 11);
        for w in s.as_slice() {
            assert!(w.offset + 4 <= regs::WINDOW_LEN, "0x{:x} outside the window", w.offset);
            assert_eq!(w.offset % 4, 0);
        }
    }
}
