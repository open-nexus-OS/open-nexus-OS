// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Surface atlas allocator for the retained-surface GPU compositor.
//! Cached layer surfaces (base UI, sidebar, chat window, shell windows, buttons)
//! live in the framebuffer VMO beyond the four display planes, so the compositor
//! can blit/composite a layer at any position without re-rendering it.
//!
//! Framebuffer VMO row layout (1280 px wide, BGRA8888):
//!   rows    0.. 799  Plane 0 — wallpaper source
//!   rows  800..1599  Plane 1 — retained base scene
//!   rows 1600..2399  Plane 2 — display (ring slot A)
//!   rows 2400..3199  Plane 3 — blur cache (ring slot B)
//!   rows 3200..6399  Atlas   — cached layer surfaces  ← this module
//!
//! **2D packing.** The atlas is a 1280-wide × `ATLAS_ROWS`-tall region packed by a
//! free-rectangle allocator (guillotine split on alloc, axis-aligned coalescing on
//! free). A narrow surface (e.g. a 360-wide window) no longer wastes a whole
//! 1280-wide row band — several pack side-by-side in one band — which is what
//! lets the same VMO host many windows / HiDPI surfaces (a shared multi-surface atlas model).
//! Surfaces are addressed by absolute VMO row **and column** (`x`); gpud samples
//! `src_x`/`src_row_abs` natively on both the CPU and virgl paths.
//!
//! Two allocation shapes:
//!   - [`AtlasAllocator::alloc_band`] — a FULL-WIDTH band (`x = 0`). Required by
//!     surfaces whose CPU renderer writes full-stride rows in `vmo_write` bands
//!     (chat, base): nothing packs beside them, so the banded write is safe.
//!   - [`AtlasAllocator::alloc`] — a 2D-PACKED sub-region (`x` may be > 0). Its
//!     renderer must write per-row at column `x` (sub-stride). For small surfaces
//!     rendered on infrequent events (window content/blur), that cost is fine.
//!
//! Pure logic, no OS deps → host-testable.
//!
//! OWNERS: @ui
//! STATUS: retained-surface compositor — 2D pack
//! API_STABILITY: Unstable

/// Display width in pixels (atlas surfaces share the framebuffer stride).
pub(crate) const ATLAS_WIDTH: u32 = 1280;
/// First atlas row (immediately after the four display planes, each 800 rows).
pub(crate) const ATLAS_ROW_OFFSET: u32 = 3200;
/// Atlas height in rows.
// 6400 rows: a real desktop needs desktop band (800) + one resident scroll
// band (chat ≈ 2568) + 3 floating windows (+ blur bands) + dock + fullscreen
// round-trips. 4000 starved on the fullscreen re-create with 4 windows open
// (`FAIL atlas need=1280x800 rows_remaining=24`).
pub(crate) const ATLAS_ROWS: u32 = 6400;
/// Total framebuffer-resource height including the atlas. windowd sizes the VMO
/// to this; gpud's `RESOURCE_HEIGHT` MUST match (separate crate, no shared dep).
pub(crate) const RESOURCE_HEIGHT: u32 = ATLAS_ROW_OFFSET + ATLAS_ROWS; // 9600

/// A cached layer surface: a packed rectangle in the atlas region.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AtlasSurface {
    /// Absolute VMO column of the surface's left edge (`src_x` for BlitAbsolute /
    /// CompositeLayer). 0 for full-width band surfaces.
    pub(crate) x: u32,
    /// Absolute VMO start row of the surface (`src_y_abs` for BlitAbsolute).
    pub(crate) abs_row: u32,
    /// Reserved width in columns (full atlas width for band surfaces).
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl AtlasSurface {
    /// Byte offset of the surface's top-left pixel within the framebuffer VMO.
    pub(crate) fn byte_offset(self, stride_bytes: usize) -> usize {
        self.abs_row as usize * stride_bytes + self.x as usize * 4
    }
}

/// A free rectangle available for allocation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FreeRect {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

/// Max free rectangles tracked. Guillotine split adds ≤2 per alloc; coalescing on
/// free keeps the count bounded well under this for the handful of live surfaces.
const MAX_FREE_RECTS: usize = 48;

/// Free-rectangle (guillotine) allocator over the atlas region. `alloc`/`alloc_band`
/// best-fit a free rect and split the remainder; `free` returns the rect and
/// coalesces it with axis-aligned neighbours. The boot sequence (only `alloc_band`,
/// all before any `free`) degenerates to simple vertical stacking — byte-for-byte
/// the old row-bump layout — so existing surfaces keep their exact positions.
pub(crate) struct AtlasAllocator {
    free: [FreeRect; MAX_FREE_RECTS],
    free_len: usize,
}

