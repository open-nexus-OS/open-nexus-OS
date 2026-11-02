// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Surface buffers for an owner, windowd → gpud (RFC-0114 §1–§2, ADR-0092). gpud is the
//! one allocator of memory the GPU reads: windowd asks for an app surface's object on the app's
//! behalf (the owner is the app's KERNEL id, which windowd took from the app's create — gpud
//! serves windowd only), gpud makes one GPU-reachable object of `slots` slots of the RFC-0109
//! surface layout at a 256-byte stride, answers its geometry and moves its capability. The ask
//! carries a cookie the answer names: windowd never waits on gpud (ADR-0075) — the answer
//! arrives on its gpud response route among the present acks, tagged so the two never mix.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal v1
//! TEST_COVERAGE: the goldens and `test_reject_*` below
//! INVARIANTS: a slot is `rows` rows of `stride` bytes, slot `s` at row `s × rows`; the stride
//!   holds the width and is a multiple of `STRIDE_ALIGN`; an object is whole pages; a size is
//!   rounded up to its class (`WIDTH_STEP` × `ROWS_STEP`) so a resize inside it reuses the
//!   object; no width past the layout's, no rows past `MAX_SURFACE_ROWS`, no more than
//!   `SLOTS_MAX` slots, no usage gpud does not know.

use crate::layout::LAYOUT_MAX;
use crate::surface_layers::MAX_SURFACE_ROWS;

/// windowd → gpud: an app surface's object (`AllocRequest`); answered by `Allocated` + the cap.
pub const OP_SURFACE_ALLOC: u8 = 18;
/// windowd → gpud: the object is no longer composed (`buffer: u32`); gpud unbinds it once the
/// last frame that read it retired and ends the charge. No answer.
pub const OP_SURFACE_FREE: u8 = 19;
/// RFC-0112 Phase 3 (reserved): a restarted compositor adopts the objects gpud kept.
pub const OP_SURFACE_ADOPT: u8 = 20;

/// The answer's first byte: a present ack starts with a `STATUS_*` (below `0x80`), so the two
/// never mix on windowd's gpud response route.
pub const ALLOCATED_TAG: u8 = 0x80 | OP_SURFACE_ALLOC;

/// Slots an object may hold: two for an app the CPU draws, three for one the GPU draws.
pub const SLOTS_MAX: u8 = 3;
/// `usage` bit 0: the GPU samples the surface (every surface).
pub const USAGE_SAMPLE: u8 = 1;
/// `usage` bit 1: a display plane may scan a slot out whole (reserved: direct scanout).
pub const USAGE_SCANOUT: u8 = 2;
/// Every usage bit gpud knows.
const USAGE_KNOWN: u8 = USAGE_SAMPLE | USAGE_SCANOUT;

/// A slot's row stride is a multiple of this many bytes (64 px: safe for every texture unit and
/// display plane we drive).
pub const STRIDE_ALIGN: u32 = 256;
/// A size class's steps: a resize inside the class reuses the object.
pub const WIDTH_STEP: u16 = 64;
pub const ROWS_STEP: u16 = 64;
const PAGE: usize = 4096;

/// `OP_SURFACE_ALLOC`'s request: `[op, cookie u32, owner_sid u64, width u16, rows u16, slots u8,
/// usage u8]`.
pub const ALLOC_REQUEST_LEN: usize = 19;
/// The answer: `[ALLOCATED_TAG, cookie u32, status u8, buffer u32, stride u32, width u16,
/// rows u16, slots u8, cache_block u16]`.
pub const ALLOCATED_LEN: usize = 21;
/// `OP_SURFACE_FREE`: `[op, buffer u32]`.
pub const FREE_LEN: usize = 5;

/// windowd's ask for an app surface's object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AllocRequest {
    /// windowd's tag, named by the answer.
    pub cookie: u32,
    /// The app's kernel service id (from its create): the object's owner, charged for it.
    pub owner_sid: u64,
    /// The surface's width and its rows per slot (base + store), before the size class.
    pub width: u16,
    pub rows: u16,
    pub slots: u8,
    pub usage: u8,
}

/// gpud's answer: the object's geometry (its capability rides with it when `status` is OK).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Allocated {
    pub cookie: u32,
    pub status: u8,
    /// gpud's id of the object in windowd's context (never 0 when OK; a layer's `source`).
    pub buffer: u32,
    /// Bytes from one row to the next.
    pub stride: u32,
    /// The size class allocated: the widest width and the most rows per slot it holds.
    pub width: u16,
    pub rows: u16,
    pub slots: u8,
    /// The cache line the writer cleans in (`vmo_clean`); `0` = the GPU snoops the caches.
    pub cache_block: u16,
}

