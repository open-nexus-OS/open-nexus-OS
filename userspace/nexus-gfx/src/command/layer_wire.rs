// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The `CompositeLayer` record on the scene wire (split out of `buffer.rs` under the
//! structure ratchet, RFC-0114 §6): tag 9 and 21 little-endian words — the 20 of RFC-0101/0109
//! (signed `shadow_offset_y` bit-cast through u32) and the layer's SOURCE (0 = the shared layout
//! object; else a gpud surface buffer the app draws into) — and the SLOT of a buffer the layer
//! samples, carried in `flags` bits 8–15. A buffer layer's rows and columns are the slot's own.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `buffer_wire_tests` (the record's goldens and its round trip)
//! ADR: docs/adr/0092-a-surface-is-a-buffer-the-app-draws-and-the-gpu-reads-gpud-allocates-it-for-its-owner-windowd-composes-by-reference.md

use super::buffer::Command;
use crate::core::error::GfxError;

/// The record's tag.
pub(super) const TAG_COMPOSITE_LAYER: u8 = 9;
/// Little-endian words after the tag.
const WORDS: usize = 21;
/// The record's length on the wire.
pub(super) const RECORD_LEN: usize = 1 + WORDS * 4;

/// `CompositeLayer::flags` bits 8–15: the slot of the surface buffer a layer samples.
pub const LAYER_SLOT_SHIFT: u32 = 8;
/// The slot's bits of `CompositeLayer::flags`.
pub const LAYER_SLOT_MASK: u32 = 0xff << LAYER_SLOT_SHIFT;

/// The slot `flags` names.
#[must_use]
pub const fn layer_slot(flags: u32) -> u8 {
    ((flags & LAYER_SLOT_MASK) >> LAYER_SLOT_SHIFT) as u8
}

/// `flags` naming `slot` (every other bit kept).
#[must_use]
pub const fn with_slot(flags: u32, slot: u8) -> u32 {
    (flags & !LAYER_SLOT_MASK) | ((slot as u32) << LAYER_SLOT_SHIFT)
}

/// A layer's `key` (RFC-0109, RFC-0114 §7): `owner << 16 | kind << 8 | index` — the window's
/// wire key, what of the window the layer is, and which one of those. gpud reads a window's body
/// off the key after a compositor restart (RFC-0112 Phase 3: the ghost's place on the screen).
#[must_use]
pub const fn layer_key(owner: u32, kind: u32, index: u32) -> u32 {
    (owner << 16) | ((kind & 0xff) << 8) | (index & 0xff)
}

/// The window key a layer `key` names.
#[must_use]
pub const fn layer_key_owner(key: u32) -> u16 {
    (key >> 16) as u16
}

/// What of its window a layer `key` is (`LAYER_KIND_BODY` …).
#[must_use]
pub const fn layer_key_kind(key: u32) -> u32 {
    (key >> 8) & 0xff
}

/// Layer kinds in a key: the window's body (its surface's base layer).
pub const LAYER_KIND_BODY: u32 = 1;
pub const LAYER_KIND_REGION: u32 = 2;
pub const LAYER_KIND_SLICE: u32 = 3;
pub const LAYER_KIND_WASH: u32 = 4;
pub const LAYER_KIND_SOLID: u32 = 5;
pub const LAYER_KIND_BAR: u32 = 6;
pub const LAYER_KIND_CARET: u32 = 7;

/// Writes the record of `c` (a `CompositeLayer`) at `pos`; the position after it. `limit`: the
/// serialized buffer's bound.
pub(super) fn ser(
    buf: &mut [u8],
    pos: usize,
    c: &Command,
    limit: usize,
) -> Result<usize, GfxError> {
    let Command::CompositeLayer {
        src_row_abs,
        src_x,
        width,
        height,
        dst_x,
        dst_y,
        opacity,
        corner_radius,
        shadow_blur,
        shadow_offset_y,
        shadow_alpha,
        backdrop_blur,
        content_w,
        content_h,
        scroll_band_top_abs,
        scroll_band_h,
        layer_id,
        content_epoch,
        flags,
        key,
        source,
    } = c
    else {
        return Err(GfxError::InvalidArgument);
    };
    let words: [u32; WORDS] = [
        *src_row_abs,
        *src_x,
        *width,
        *height,
        *dst_x,
        *dst_y,
        *opacity,
        *corner_radius,
        *shadow_blur,
        *shadow_offset_y as u32,
        *shadow_alpha,
        *backdrop_blur,
        *content_w,
        *content_h,
        *scroll_band_top_abs,
        *scroll_band_h,
        *layer_id,
        *content_epoch,
        *flags,
        *key,
        *source,
    ];
    let needed = pos + RECORD_LEN;
    if buf.len() < needed || needed > limit {
        return Err(GfxError::ResourceExhausted);
    }
    buf[pos] = TAG_COMPOSITE_LAYER;
    let mut p = pos + 1;
    for w in words {
        buf[p..p + 4].copy_from_slice(&w.to_le_bytes());
        p += 4;
    }
    Ok(p)
}

/// Reads the record whose words start at `pos` (after the tag); the position after it.
pub(super) fn deser(buf: &[u8], pos: usize) -> Result<(Command, usize), GfxError> {
    let bytes = buf.get(pos..pos + WORDS * 4).ok_or(GfxError::InvalidArgument)?;
    let mut w = [0u32; WORDS];
    for (i, slot) in w.iter_mut().enumerate() {
        let b = &bytes[i * 4..i * 4 + 4];
        *slot = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    }
    Ok((
        Command::CompositeLayer {
            src_row_abs: w[0],
            src_x: w[1],
            width: w[2],
            height: w[3],
            dst_x: w[4],
            dst_y: w[5],
            opacity: w[6],
            corner_radius: w[7],
            shadow_blur: w[8],
            shadow_offset_y: w[9] as i32,
            shadow_alpha: w[10],
            backdrop_blur: w[11],
            content_w: w[12],
            content_h: w[13],
            scroll_band_top_abs: w[14],
            scroll_band_h: w[15],
            layer_id: w[16],
            content_epoch: w[17],
            flags: w[18],
            key: w[19],
            source: w[20],
        },
        pos + WORDS * 4,
    ))
}

#[cfg(test)]
mod key_tests {
    use super::*;

    #[test]
    fn a_layer_key_names_its_window_its_kind_and_its_index() {
        let key = layer_key(0x1234, LAYER_KIND_BODY, 0);
        assert_eq!(key, 0x1234_0100);
        assert_eq!(layer_key_owner(key), 0x1234);
        assert_eq!(layer_key_kind(key), LAYER_KIND_BODY);
        let desk = layer_key(0xFFFF, LAYER_KIND_REGION, 0x1FF);
        assert_eq!((layer_key_owner(desk), layer_key_kind(desk), desk & 0xff), (0xFFFF, 2, 0xff));
    }
}
