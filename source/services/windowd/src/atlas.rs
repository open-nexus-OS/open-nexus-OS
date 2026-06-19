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

/// A freed row band available for reuse: `[row, row + rows)` in the atlas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FreeSpan {
    row: u32,
    rows: u32,
}

/// Max distinct freed bands tracked at once. Windows acquire/release surfaces on
/// show/hide, so only a handful of bands are ever free simultaneously; freed
/// bands are coalesced (and the high-water tail reclaimed) on every `free`, so
/// fragmentation stays bounded well under this cap.
const MAX_FREE_SPANS: usize = 16;

/// Row allocator over the atlas region. Each surface gets a full-width row band.
/// A high-water bump pointer (`next_row`) hands out fresh bands; freed bands go
/// to a small free list that `alloc` reuses (first-fit + split) before bumping.
/// On `free`, adjacent free bands are coalesced and any band touching the
/// high-water mark is folded back into the bump tail — so a window can be shown,
/// hidden, and shown again without permanently consuming atlas rows, and the
/// boot sequence (all `alloc`s before any `free`) behaves exactly like the old
/// pure-bump allocator.
pub(crate) struct AtlasAllocator {
    next_row: u32,
    free: [FreeSpan; MAX_FREE_SPANS],
    free_len: usize,
}

impl AtlasAllocator {
    pub(crate) const fn new() -> Self {
        Self {
            next_row: ATLAS_ROW_OFFSET,
            free: [FreeSpan { row: 0, rows: 0 }; MAX_FREE_SPANS],
            free_len: 0,
        }
    }

    /// Reserve a surface `height` rows tall (full atlas width). Reuses a freed
    /// band when one fits (first-fit, splitting the remainder back onto the free
    /// list); otherwise bumps the high-water mark. Returns `None` if neither a
    /// free band nor the remaining tail can satisfy the request.
    pub(crate) fn alloc(&mut self, width: u32, height: u32) -> Option<AtlasSurface> {
        if height == 0 || width == 0 || width > ATLAS_WIDTH {
            return None;
        }
        // First-fit over the free list (reuse before growing the high-water mark).
        for i in 0..self.free_len {
            if self.free[i].rows >= height {
                let row = self.free[i].row;
                if self.free[i].rows == height {
                    self.remove_span(i);
                } else {
                    // Split: keep the remainder as a free band below the surface.
                    self.free[i].row += height;
                    self.free[i].rows -= height;
                }
                return Some(AtlasSurface { abs_row: row, width, height });
            }
        }
        // Bump the high-water mark.
        let end = self.next_row.checked_add(height)?;
        if end > ATLAS_ROW_OFFSET + ATLAS_ROWS {
            return None;
        }
        let surface = AtlasSurface { abs_row: self.next_row, width, height };
        self.next_row = end;
        Some(surface)
    }

    /// Return a surface's rows to the allocator. Coalesces with adjacent free
    /// bands and reclaims the high-water tail, so the freed rows are fully
    /// reusable (no fragmentation creep across show/hide cycles).
    pub(crate) fn free(&mut self, surface: AtlasSurface) {
        if surface.height == 0 {
            return;
        }
        // Drop the band on the floor only if the (tiny) free list is somehow full
        // AND it can't be merged — extremely unlikely given coalescing runs first.
        if self.free_len < MAX_FREE_SPANS {
            self.free[self.free_len] = FreeSpan { row: surface.abs_row, rows: surface.height };
            self.free_len += 1;
        }
        self.coalesce();
    }

    /// Largest contiguous tail still unallocated by the bump pointer. Used by the
    /// boot path to clamp a "take the rest" allocation; at boot the free list is
    /// empty so this equals the total free space.
    pub(crate) fn rows_remaining(&self) -> u32 {
        (ATLAS_ROW_OFFSET + ATLAS_ROWS).saturating_sub(self.next_row)
    }

    /// Total free rows including coalesced free bands and the bump tail.
    pub(crate) fn total_free_rows(&self) -> u32 {
        let mut total = self.rows_remaining();
        for i in 0..self.free_len {
            total += self.free[i].rows;
        }
        total
    }

    /// Remove free-list entry `i` (order-independent compaction).
    fn remove_span(&mut self, i: usize) {
        self.free_len -= 1;
        self.free[i] = self.free[self.free_len];
    }