impl AtlasAllocator {
    pub(crate) fn new() -> Self {
        let mut free = [FreeRect { x: 0, y: 0, w: 0, h: 0 }; MAX_FREE_RECTS];
        free[0] = FreeRect { x: 0, y: ATLAS_ROW_OFFSET, w: ATLAS_WIDTH, h: ATLAS_ROWS };
        Self { free, free_len: 1 }
    }

    /// Reserve a FULL-WIDTH band `height` rows tall (`x = 0`). For surfaces whose
    /// CPU renderer writes full-stride `vmo_write` bands (nothing packs beside it).
    pub(crate) fn alloc_band(&mut self, height: u32) -> Option<AtlasSurface> {
        self.alloc(ATLAS_WIDTH, height)
    }

    /// Reserve a 2D-packed `width × height` sub-region (`x` may be > 0). Best-fit +
    /// guillotine split. Returns `None` if no free rect fits.
    pub(crate) fn alloc(&mut self, width: u32, height: u32) -> Option<AtlasSurface> {
        if width == 0 || height == 0 || width > ATLAS_WIDTH || height > ATLAS_ROWS {
            return None;
        }
        // Best-area-fit: the free rect that fits with the least leftover area, so
        // a full-width band prefers the full-width tail and a narrow surface tucks
        // into a side gap rather than carving up a fresh band.
        let mut best: Option<usize> = None;
        let mut best_leftover = u64::MAX;
        for i in 0..self.free_len {
            let r = self.free[i];
            if r.w >= width && r.h >= height {
                let leftover = r.w as u64 * r.h as u64 - width as u64 * height as u64;
                if leftover < best_leftover {
                    best_leftover = leftover;
                    best = Some(i);
                }
            }
        }
        let i = best?;
        let r = self.free[i];
        self.remove_rect(i);
        // Guillotine split: a strip to the right (same height) + the rest below.
        if r.w > width {
            self.push_rect(FreeRect { x: r.x + width, y: r.y, w: r.w - width, h: height });
        }
        if r.h > height {
            self.push_rect(FreeRect { x: r.x, y: r.y + height, w: r.w, h: r.h - height });
        }
        Some(AtlasSurface { x: r.x, abs_row: r.y, width, height })
    }

    /// Return a surface's rectangle to the allocator, coalescing it with adjacent
    /// free rects so packed/banded space is fully recovered across show/hide.
    pub(crate) fn free(&mut self, surface: AtlasSurface) {
        if surface.width == 0 || surface.height == 0 {
            return;
        }
        self.push_rect(FreeRect {
            x: surface.x,
            y: surface.abs_row,
            w: surface.width,
            h: surface.height,
        });
        self.coalesce();
    }

    /// Tallest FULL-WIDTH free band still available (`x == 0`, spans the stride).
    /// The boot "take the rest" side-panel clamp uses this; at boot the free list
    /// is a single full-width tail, so it equals the remaining rows.
    pub(crate) fn rows_remaining(&self) -> u32 {
        let mut tallest = 0;
        for i in 0..self.free_len {
            let r = self.free[i];
            if r.x == 0 && r.w == ATLAS_WIDTH && r.h > tallest {
                tallest = r.h;
            }
        }
        tallest
    }

    /// Total free area expressed in full-width row-equivalents (telemetry / leak
    /// checks): `Σ(w·h) / ATLAS_WIDTH`.
    pub(crate) fn total_free_rows(&self) -> u32 {
        let mut area = 0u64;
        for i in 0..self.free_len {
            area += self.free[i].w as u64 * self.free[i].h as u64;
        }
        (area / ATLAS_WIDTH as u64) as u32
    }

    fn remove_rect(&mut self, i: usize) {
        self.free_len -= 1;
        self.free[i] = self.free[self.free_len];
    }

    fn push_rect(&mut self, r: FreeRect) {
        if r.w == 0 || r.h == 0 {
            return;
        }
        if self.free_len < MAX_FREE_RECTS {
            self.free[self.free_len] = r;
            self.free_len += 1;
        }
    }

