// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The app ↔ windowd surface protocol v2 (RFC-0114 §3, ADR-0092): a surface is a buffer
//! gpud made for the app. windowd answers a create with `OP_SURFACE_BUFFERS` — the object's
//! geometry, its capability moved with the frame — the app draws a slot where the GPU reads and
//! presents it by number (`OP_SURFACE_PRESENT` with its slot and the changed rects), and the
//! present's ack is the release: windowd sends it once every other slot is free (no frame still
//! reads it), so the app draws the next frame into one. One present in flight per surface, as
//! before — now meaning one slot ahead.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: the goldens and `test_reject_*` below; windowd's `surface_release` host tests
//! INVARIANTS: a slot is `rows` rows of `stride` bytes, slot `s` at row `s × rows` (the layout of
//!   `crate::surface_alloc`, the one SSOT); an OK answer names a surface and a geometry that holds
//!   its width; a present names a slot below `SLOTS_MAX` and at most `MAX_DAMAGE_RECTS` rects.

use crate::client_surface::{DamageRect, MAX_DAMAGE_RECTS};
use crate::envelope::{has_op, header, HEADER_LEN};
use crate::surface_alloc::{object_bytes, slot_offset, SLOTS_MAX, STRIDE_ALIGN};

/// windowd → app: the answer to `OP_SURFACE_CREATE` — the surface's buffer (`Buffers`), its
/// capability moved with the frame when `status` is OK. A refusal carries no capability.
pub const OP_SURFACE_BUFFERS: u8 = 38;

pub const SURFACE_BUFFERS_FRAME_LEN: usize = HEADER_LEN + 16;

/// The surface's buffer as the app draws it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Buffers {
    /// `SURFACE_STATUS_*`: OK, or why the create was refused (`QUOTA`: no memory for it).
    pub status: u8,
    /// The surface's id (`0` in a refusal).
    pub surface_id: u32,
    /// Bytes from one row to the next (a multiple of `STRIDE_ALIGN`).
    pub stride: u32,
    /// The widest content a slot holds (the size class — at least the surface's width).
    pub width: u16,
    /// Rows per slot: the base's and the store's, rounded up to the class.
    pub rows: u16,
    /// Slots in the object (two for an app the CPU draws).
    pub slots: u8,
    /// The cache line the app cleans its writes in (`vmo_clean`); `0` = the GPU snoops the caches.
    pub cache_block: u16,
}

impl Buffers {
    /// A refusal: no surface, no capability.
    #[must_use]
    pub const fn refused(status: u8) -> Self {
        Self { status, surface_id: 0, stride: 0, width: 0, rows: 0, slots: 0, cache_block: 0 }
    }

    /// The object's bytes: every slot, whole pages — what the app maps.
    #[must_use]
    pub const fn object_bytes(&self) -> usize {
        object_bytes(self.stride, self.rows, self.slots)
    }

    /// Slot `slot`'s first byte.
    #[must_use]
    pub const fn slot_offset(&self, slot: u8) -> usize {
        slot_offset(self.stride, self.rows, slot)
    }
}

#[must_use]
pub fn encode_surface_buffers(b: &Buffers) -> [u8; SURFACE_BUFFERS_FRAME_LEN] {
    let mut f = [0u8; SURFACE_BUFFERS_FRAME_LEN];
    f[..HEADER_LEN].copy_from_slice(&header(OP_SURFACE_BUFFERS));
    f[4] = b.status;
    f[5..9].copy_from_slice(&b.surface_id.to_le_bytes());
    f[9..13].copy_from_slice(&b.stride.to_le_bytes());
    f[13..15].copy_from_slice(&b.width.to_le_bytes());
    f[15..17].copy_from_slice(&b.rows.to_le_bytes());
    f[17] = b.slots;
    f[18..20].copy_from_slice(&b.cache_block.to_le_bytes());
    f
}

