// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//! CONTEXT: the pointer as a layer of the display controller (TASK-0251 P2a step 3c) as pure
//! logic over the register-writer trait: the stock system's arrangement measured 2026-10-05
//! (`docs/board/measurements/2026-10-05-cursor-layer/`) — RDMA channel 2 feeding composer
//! layer 6 with a 64×64 ARGB8888 block, a move the layer's rectangle, the latch config-ready
//! alone (the scan keeps running). The driver (`gpud`'s controller path) runs these over its
//! window; the goldens below hold the measured words.
//! OWNERS: @gpu
//! STATUS: Functional (the first board cycle decides the rectangle's packing and the latch)
//! API_STABILITY: Internal
//! TEST_COVERAGE: goldens below (the measured words, a move, the clipping, the sprite layout)
use super::model::{write_plane_address, RegWriter};
use super::regs;

/// The pointer's buffer: one 64×64 ARGB8888 block (`regs::CURSOR_BYTES`, rows of
/// `regs::CURSOR_STRIDE_BYTES`) the controller reads by its bus address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CursorPlane {
    pub bus_addr: u64,
}

/// Where the pointer's layer sits on screen (TASK-0251 P2a step 3c): the sprite's hotspot at
/// the pointer, the layer's rectangle clipped to the mode — a sprite partly off the left or
/// top edge is cropped by the channel's bounding box, one off the right or bottom by the
/// rectangle (the buffer stays 64×64).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CursorRect {
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
    /// The first buffer column/row shown (the part cut off the left/top edge).
    pub crop_x: u32,
    pub crop_y: u32,
}

/// The layer's rectangle for a pointer at `x, y` with the sprite's hotspot `hot` on a
/// `width`×`height` mode; `None` when nothing of the 64×64 buffer is on screen.
#[must_use]
pub fn cursor_rect(x: i32, y: i32, hot: (u32, u32), width: u32, height: u32) -> Option<CursorRect> {
    let size = regs::CURSOR_SIZE as i64;
    let left = i64::from(x) - i64::from(hot.0);
    let top = i64::from(y) - i64::from(hot.1);
    let (w, h) = (i64::from(width), i64::from(height));
    if left + size <= 0 || top + size <= 0 || left >= w || top >= h {
        return None;
    }
    let crop_x = (-left).max(0);
    let crop_y = (-top).max(0);
    let left_on = left.max(0);
    let top_on = top.max(0);
    let right = (left + size - 1).min(w - 1);
    let bottom = (top + size - 1).min(h - 1);
    Some(CursorRect {
        left: left_on as u32,
        top: top_on as u32,
        right: right as u32,
        bottom: bottom as u32,
        crop_x: crop_x as u32,
        crop_y: crop_y as u32,
    })
}

/// Arm the pointer's layer: channel 2 reads `cursor` as 64×64 ARGB8888 (the stock pointer
/// channel's words), composer layer 6 shows it at `rect` with the stock blend mode and alpha,
/// the channel joins the pipeline's enable word beside the desktop's `rdma`, and the
/// configuration is latched the way the proven plane switch latches (config-ready and the
/// software start — board cycle 12 armed the layer with config-ready alone and showed nothing).
/// Two word groups the stock dump holds on the pointer's channel (`0xcc`, `0xe0..=0xfc`) are
/// not written: cycle 12 read them back unchanged before the channel ran (state, not glue).
pub fn cursor_layer_on<W: RegWriter>(
    w: &mut W,
    cursor: &CursorPlane,
    rdma: u32,
    rect: &CursorRect,
) {
    let ch = regs::rdma_base(regs::CURSOR_RDMA);
    w.write(ch + regs::RDMA_CTRL, regs::RDMA_CTRL_CURSOR);
    write_plane_address(w, ch, cursor.bus_addr);
    w.write(ch + regs::RDMA_STRIDE_BYTES, regs::CURSOR_STRIDE_BYTES);
    w.write(ch + regs::RDMA_SIZE, (regs::CURSOR_SIZE << 16) | regs::CURSOR_SIZE);
    w.write(ch + regs::RDMA_FORMAT, regs::RDMA_FORMAT_ARGB8888);
    w.write(ch + regs::RDMA_CURSOR_WORD_78, regs::RDMA_CURSOR_WORD_78_VALUE);
    cursor_rect_words(w, rect);
    let layer = regs::cmps_base(regs::PIPELINE) + regs::cmps_layer_en(regs::CURSOR_LAYER);
    w.write(layer + regs::LAYER_ALPHA, regs::CURSOR_ALPHA_WORD);
    w.write(layer, regs::cmps_layer_word(regs::CURSOR_RDMA));
    w.write(regs::CTL2_ENABLE, (1 << rdma) | (1 << regs::CURSOR_RDMA) | regs::CTL_OUTCTL_EN);
    super::model::flush(w);
}

