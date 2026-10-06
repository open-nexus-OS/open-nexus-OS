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
/// address (the kernel's answer for the controller's device, `vmo_dma_base`), `stride`
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
    w.write(cm + regs::CMPS_BG_A, regs::CMPS_BG_ALPHA);
    // The desktop on the stock kernel's layer 7 (the pointer's layer 6 composites above it);
    // the loader's splash layer 0 off. The layer reads the channel the plane is fed through —
    // never a channel left idle.
    w.write(cm + regs::cmps_layer_en(regs::SPLASH_LAYER), 0);
    let layer = cm + regs::cmps_layer_en(regs::DESKTOP_LAYER);
    w.write(layer + regs::LAYER_LEFT, 0);
    w.write(layer + regs::LAYER_RIGHT_TOP, h.saturating_sub(1) << 16);
    w.write(
        layer + regs::LAYER_BOTTOM_BLEND,
        (regs::DESKTOP_BLEND_MODE << 16) | v.saturating_sub(1),
    );
    w.write(layer + regs::LAYER_ALPHA, regs::DESKTOP_ALPHA_WORD);
    w.write(layer, regs::cmps_layer_word(rdma));

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

/// Points RDMA channel `rdma` at another plane of the programmed mode — its bus address and its
/// row stride — and latches it: the reveal's switch from the boot splash to the desktop
/// (TASK-0251 P2a step 2), a frame-ring flip later. The plane's size is the mode's; a flip never
/// changes the mode.
pub fn flip<W: RegWriter>(w: &mut W, rdma: u32, plane: &Plane) {
    let ch = regs::rdma_base(rdma);
    write_plane_address(w, ch, plane.bus_addr);
    w.write(ch + regs::RDMA_STRIDE_BYTES, plane.stride);
    flush(w);
}

/// Bytes per pixel of the controller's XRGB8888 plane.
const BYTES_PER_PIXEL: usize = 4;

/// The bytes of `plane` (offsets from its first byte) that the damage `x, y, w, h` covers, clamped
/// to the plane's visible size: what the driver cleans out of the CPU caches before the controller
/// reads them (it does not snoop them). A damage across the whole visible width is ONE run of
/// whole rows; a narrower one is one run per row; a damage outside the plane covers nothing.
pub fn damage_spans(plane: &Plane, x: u32, y: u32, w: u32, h: u32) -> DamageSpans {
    let (width, height) = (u32::from(plane.width), u32::from(plane.height));
    let x_end = x.saturating_add(w).min(width);
    let y_end = y.saturating_add(h).min(height);
    if x >= x_end || y >= y_end {
        return DamageSpans { row: 0, end: 0, x_off: 0, len: 0, stride: 0, whole: false };
    }
    DamageSpans {
        row: y,
        end: y_end,
        x_off: x as usize * BYTES_PER_PIXEL,
        len: (x_end - x) as usize * BYTES_PER_PIXEL,
        stride: plane.stride as usize,
        whole: x == 0 && x_end == width,
    }
}

/// The runs [`damage_spans`] yields, as byte ranges from the plane's first byte.
#[derive(Clone, Debug)]
pub struct DamageSpans {
    row: u32,
    end: u32,
    x_off: usize,
    len: usize,
    stride: usize,
    whole: bool,
}

impl Iterator for DamageSpans {
    type Item = core::ops::Range<usize>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.row >= self.end {
            return None;
        }
        let start = self.row as usize * self.stride;
        if self.whole {
            self.row = self.end;
            return Some(start..self.end as usize * self.stride);
        }
        self.row += 1;
        Some(start + self.x_off..start + self.x_off + self.len)
    }
}

