// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Surface atlas allocator for the retained-surface GPU compositor
//! (TASK #8). Cached layer surfaces (base UI, sidebar, chat window, buttons)
//! live in the framebuffer VMO beyond the four display planes, so the compositor
//! can blit a layer to the display at any position without re-rendering it.
//!
//! Framebuffer VMO row layout (1280 px wide, BGRA8888):
//!   rows    0.. 799  Plane 0 — wallpaper source
//!   rows  800..1599  Plane 1 — retained base scene
//!   rows 1600..2399  Plane 2 — display (ring slot A)
//!   rows 2400..3199  Plane 3 — blur cache (ring slot B)
//!   rows 3200..6399  Atlas   — cached layer surfaces  ← this module
//!
//! Pure logic, no OS deps → host-testable. Surfaces are addressed by absolute
//! VMO row, consumed by the existing `BlitAbsolute` GPU command + `vmo_write`.
//!
//! OWNERS: @ui
//! STATUS: TASK #8 — retained-surface compositor
//! API_STABILITY: Unstable

/// Display width in pixels (atlas surfaces share the framebuffer stride).
pub(crate) const ATLAS_WIDTH: u32 = 1280;
/// First atlas row (immediately after the four display planes, each 800 rows).
pub(crate) const ATLAS_ROW_OFFSET: u32 = 3200;
/// Atlas height in rows. Sized to hold all cached layers full-width with slack:
/// base ~260 + sidebar ~764 + chat ~560 + buttons ~56 ≈ 1640 rows used.
pub(crate) const ATLAS_ROWS: u32 = 3200;
/// Total framebuffer-resource height including the atlas. windowd sizes the VMO
/// to this; gpud's `RESOURCE_HEIGHT` MUST match (separate crate, no shared dep).
pub(crate) const RESOURCE_HEIGHT: u32 = ATLAS_ROW_OFFSET + ATLAS_ROWS; // 6400

/// A cached layer surface: a full-width row band in the atlas region.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AtlasSurface {
    /// Absolute VMO start row of the surface (use as `src_y_abs` for BlitAbsolute).
    pub(crate) abs_row: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl AtlasSurface {
    /// Byte offset of the surface's first pixel within the framebuffer VMO.
    pub(crate) fn byte_offset(self, stride_bytes: usize) -> usize {
        self.abs_row as usize * stride_bytes
    }
}

/// Row-bump allocator over the atlas region. Each surface gets a full-width row
/// band. Allocation is one-shot at startup (layers are fixed) — no free list
/// needed, which suits windowd's non-freeing bump heap.
pub(crate) struct AtlasAllocator {
    next_row: u32,
}

impl AtlasAllocator {
    pub(crate) const fn new() -> Self {
        Self { next_row: ATLAS_ROW_OFFSET }
    }

    /// Reserve a surface `height` rows tall (full atlas width). Returns `None`
    /// if the atlas is exhausted.
    pub(crate) fn alloc(&mut self, width: u32, height: u32) -> Option<AtlasSurface> {
        if height == 0 || width == 0 || width > ATLAS_WIDTH {
            return None;
        }
        let end = self.next_row.checked_add(height)?;
        if end > ATLAS_ROW_OFFSET + ATLAS_ROWS {
            return None;
        }
        let surface = AtlasSurface { abs_row: self.next_row, width, height };
        self.next_row = end;
        Some(surface)
    }

    /// Rows still free in the atlas.
    pub(crate) fn rows_remaining(&self) -> u32 {
        (ATLAS_ROW_OFFSET + ATLAS_ROWS).saturating_sub(self.next_row)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_height_matches_layout() {
        assert_eq!(RESOURCE_HEIGHT, 6400);
        // Atlas starts right after the 4 display planes (4 × 800).
        assert_eq!(ATLAS_ROW_OFFSET, 4 * 800);
    }

    #[test]
    fn allocations_stack_and_stay_in_region() {
        let mut a = AtlasAllocator::new();
        let s0 = a.alloc(826, 260).expect("base");
        let s1 = a.alloc(320, 764).expect("sidebar");
        assert_eq!(s0.abs_row, ATLAS_ROW_OFFSET);
        assert_eq!(s1.abs_row, ATLAS_ROW_OFFSET + 260);
        // No overlap; both inside the atlas.
        assert!(s1.abs_row >= s0.abs_row + s0.height);
        assert!(s1.abs_row + s1.height <= ATLAS_ROW_OFFSET + ATLAS_ROWS);
    }

    #[test]
    fn exhaustion_returns_none() {
        let mut a = AtlasAllocator::new();
        assert!(a.alloc(1280, ATLAS_ROWS).is_some());
        assert!(a.alloc(1280, 1).is_none());
        assert_eq!(a.rows_remaining(), 0);
    }

    #[test]
    fn rejects_overwide_or_empty() {
        let mut a = AtlasAllocator::new();
        assert!(a.alloc(ATLAS_WIDTH + 1, 10).is_none());
        assert!(a.alloc(100, 0).is_none());
    }

    #[test]
    fn byte_offset_uses_absolute_row() {
        let s = AtlasSurface { abs_row: ATLAS_ROW_OFFSET, width: 100, height: 10 };
        assert_eq!(s.byte_offset(ATLAS_WIDTH as usize * 4), 3200 * 1280 * 4);
    }
}