/// The size class `(width, rows)` is allocated as: each rounded up to its step, within bounds.
#[must_use]
pub fn size_class(width: u16, rows: u16) -> (u16, u16) {
    let up = |v: u16, step: u16, max: u16| v.div_ceil(step).saturating_mul(step).min(max).max(v);
    (up(width, WIDTH_STEP, LAYOUT_MAX.0 as u16), up(rows, ROWS_STEP, MAX_SURFACE_ROWS))
}

/// A slot's row stride for `width` pixels.
#[must_use]
pub const fn stride_for(width: u16) -> u32 {
    (width as u32 * 4).div_ceil(STRIDE_ALIGN) * STRIDE_ALIGN
}

/// Byte offset of slot `slot` of an object of `rows` rows per slot at `stride`.
#[must_use]
pub const fn slot_offset(stride: u32, rows: u16, slot: u8) -> usize {
    slot as usize * rows as usize * stride as usize
}

/// The object's length: `slots` slots, whole pages.
#[must_use]
pub const fn object_bytes(stride: u32, rows: u16, slots: u8) -> usize {
    slot_offset(stride, rows, slots).div_ceil(PAGE) * PAGE
}

#[must_use]
pub fn encode_alloc_request(r: &AllocRequest) -> [u8; ALLOC_REQUEST_LEN] {
    let mut f = [0u8; ALLOC_REQUEST_LEN];
    f[0] = OP_SURFACE_ALLOC;
    f[1..5].copy_from_slice(&r.cookie.to_le_bytes());
    f[5..13].copy_from_slice(&r.owner_sid.to_le_bytes());
    f[13..15].copy_from_slice(&r.width.to_le_bytes());
    f[15..17].copy_from_slice(&r.rows.to_le_bytes());
    f[17] = r.slots;
    f[18] = r.usage;
    f
}

/// Length-exact; refuses a zero size, a width past the layout's, rows past `MAX_SURFACE_ROWS`,
/// no slot or more than `SLOTS_MAX`, a usage without `USAGE_SAMPLE` or with a bit gpud does not
/// know, and an owner of 0 (the kernel names every process).
#[must_use]
pub fn decode_alloc_request(frame: &[u8]) -> Option<AllocRequest> {
    if frame.len() != ALLOC_REQUEST_LEN || frame[0] != OP_SURFACE_ALLOC {
        return None;
    }
    let u16_at = |at: usize| u16::from_le_bytes([frame[at], frame[at + 1]]);
    let mut sid = [0u8; 8];
    sid.copy_from_slice(&frame[5..13]);
    let r = AllocRequest {
        cookie: u32::from_le_bytes([frame[1], frame[2], frame[3], frame[4]]),
        owner_sid: u64::from_le_bytes(sid),
        width: u16_at(13),
        rows: u16_at(15),
        slots: frame[17],
        usage: frame[18],
    };
    let sane = r.width != 0
        && u32::from(r.width) <= LAYOUT_MAX.0
        && r.rows != 0
        && r.rows <= MAX_SURFACE_ROWS
        && (1..=SLOTS_MAX).contains(&r.slots)
        && r.usage & USAGE_SAMPLE != 0
        && r.usage & !USAGE_KNOWN == 0
        && r.owner_sid != 0;
    sane.then_some(r)
}

#[must_use]
pub fn encode_allocated(a: &Allocated) -> [u8; ALLOCATED_LEN] {
    let mut f = [0u8; ALLOCATED_LEN];
    f[0] = ALLOCATED_TAG;
    f[1..5].copy_from_slice(&a.cookie.to_le_bytes());
    f[5] = a.status;
    f[6..10].copy_from_slice(&a.buffer.to_le_bytes());
    f[10..14].copy_from_slice(&a.stride.to_le_bytes());
    f[14..16].copy_from_slice(&a.width.to_le_bytes());
    f[16..18].copy_from_slice(&a.rows.to_le_bytes());
    f[18] = a.slots;
    f[19..21].copy_from_slice(&a.cache_block.to_le_bytes());
    f
}

/// Length-exact; an OK answer must name a buffer, a stride that holds its width at the
/// alignment, and slots within bounds.
#[must_use]
pub fn decode_allocated(frame: &[u8]) -> Option<Allocated> {
    if frame.len() != ALLOCATED_LEN || frame[0] != ALLOCATED_TAG {
        return None;
    }
    let u16_at = |at: usize| u16::from_le_bytes([frame[at], frame[at + 1]]);
    let u32_at =
        |at: usize| u32::from_le_bytes([frame[at], frame[at + 1], frame[at + 2], frame[at + 3]]);
    let a = Allocated {
        cookie: u32_at(1),
        status: frame[5],
        buffer: u32_at(6),
        stride: u32_at(10),
        width: u16_at(14),
        rows: u16_at(16),
        slots: frame[18],
        cache_block: u16_at(19),
    };
    let ok = a.status != crate::STATUS_OK
        || (a.buffer != 0
            && a.stride % STRIDE_ALIGN == 0
            && a.stride >= u32::from(a.width) * 4
            && a.rows != 0
            && (1..=SLOTS_MAX).contains(&a.slots));
    ok.then_some(a)
}

