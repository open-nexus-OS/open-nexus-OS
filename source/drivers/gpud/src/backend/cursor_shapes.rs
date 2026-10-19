// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The pointer's shape cache — ONE for every display (TASK-0215 5d). windowd uploads
//! its shapes once (`OP_UPLOAD_CURSOR_SHAPE`) and switches between them by id
//! (`OP_SELECT_CURSOR_SHAPE`): on QEMU the CPU executor's sprite (`cpu_frame`) and the virtio
//! overlay take the bytes from here, on the board the controller's own pointer layer
//! (`backend/dc/cursor.rs`) does — the board's display has no CPU executor left to keep the
//! cache in. The sizes come from windowd's frames and are untrusted: a sprite shorter than its
//! size, an empty size, a slot outside the cache are refused.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: host (`test_reject_*` below); every QEMU lane with a pointer; the board ladder
//!   (`gpud: dc cursor layer ok (`)

use alloc::vec::Vec;

use nexus_gfx::backend::error::GfxError;

/// One cached cursor shape: `(premultiplied BGRA, w, h, hot_x, hot_y)`.
pub(crate) type CursorShapeEntry = (Vec<u8>, u32, u32, u32, u32);
/// A cached shape read: `(premultiplied BGRA, w, h, (hot_x, hot_y))`.
pub(crate) type CursorShapeRef<'a> = (&'a [u8], u32, u32, (u32, u32));

/// The bytes a `w`×`h` BGRA sprite takes, if `bgra` holds at least that many.
pub(crate) fn sprite_len(bgra: &[u8], w: u32, h: u32) -> Result<usize, GfxError> {
    let needed = (w as usize).saturating_mul(h as usize).saturating_mul(4);
    if needed == 0 || bgra.len() < needed {
        return Err(GfxError::InvalidArgument);
    }
    Ok(needed)
}

/// Pre-uploaded shapes: a shape change is a 2-byte select, not a blocking re-upload. Slot =
/// shape id.
pub(crate) struct CursorShapes {
    shapes: [Option<CursorShapeEntry>; nexus_display_proto::CURSOR_SHAPE_SLOTS],
}

impl CursorShapes {
    pub(crate) fn new() -> Self {
        Self { shapes: [const { None }; nexus_display_proto::CURSOR_SHAPE_SLOTS] }
    }

    /// Fill shape slot `id`; arms nothing (a select activates a slot). A re-upload reuses the
    /// slot's allocation: the service's heap never frees.
    pub(crate) fn cache(
        &mut self,
        id: u8,
        bgra: &[u8],
        w: u32,
        h: u32,
        hot: (u32, u32),
    ) -> Result<(), GfxError> {
        let slot = self.shapes.get_mut(id as usize).ok_or(GfxError::InvalidArgument)?;
        let needed = sprite_len(bgra, w, h)?;
        match slot {
            Some((buf, sw, sh, hx, hy)) => {
                buf.clear();
                buf.extend_from_slice(&bgra[..needed]);
                (*sw, *sh, *hx, *hy) = (w, h, hot.0, hot.1);
            }
            empty => {
                let mut buf = Vec::with_capacity(needed);
                buf.extend_from_slice(&bgra[..needed]);
                *empty = Some((buf, w, h, hot.0, hot.1));
            }
        }
        Ok(())
    }

    /// The cached shape `id`: its sprite, size and hotspot; `None` for an empty or absent slot.
    pub(crate) fn get(&self, id: u8) -> Option<CursorShapeRef<'_>> {
        let (buf, w, h, hx, hy) = self.shapes.get(id as usize)?.as_ref()?;
        Some((buf.as_slice(), *w, *h, (*hx, *hy)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sprite(w: u32, h: u32, fill: u8) -> Vec<u8> {
        alloc::vec![fill; (w * h * 4) as usize]
    }

    #[test]
    fn a_cached_shape_is_read_back_with_its_size_and_hotspot() {
        let mut shapes = CursorShapes::new();
        shapes.cache(3, &sprite(4, 2, 7), 4, 2, (1, 1)).unwrap();
        let (bytes, w, h, hot) = shapes.get(3).unwrap();
        assert_eq!((w, h, hot), (4, 2, (1, 1)));
        assert_eq!(bytes, sprite(4, 2, 7).as_slice());
        // A re-upload replaces the slot's bytes in place.
        shapes.cache(3, &sprite(1, 1, 9), 1, 1, (0, 0)).unwrap();
        assert_eq!(shapes.get(3).unwrap().1, 1);
    }

    /// The sizes come from windowd's frame — untrusted. A sprite shorter than its size, an empty
    /// size, a size whose byte count overflows: refused, the slot untouched.
    #[test]
    fn test_reject_a_sprite_shorter_than_its_size() {
        let mut shapes = CursorShapes::new();
        shapes.cache(0, &sprite(2, 2, 1), 2, 2, (0, 0)).unwrap();
        for (w, h) in [(3, 2), (0, 2), (2, 0), (u32::MAX, u32::MAX)] {
            assert!(shapes.cache(0, &sprite(2, 2, 5), w, h, (0, 0)).is_err(), "{w}x{h}");
        }
        assert_eq!(shapes.get(0).unwrap().0, sprite(2, 2, 1).as_slice());
    }

    #[test]
    fn test_reject_a_shape_slot_outside_the_cache() {
        let mut shapes = CursorShapes::new();
        let past = nexus_display_proto::CURSOR_SHAPE_SLOTS as u8;
        assert!(shapes.cache(past, &sprite(1, 1, 1), 1, 1, (0, 0)).is_err());
        assert!(shapes.get(past).is_none());
        assert!(shapes.get(0).is_none(), "an empty slot selects nothing");
    }
}