    /// Merge adjacent free bands and fold any band touching the high-water mark
    /// back into the bump tail. O(n²) over a list capped at `MAX_FREE_SPANS`.
    fn coalesce(&mut self) {
        // Fold bands that abut the high-water tail back into `next_row`, repeating
        // until none touch (a chain of freed-from-the-top windows collapses fully).
        let mut folded = true;
        while folded {
            folded = false;
            for i in 0..self.free_len {
                if self.free[i].row + self.free[i].rows == self.next_row {
                    self.next_row = self.free[i].row;
                    self.remove_span(i);
                    folded = true;
                    break;
                }
            }
        }
        // Merge any two adjacent interior bands into one, repeating until stable.
        let mut merged = true;
        while merged {
            merged = false;
            'outer: for a in 0..self.free_len {
                for b in 0..self.free_len {
                    if a == b {
                        continue;
                    }
                    if self.free[a].row + self.free[a].rows == self.free[b].row {
                        self.free[a].rows += self.free[b].rows;
                        self.remove_span(b);
                        merged = true;
                        break 'outer;
                    }
                }
            }
        }
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

    #[test]
    fn free_of_interior_band_is_reused_by_a_fitting_alloc() {
        let mut a = AtlasAllocator::new();
        let s0 = a.alloc(360, 100).unwrap();
        let s1 = a.alloc(360, 100).unwrap();
        let _s2 = a.alloc(360, 100).unwrap();
        // Free the middle band, then a same-size alloc must reuse its rows.
        a.free(s1);
        let reused = a.alloc(360, 100).unwrap();
        assert_eq!(reused.abs_row, s1.abs_row);
        // The high-water mark did not advance (we reused, not bumped).
        assert_eq!(a.rows_remaining(), ATLAS_ROWS - 300);
        assert_ne!(reused.abs_row, s0.abs_row);
    }

    #[test]
    fn free_splits_a_larger_band_and_keeps_the_remainder() {
        let mut a = AtlasAllocator::new();
        let s0 = a.alloc(360, 200).unwrap();
        let _s1 = a.alloc(360, 50).unwrap();
        a.free(s0); // 200-row band free
        let small = a.alloc(360, 60).unwrap(); // takes 60, leaves 140
        assert_eq!(small.abs_row, s0.abs_row);
        let rest = a.alloc(360, 140).unwrap(); // exactly the remainder
        assert_eq!(rest.abs_row, s0.abs_row + 60);
        // Nothing bumped beyond the original high-water mark.
        assert_eq!(a.rows_remaining(), ATLAS_ROWS - 250);
    }

    #[test]
    fn freeing_the_top_band_reclaims_the_high_water_tail() {
        let mut a = AtlasAllocator::new();
        let _s0 = a.alloc(360, 100).unwrap();
        let s1 = a.alloc(360, 100).unwrap();
        assert_eq!(a.rows_remaining(), ATLAS_ROWS - 200);
        a.free(s1); // top band → folded back into the bump tail
        assert_eq!(a.rows_remaining(), ATLAS_ROWS - 100);
        assert_eq!(a.total_free_rows(), ATLAS_ROWS - 100);
    }

    #[test]
    fn adjacent_freed_bands_coalesce_into_one_big_band() {
        let mut a = AtlasAllocator::new();
        let s0 = a.alloc(360, 100).unwrap();
        let s1 = a.alloc(360, 100).unwrap();
        let _guard = a.alloc(360, 100).unwrap(); // keep the tail away
        // Free two adjacent interior bands in either order; they must merge so a
        // single 200-row alloc fits in the hole (not just two 100-row allocs).
        a.free(s1);
        a.free(s0);
        let big = a.alloc(360, 200).unwrap();
        assert_eq!(big.abs_row, s0.abs_row);
        assert_eq!(a.rows_remaining(), ATLAS_ROWS - 300);
    }

    #[test]
    fn show_hide_show_cycles_do_not_leak_rows() {
        let mut a = AtlasAllocator::new();
        let base = a.alloc(1280, 500).unwrap();
        let _ = base;
        let start_free = a.total_free_rows();
        // Open + close a window surface many times; free rows must stay constant.
        for _ in 0..1000 {
            let content = a.alloc(360, 374).unwrap();
            let blur = a.alloc(360, 374).unwrap();
            a.free(blur);
            a.free(content);
            assert_eq!(a.total_free_rows(), start_free);
        }
    }
}