    /// Merge axis-aligned adjacent free rects (same column-span & touching rows, or
    /// same row-span & touching columns) until stable. O(n²) over a small list.
    fn coalesce(&mut self) {
        let mut merged = true;
        while merged {
            merged = false;
            'outer: for a in 0..self.free_len {
                for b in 0..self.free_len {
                    if a == b {
                        continue;
                    }
                    let ra = self.free[a];
                    let rb = self.free[b];
                    // Vertically adjacent, same x-span → stack into one.
                    let vstack = ra.x == rb.x && ra.w == rb.w && ra.y + ra.h == rb.y;
                    // Horizontally adjacent, same y-span → join side-by-side.
                    let hjoin = ra.y == rb.y && ra.h == rb.h && ra.x + ra.w == rb.x;
                    if vstack {
                        self.free[a].h += rb.h;
                        self.remove_rect(b);
                        merged = true;
                        break 'outer;
                    }
                    if hjoin {
                        self.free[a].w += rb.w;
                        self.remove_rect(b);
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
        assert_eq!(RESOURCE_HEIGHT, 9600);
        assert_eq!(ATLAS_ROW_OFFSET, 4 * 800);
    }

    #[test]
    fn bands_stack_like_the_old_bump_allocator() {
        // Boot path: only full-width bands, allocated before any free → identical
        // vertical stacking + positions to the historical row-bump allocator.
        let mut a = AtlasAllocator::new();
        let s0 = a.alloc_band(260).unwrap();
        let s1 = a.alloc_band(764).unwrap();
        assert_eq!((s0.x, s0.abs_row), (0, ATLAS_ROW_OFFSET));
        assert_eq!((s1.x, s1.abs_row), (0, ATLAS_ROW_OFFSET + 260));
        assert_eq!(a.rows_remaining(), ATLAS_ROWS - 260 - 764);
    }

    #[test]
    fn narrow_surfaces_pack_side_by_side_in_one_band() {
        // The 2D win: three 360-wide surfaces share ONE 374-row band (3×360=1080 ≤
        // 1280) instead of consuming three full-width bands.
        let mut a = AtlasAllocator::new();
        let c = a.alloc(360, 374).unwrap();
        let blur = a.alloc(360, 374).unwrap();
        let third = a.alloc(360, 374).unwrap();
        assert_eq!((c.x, c.abs_row), (0, ATLAS_ROW_OFFSET));
        assert_eq!((blur.x, blur.abs_row), (360, ATLAS_ROW_OFFSET));
        assert_eq!((third.x, third.abs_row), (720, ATLAS_ROW_OFFSET));
        // Only 374 rows of full-width capacity were consumed (one band), not 3×374.
        assert_eq!(a.rows_remaining(), ATLAS_ROWS - 374);
    }

    #[test]
    fn free_and_realloc_reuses_the_same_slot() {
        let mut a = AtlasAllocator::new();
        let c = a.alloc(360, 374).unwrap();
        let blur = a.alloc(360, 374).unwrap();
        a.free(blur);
        let reused = a.alloc(360, 374).unwrap();
        assert_eq!((reused.x, reused.abs_row), (blur.x, blur.abs_row));
        let _ = c;
    }

    #[test]
    fn freeing_both_packed_surfaces_coalesces_back_to_full_width() {
        // Two side-by-side surfaces freed → their band coalesces back into the
        // full-width tail, so a later full-width band can use those rows again.
        let mut a = AtlasAllocator::new();
        let c = a.alloc(360, 374).unwrap();
        let blur = a.alloc(360, 374).unwrap();
        assert!(a.rows_remaining() < ATLAS_ROWS); // band carved out
        a.free(c);
        a.free(blur);
        assert_eq!(a.rows_remaining(), ATLAS_ROWS, "band fully reclaimed for full-width use");
        // And a full-width band now fits at the very top again.
        let band = a.alloc_band(374).unwrap();
        assert_eq!((band.x, band.abs_row), (0, ATLAS_ROW_OFFSET));
    }

    #[test]
    fn band_then_free_reclaims_for_full_width() {
        let mut a = AtlasAllocator::new();
        let _base = a.alloc_band(500).unwrap();
        let b = a.alloc_band(374).unwrap();
        assert_eq!(a.rows_remaining(), ATLAS_ROWS - 874);
        a.free(b);
        assert_eq!(a.rows_remaining(), ATLAS_ROWS - 500, "band rows returned to the tail");
    }

    #[test]
    fn exhaustion_returns_none() {
        let mut a = AtlasAllocator::new();
        assert!(a.alloc_band(ATLAS_ROWS).is_some());
        assert!(a.alloc_band(1).is_none());
        assert_eq!(a.rows_remaining(), 0);
    }

    #[test]
    fn rejects_overwide_or_empty() {
        let mut a = AtlasAllocator::new();
        assert!(a.alloc(ATLAS_WIDTH + 1, 10).is_none());
        assert!(a.alloc(100, 0).is_none());
        assert!(a.alloc(0, 10).is_none());
    }

    #[test]
    fn byte_offset_accounts_for_row_and_column() {
        let s = AtlasSurface { x: 360, abs_row: ATLAS_ROW_OFFSET, width: 360, height: 10 };
        let stride = ATLAS_WIDTH as usize * 4;
        assert_eq!(s.byte_offset(stride), ATLAS_ROW_OFFSET as usize * stride + 360 * 4);
    }

    #[test]
    fn show_hide_show_cycles_do_not_leak() {
        let mut a = AtlasAllocator::new();
        let _base = a.alloc_band(500).unwrap();
        let start = a.total_free_rows();
        for _ in 0..1000 {
            let content = a.alloc(360, 374).unwrap();
            let blur = a.alloc(360, 374).unwrap();
            a.free(blur);
            a.free(content);
            assert_eq!(a.total_free_rows(), start, "no fragmentation creep across cycles");
        }
    }
}