/// Move the pointer's layer to `rect` and latch: the channel's crop and the layer's three
/// rectangle words — no present, no scan restart.
pub fn cursor_move<W: RegWriter>(w: &mut W, rect: &CursorRect) {
    cursor_rect_words(w, rect);
    latch(w);
}

/// Take the pointer's layer off the composer (the channel stays programmed) and latch.
pub fn cursor_layer_off<W: RegWriter>(w: &mut W, rdma: u32) {
    let layer = regs::cmps_base(regs::PIPELINE) + regs::cmps_layer_en(regs::CURSOR_LAYER);
    w.write(layer, 0);
    w.write(regs::CTL2_ENABLE, (1 << rdma) | regs::CTL_OUTCTL_EN);
    latch(w);
}

/// The channel's bounding box (the crop) and the layer's rectangle words.
fn cursor_rect_words<W: RegWriter>(w: &mut W, rect: &CursorRect) {
    let ch = regs::rdma_base(regs::CURSOR_RDMA);
    let last = regs::CURSOR_SIZE - 1;
    w.write(ch + regs::RDMA_COMPOSER_Y, rect.top);
    w.write(ch + regs::RDMA_BBOX_START, (rect.crop_y << 16) | rect.crop_x);
    w.write(ch + regs::RDMA_BBOX_END, (last << 16) | last);
    let layer = regs::cmps_base(regs::PIPELINE) + regs::cmps_layer_en(regs::CURSOR_LAYER);
    w.write(layer + regs::LAYER_LEFT, rect.left << 8);
    w.write(layer + regs::LAYER_RIGHT_TOP, (rect.right << 16) | rect.top);
    w.write(layer + regs::LAYER_BOTTOM_BLEND, (regs::CURSOR_BLEND_MODE << 16) | rect.bottom);
}

/// Latch the shadowed configuration for the next frame without restarting the scan (the
/// pointer's moves; `flush` also issues the software start the splash ends with).
pub fn latch<W: RegWriter>(w: &mut W) {
    w.write(regs::CTL2_CFG_READY, 1);
}