/// Length-exact. An OK answer names a surface, a stride at the alignment that holds its width,
/// rows, and one to `SLOTS_MAX` slots; a refusal names no surface.
#[must_use]
pub fn decode_surface_buffers(frame: &[u8]) -> Option<Buffers> {
    if !has_op(frame, OP_SURFACE_BUFFERS) || frame.len() != SURFACE_BUFFERS_FRAME_LEN {
        return None;
    }
    let u16_at = |at: usize| u16::from_le_bytes([frame[at], frame[at + 1]]);
    let u32_at =
        |at: usize| u32::from_le_bytes([frame[at], frame[at + 1], frame[at + 2], frame[at + 3]]);
    let b = Buffers {
        status: frame[4],
        surface_id: u32_at(5),
        stride: u32_at(9),
        width: u16_at(13),
        rows: u16_at(15),
        slots: frame[17],
        cache_block: u16_at(18),
    };
    let sane = if b.status == crate::client_surface::SURFACE_STATUS_OK {
        b.surface_id != 0
            && b.stride != 0
            && b.stride % STRIDE_ALIGN == 0
            && b.stride >= u32::from(b.width) * 4
            && b.width != 0
            && b.rows != 0
            && (1..=SLOTS_MAX).contains(&b.slots)
    } else {
        b.surface_id == 0
    };
    sane.then_some(b)
}

// ----------------------------------------------------------------- present

pub const SURFACE_PRESENT_MAX_LEN: usize = HEADER_LEN + 10 + MAX_DAMAGE_RECTS * 8;

/// A present (v2): the slot the app drew and what changed in it since the surface's last present.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfacePresent {
    pub surface_id: u32,
    pub seq: u32,
    pub slot: u8,
    rects: [DamageRect; MAX_DAMAGE_RECTS],
    count: usize,
}

impl SurfacePresent {
    /// The changed rects (surface pixels: the base's rows, then the store's — RFC-0109).
    #[must_use]
    pub fn damage(&self) -> &[DamageRect] {
        &self.rects[..self.count]
    }
}

/// Encodes a present of slot `slot`; `damage` is clamped to [`MAX_DAMAGE_RECTS`]. Returns the
/// frame length.
#[must_use]
pub fn encode_surface_present(
    surface_id: u32,
    seq: u32,
    slot: u8,
    damage: &[DamageRect],
    out: &mut [u8; SURFACE_PRESENT_MAX_LEN],
) -> usize {
    let count = damage.len().min(MAX_DAMAGE_RECTS);
    out[..HEADER_LEN].copy_from_slice(&header(crate::client_surface::OP_SURFACE_PRESENT));
    out[4..8].copy_from_slice(&surface_id.to_le_bytes());
    out[8..12].copy_from_slice(&seq.to_le_bytes());
    out[12] = slot;
    out[13] = count as u8;
    let mut pos = 14;
    for rect in &damage[..count] {
        out[pos..pos + 2].copy_from_slice(&rect.x.to_le_bytes());
        out[pos + 2..pos + 4].copy_from_slice(&rect.y.to_le_bytes());
        out[pos + 4..pos + 6].copy_from_slice(&rect.width.to_le_bytes());
        out[pos + 6..pos + 8].copy_from_slice(&rect.height.to_le_bytes());
        pos += 8;
    }
    pos
}