#[must_use]
pub fn encode_free(buffer: u32) -> [u8; FREE_LEN] {
    let mut f = [0u8; FREE_LEN];
    f[0] = OP_SURFACE_FREE;
    f[1..5].copy_from_slice(&buffer.to_le_bytes());
    f
}

#[must_use]
pub fn decode_free(frame: &[u8]) -> Option<u32> {
    (frame.len() == FREE_LEN && frame[0] == OP_SURFACE_FREE)
        .then(|| u32::from_le_bytes([frame[1], frame[2], frame[3], frame[4]]))
        .filter(|b| *b != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const REQ: AllocRequest =
        AllocRequest { cookie: 9, owner_sid: 0xA11CE, width: 1440, rows: 2389, slots: 2, usage: 1 };

    #[test]
    fn the_request_and_the_answer_round_trip() {
        assert_eq!(decode_alloc_request(&encode_alloc_request(&REQ)), Some(REQ));
        let a = Allocated {
            cookie: 9,
            status: crate::STATUS_OK,
            buffer: 3,
            stride: stride_for(1472),
            width: 1472,
            rows: 2432,
            slots: 2,
            cache_block: 64,
        };
        assert_eq!(decode_allocated(&encode_allocated(&a)), Some(a));
        assert_eq!(decode_free(&encode_free(3)), Some(3));
        assert!(ALLOCATED_TAG >= 0x80, "never a present ack's status");
    }

    #[test]
    fn a_size_rounds_up_to_its_class_and_a_slot_is_rows_of_the_stride() {
        assert_eq!(size_class(1440, 2389), (1472, 2432));
        assert_eq!(size_class(1920, 3072), (1920, 3072), "the bounds are a class of their own");
        assert_eq!(size_class(1, 1), (64, 64));
        assert_eq!(stride_for(1440), 5888, "1440 px = 5760 B, up to 256");
        assert_eq!(stride_for(1920), 7680, "the layout's own stride");
        assert_eq!(slot_offset(5888, 2432, 1), 5888 * 2432);
        assert_eq!(object_bytes(5888, 2432, 2) % 4096, 0);
        assert!(object_bytes(5888, 2432, 2) >= 2 * 5888 * 2432);
    }

    #[test]
    fn test_reject_alloc_request_out_of_bounds() {
        let bad = |f: &dyn Fn(&mut AllocRequest)| {
            let mut r = REQ;
            f(&mut r);
            decode_alloc_request(&encode_alloc_request(&r))
        };
        assert_eq!(bad(&|r| r.width = 0), None);
        assert_eq!(bad(&|r| r.width = 1921), None, "wider than the layout");
        assert_eq!(bad(&|r| r.rows = 0), None);
        assert_eq!(bad(&|r| r.rows = MAX_SURFACE_ROWS + 1), None);
        assert_eq!(bad(&|r| r.slots = 0), None);
        assert_eq!(bad(&|r| r.slots = SLOTS_MAX + 1), None);
        assert_eq!(bad(&|r| r.usage = USAGE_SCANOUT), None, "every surface is sampled");
        assert_eq!(bad(&|r| r.usage = USAGE_SAMPLE | 0x80), None, "an unknown usage");
        assert_eq!(bad(&|r| r.owner_sid = 0), None, "no owner");
        let mut long = encode_alloc_request(&REQ).to_vec();
        long.push(0);
        assert_eq!(decode_alloc_request(&long), None);
    }

    #[test]
    fn test_reject_allocated_that_cannot_hold_its_width() {
        let a = Allocated {
            cookie: 1,
            status: crate::STATUS_OK,
            buffer: 2,
            stride: 1024,
            width: 512,
            rows: 64,
            slots: 2,
            cache_block: 0,
        };
        assert_eq!(decode_allocated(&encode_allocated(&a)), None, "512 px need 2048 B");
        assert_eq!(
            decode_allocated(&encode_allocated(&Allocated { buffer: 0, stride: 2048, ..a })),
            None
        );
        // A refusal carries no geometry to check.
        let refused = Allocated { status: crate::STATUS_QUOTA, buffer: 0, stride: 0, ..a };
        assert_eq!(decode_allocated(&encode_allocated(&refused)), Some(refused));
        assert_eq!(decode_free(&encode_free(0)), None, "buffer 0 is the layout");
    }
}