/// Lay a `w`×`h` premultiplied BGRA sprite into the 64×64 pointer buffer `out` (rows of
/// `regs::CURSOR_STRIDE_BYTES`), the rest transparent. `None` when the sprite is larger than
/// the buffer or `bgra` shorter than the sprite.
pub fn lay_cursor(out: &mut [u8], bgra: &[u8], w: u32, h: u32) -> Option<()> {
    if out.len() < regs::CURSOR_BYTES
        || w == 0
        || h == 0
        || w > regs::CURSOR_SIZE
        || h > regs::CURSOR_SIZE
    {
        return None;
    }
    let row_bytes = w as usize * 4;
    if bgra.len() < row_bytes * h as usize {
        return None;
    }
    out[..regs::CURSOR_BYTES].fill(0);
    let stride = regs::CURSOR_STRIDE_BYTES as usize;
    for row in 0..h as usize {
        let src = &bgra[row * row_bytes..row * row_bytes + row_bytes];
        out[row * stride..row * stride + row_bytes].copy_from_slice(src);
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::dc::model::{Sequence, Write};

    /// The pointer's layer as the stock system runs it (2026-10-05, the pointer at 1725,971
    /// with the buffer at a bank-1 bus address — the sample's own is a platform literal the scanner refuses): every word of the diff against the D0 dump, in the
    /// channel, the composer and the pipeline's enable word — and the latch without a restart.
    #[test]
    fn the_pointer_layer_leaves_the_measured_words() {
        let cursor = CursorPlane { bus_addr: 0x1_2345_6000 };
        let rect = cursor_rect(1725, 971, (0, 0), 1920, 1080).unwrap();
        let mut s = Sequence::new();
        cursor_layer_on(&mut s, &cursor, 1, &rect);
        for (offset, value) in [
            (0xc80, 0x001e_203c),
            (0xc84, 0x3cb),
            (0xca0, 0x2345_6000),
            (0xca4, 0x0000_0001),
            (0xcb8, 0x100),
            (0xcbc, 0x0040_0040),
            (0xcc0, 0),
            (0xcc4, 0x003f_003f),
            (0xcf0, 0x4),
            (0xcf8, 0x1000_0008),
            (0x4cf8, 0x5),
            (0x4d08, 0x0006_bd00),
            (0x4d0c, 0x06fc_03cb),
            (0x4d10, 0x0005_040a),
            (0x4d14, 0x00ff_000a),
            (0x560, 0x0004_0006),
            (regs::CTL2_CFG_READY, 1),
        ] {
            assert_eq!(s.value_at(offset), Some(value), "offset 0x{offset:x}");
        }
        assert_eq!(s.value_at(regs::CTL2_SW_START), Some(1), "armed like the plane switch");
        assert_eq!(s.value_at(0xd4c), None, "the channel's state words are not glue");
        assert_eq!(s.value_at(0xd60), None);
        assert_eq!(s.dropped, 0);
        // The enable word is written after the layer's words (the layer shows complete).
        let en = s.as_slice().iter().position(|w| w.offset == 0x4cf8).unwrap();
        let alpha = s.as_slice().iter().position(|w| w.offset == 0x4d14).unwrap();
        assert!(alpha < en);
    }

    /// A move is the channel's composer row, the crop, the three rectangle words and the latch —
    /// no restart;
    /// taking the layer off clears its enable word and the channel's enable bit.
    #[test]
    fn a_pointer_move_is_the_rectangle_and_the_latch() {
        let mut s = Sequence::new();
        cursor_move(&mut s, &cursor_rect(100, 50, (4, 2), 1920, 1080).unwrap());
        assert_eq!(
            s.as_slice(),
            &[
                Write { offset: 0xc84, value: 48 },
                Write { offset: 0xcc0, value: 0 },
                Write { offset: 0xcc4, value: 0x003f_003f },
                Write { offset: 0x4d08, value: 96 << 8 },
                Write { offset: 0x4d0c, value: (159 << 16) | 48 },
                Write { offset: 0x4d10, value: (5 << 16) | 111 },
                Write { offset: regs::CTL2_CFG_READY, value: 1 },
            ]
        );
        let mut off = Sequence::new();
        cursor_layer_off(&mut off, 1);
        assert_eq!(off.value_at(0x4cf8), Some(0));
        assert_eq!(off.value_at(0x560), Some(0x0004_0002));
    }

    /// The rectangle at the edges: a pointer near the left/top edge crops the buffer (the
    /// rectangle starts at 0, the crop names the cut), near the right/bottom edge the rectangle
    /// stops at the mode's last pixel; a sprite wholly off screen is no rectangle.
    #[test]
    fn the_pointer_rectangle_is_clipped_to_the_mode() {
        let r = cursor_rect(2, 1, (4, 4), 1920, 1080).unwrap();
        assert_eq!((r.left, r.top, r.crop_x, r.crop_y, r.right, r.bottom), (0, 0, 2, 3, 61, 60));
        let r = cursor_rect(1919, 1079, (0, 0), 1920, 1080).unwrap();
        assert_eq!((r.left, r.top, r.right, r.bottom, r.crop_x), (1919, 1079, 1919, 1079, 0));
        assert_eq!(cursor_rect(-64, 10, (0, 0), 1920, 1080), None);
        assert_eq!(cursor_rect(1920, 10, (0, 0), 1920, 1080), None);
        assert_eq!(cursor_rect(10, -70, (0, 0), 1920, 1080), None);
    }

    /// A 32×32 sprite lands in the buffer's top-left rows of 256 bytes, the rest transparent;
    /// a sprite larger than the buffer or shorter than it claims is refused.
    #[test]
    fn test_reject_a_sprite_the_pointer_buffer_cannot_hold() {
        let mut out = [0xaau8; regs::CURSOR_BYTES];
        let sprite: Vec<u8> = (0..32 * 32 * 4).map(|i| (i % 251) as u8).collect();
        assert_eq!(lay_cursor(&mut out, &sprite, 32, 32), Some(()));
        assert_eq!(&out[..128], &sprite[..128]);
        assert_eq!(&out[128..256], &[0u8; 128][..], "the row's rest transparent");
        assert_eq!(&out[256..384], &sprite[128..256], "the second row at the stride");
        assert!(out[32 * 256..].iter().all(|b| *b == 0), "rows below the sprite transparent");
        assert_eq!(lay_cursor(&mut out, &sprite, 65, 1), None);
        assert_eq!(lay_cursor(&mut out, &sprite[..100], 32, 32), None);
        assert_eq!(lay_cursor(&mut out[..100], &sprite, 32, 32), None);
        assert_eq!(lay_cursor(&mut out, &sprite, 0, 32), None);
    }
}