pub(super) fn write_plane_address<W: RegWriter>(w: &mut W, channel: u32, bus_addr: u64) {
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

    /// A 1080p plane at an example bus address (the stock system scanned through its display
    /// MMU, so the dump holds IO-virtual addresses, not this one), pitch 7680.
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
        assert_eq!(s.value_at(0x4c38), Some(0), "the loader's layer 0 is switched off");
        assert_eq!(s.value_at(0x4d18), Some(7), "the desktop's layer 7 reads RDMA3 here");
        let n = s.as_slice().len();
        assert_eq!(s.as_slice()[n - 2], Write { offset: 0x56c, value: 1 });
        assert_eq!(s.as_slice()[n - 1], Write { offset: 0x58c, value: 1 });
        assert!(n <= 40, "the first-light sequence is about thirty writes, got {n}");
    }

    /// The defect the board found (2026-10-03): the layer word named RDMA3 while the plane was
    /// fed through RDMA1, so the composer waited on an idle channel and nothing scanned. The
    /// layer must read exactly a channel the control word enables, for every channel.
    #[test]
    fn test_reject_a_layer_reading_a_channel_the_control_word_does_not_enable() {
        for rdma in 0..12 {
            let mut s = Sequence::new();
            bring_up(&mut s, &mode_1080p60(), &plane_1080p(), rdma);
            let layer = s.value_at(0x4d18).expect("the layer word");
            let enabled = s.value_at(0x560).expect("the control word") & 0xfff;
            assert_eq!(layer & 1, 1, "the layer is enabled");
            assert_eq!(enabled, 1 << (layer >> 1), "rdma {rdma}: the layer reads the fed channel");
        }
        // The stock kernel's words for its desktop layer 7 on RDMA1 (`dpu-regs-on-annotated.txt`:
        // 0x4d18 = 0x3, 0x4d2c = 0x077f0000, 0x4d30 = 0x00010437, 0x4d34 = 0x00ff0037).
        let mut s = Sequence::new();
        bring_up(&mut s, &mode_1080p60(), &plane_1080p(), 1);
        assert_eq!(s.value_at(0x4d18), Some(0x3));
        assert_eq!(s.value_at(0x4d28), Some(0));
        assert_eq!(s.value_at(0x4d2c), Some(0x077f_0000));
        assert_eq!(s.value_at(0x4d30), Some(0x0001_0437));
        assert_eq!(s.value_at(0x4d34), Some(0x00ff_0037));
        let en = s.as_slice().iter().position(|w| w.offset == 0x4d18).unwrap();
        let alpha = s.as_slice().iter().position(|w| w.offset == 0x4d34).unwrap();
        assert!(alpha < en, "the layer shows complete: its words before its enable");
    }

    /// A flip writes the address pair and the stride, then the two latch words, in that order —
    /// nothing of the mode.
    #[test]
    fn a_flip_is_the_address_the_stride_and_the_latch() {
        let mut s = Sequence::new();
        let plane = Plane { bus_addr: 0x1_8fb7_c000, ..plane_1080p() };
        flip(&mut s, 1, &plane);
        assert_eq!(
            s.as_slice(),
            &[
                Write { offset: 0xba0, value: 0x8fb7_c000 },
                Write { offset: 0xba4, value: 1 },
                Write { offset: 0xbb8, value: 7680 },
                Write { offset: 0x56c, value: 1 },
                Write { offset: 0x58c, value: 1 },
            ]
        );
    }

    /// A full-screen damage is ONE run of whole rows — one clean over the plane.
    #[test]
    fn a_full_screen_damage_is_one_run() {
        let spans: Vec<_> = damage_spans(&plane_1080p(), 0, 0, 1920, 1080).collect();
        assert_eq!(spans, [0..1080 * 7680]);
    }

    /// A pointer-sized damage is one run per row, each exactly the rectangle's bytes.
    #[test]
    fn a_narrow_damage_is_one_run_per_row() {
        let spans: Vec<_> = damage_spans(&plane_1080p(), 100, 200, 32, 32).collect();
        assert_eq!(spans.len(), 32);
        assert_eq!(spans[0], 200 * 7680 + 400..200 * 7680 + 400 + 128);
        assert_eq!(spans[31], 231 * 7680 + 400..231 * 7680 + 400 + 128);
    }

    /// A damage past the plane's edge is clipped to the visible size.
    #[test]
    fn a_damage_past_the_edge_is_clipped() {
        let spans: Vec<_> = damage_spans(&plane_1080p(), 1900, 1070, 100, 100).collect();
        assert_eq!(spans.len(), 10);
        assert_eq!(spans[9], 1079 * 7680 + 1900 * 4..1079 * 7680 + 1920 * 4);
    }

    /// The bytes a damage names come from windowd's commands — untrusted. Outside the plane, or
    /// empty, or at the integer edge, they name nothing; whatever they name lies inside the plane.
    #[test]
    fn test_reject_a_damage_outside_the_plane() {
        let p = plane_1080p();
        let max = u32::MAX;
        for (x, y, w, h) in [(1920, 0, 10, 10), (0, 1080, 10, 10), (0, 0, 0, 10), (5, 5, 10, 0)] {
            assert_eq!(damage_spans(&p, x, y, w, h).count(), 0, "({x},{y},{w},{h})");
        }
        assert_eq!(damage_spans(&p, max, max, max, max).count(), 0);
        let plane_bytes = 1080 * 7680;
        for (x, y, w, h) in [(0, 0, max, max), (1, 1, max, max), (1919, 1079, max, max)] {
            for span in damage_spans(&p, x, y, w, h) {
                assert!(span.start < span.end && span.end <= plane_bytes, "({x},{y}): {span:?}");
            }
        }
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