/// Count, slot and length strictly validated: a slot at or past `SLOTS_MAX` is malformed here,
/// one past the surface's own slots where the surface is known (windowd).
#[must_use]
pub fn decode_surface_present(frame: &[u8]) -> Option<SurfacePresent> {
    use crate::client_surface::OP_SURFACE_PRESENT;
    if !has_op(frame, OP_SURFACE_PRESENT) || frame.len() < HEADER_LEN + 10 {
        return None;
    }
    let u32_at =
        |at: usize| u32::from_le_bytes([frame[at], frame[at + 1], frame[at + 2], frame[at + 3]]);
    let (slot, count) = (frame[12], usize::from(frame[13]));
    if slot >= SLOTS_MAX || count > MAX_DAMAGE_RECTS || frame.len() != HEADER_LEN + 10 + count * 8 {
        return None;
    }
    let u16_at = |at: usize| u16::from_le_bytes([frame[at], frame[at + 1]]);
    let mut rects = [DamageRect { x: 0, y: 0, width: 0, height: 0 }; MAX_DAMAGE_RECTS];
    for (i, rect) in rects.iter_mut().enumerate().take(count) {
        let pos = 14 + i * 8;
        *rect = DamageRect {
            x: u16_at(pos),
            y: u16_at(pos + 2),
            width: u16_at(pos + 4),
            height: u16_at(pos + 6),
        };
    }
    Some(SurfacePresent { surface_id: u32_at(4), seq: u32_at(8), slot, rects, count })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client_surface::{SURFACE_STATUS_OK, SURFACE_STATUS_QUOTA};
    use crate::surface_alloc::stride_for;

    const OK: Buffers = Buffers {
        status: SURFACE_STATUS_OK,
        surface_id: 7,
        stride: stride_for(1472),
        width: 1472,
        rows: 2432,
        slots: 2,
        cache_block: 64,
    };

    #[test]
    fn the_buffers_answer_round_trips_and_names_the_layout() {
        let f = encode_surface_buffers(&OK);
        assert_eq!(decode_surface_buffers(&f), Some(OK));
        assert_eq!(OK.slot_offset(1), stride_for(1472) as usize * 2432);
        assert!(OK.object_bytes() >= 2 * OK.slot_offset(1));
        assert_eq!(OK.object_bytes() % 4096, 0);
        let refused = Buffers::refused(SURFACE_STATUS_QUOTA);
        assert_eq!(decode_surface_buffers(&encode_surface_buffers(&refused)), Some(refused));
    }

    #[test]
    fn test_reject_buffers_that_cannot_hold_the_surface() {
        let bad = |f: &dyn Fn(&mut Buffers)| {
            let mut b = OK;
            f(&mut b);
            decode_surface_buffers(&encode_surface_buffers(&b))
        };
        assert_eq!(bad(&|b| b.surface_id = 0), None, "an OK answer names its surface");
        assert_eq!(bad(&|b| b.stride = 5000), None, "a stride off the alignment");
        assert_eq!(bad(&|b| b.stride = 1024), None, "a stride narrower than the width");
        assert_eq!(bad(&|b| b.rows = 0), None);
        assert_eq!(bad(&|b| b.slots = 0), None);
        assert_eq!(bad(&|b| b.slots = SLOTS_MAX + 1), None);
        let mut refusal = Buffers::refused(SURFACE_STATUS_QUOTA);
        refusal.surface_id = 3;
        assert_eq!(decode_surface_buffers(&encode_surface_buffers(&refusal)), None);
        let f = encode_surface_buffers(&OK);
        assert_eq!(decode_surface_buffers(&f[..f.len() - 1]), None, "short");
    }

    #[test]
    fn a_present_names_its_slot_and_its_rects() {
        let damage = [
            DamageRect { x: 0, y: 0, width: 320, height: 240 },
            DamageRect { x: 10, y: 20, width: 30, height: 40 },
        ];
        let mut buf = [0u8; SURFACE_PRESENT_MAX_LEN];
        let len = encode_surface_present(7, 3, 1, &damage, &mut buf);
        let p = decode_surface_present(&buf[..len]).expect("decodes");
        assert_eq!((p.surface_id, p.seq, p.slot), (7, 3, 1));
        assert_eq!(p.damage(), &damage);
    }

    #[test]
    fn test_reject_present_slot_out_of_range() {
        let mut buf = [0u8; SURFACE_PRESENT_MAX_LEN];
        let len = encode_surface_present(7, 3, SLOTS_MAX, &[], &mut buf);
        assert_eq!(decode_surface_present(&buf[..len]), None, "past every object's slots");
        let len = encode_surface_present(7, 3, 1, &[], &mut buf);
        assert!(decode_surface_present(&buf[..len]).is_some());
        // Truncated and over-count frames are rejected.
        let damage = [DamageRect { x: 0, y: 0, width: 1, height: 1 }];
        let len = encode_surface_present(7, 3, 0, &damage, &mut buf);
        assert_eq!(decode_surface_present(&buf[..len - 1]), None);
        let mut over = buf;
        over[13] = (MAX_DAMAGE_RECTS + 1) as u8;
        assert_eq!(decode_surface_present(&over[..len]), None);
    }

    #[test]
    fn the_buffers_op_is_its_own() {
        use crate::client_surface as cs;
        for op in [
            cs::OP_SURFACE_CREATE,
            cs::OP_SURFACE_PRESENT,
            cs::OP_SURFACE_DESTROY,
            cs::OP_SURFACE_INPUT,
            cs::OP_SURFACE_EVENTS,
            crate::surface_caret::OP_SURFACE_CARET,
            crate::surface_pressure::OP_SURFACE_PRESSURE,
            crate::surface_seen::OP_SURFACE_SEEN,
        ] {
            assert_ne!(op, OP_SURFACE_BUFFERS);
        }
        assert!(OP_SURFACE_BUFFERS > 7, "never an input-family op");
    }
}
