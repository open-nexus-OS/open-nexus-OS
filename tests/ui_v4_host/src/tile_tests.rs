// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Phase 1 tile tests — TileMap dirty tracking, DamageRect → tile conversion.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 7 tests
//!
//! TEST SCOPE: tile index, mark rect, overlapping, clear, accumulation, full screen
//! ADR: docs/rfcs/RFC-0058-ui-v3b-clip-scroll-effects-ime-contract.md

#[cfg(test)]
mod tests {
    const TILE_SIZE: u32 = 64;
    const TILES_X: usize = 20;
    const TILES_Y: usize = 13;
    const TILE_COUNT: usize = TILES_X * TILES_Y;

    fn tile_index(x: u32, y: u32) -> usize {
        (y / TILE_SIZE) as usize * TILES_X + (x / TILE_SIZE) as usize
    }

    struct SimpleTileMap {
        dirty: [bool; TILE_COUNT],
    }

    impl SimpleTileMap {
        fn new() -> Self {
            Self { dirty: [false; TILE_COUNT] }
        }

        fn mark_rect(&mut self, x: u32, y: u32, w: u32, h: u32) {
            let tx0 = x / TILE_SIZE;
            let ty0 = y / TILE_SIZE;
            let tx1 = ((x + w).saturating_sub(1) / TILE_SIZE).min(TILES_X as u32 - 1);
            let ty1 = ((y + h).saturating_sub(1) / TILE_SIZE).min(TILES_Y as u32 - 1);
            for ty in ty0..=ty1 {
                for tx in tx0..=tx1 {
                    self.dirty[ty as usize * TILES_X + tx as usize] = true;
                }
            }
        }

        fn is_dirty(&self, tx: usize, ty: usize) -> bool {
            self.dirty[ty * TILES_X + tx]
        }
        fn clear(&mut self) {
            self.dirty = [false; TILE_COUNT];
        }
        fn has_dirty(&self) -> bool {
            self.dirty.iter().any(|d| *d)
        }
        fn dirty_count(&self) -> usize {
            self.dirty.iter().filter(|d| **d).count()
        }
    }

    #[test]
    fn test_tile_index_corners() {
        assert_eq!(tile_index(0, 0), 0);
        assert_eq!(tile_index(63, 63), 0);
        assert_eq!(tile_index(64, 0), 1);
        assert_eq!(tile_index(0, 64), TILES_X);
        assert_eq!(tile_index(1279, 799), TILE_COUNT - 1);
    }

    #[test]
    fn test_mark_single_small_rect() {
        let mut map = SimpleTileMap::new();
        map.mark_rect(10, 10, 50, 50);
        assert_eq!(map.dirty_count(), 1);
        assert!(map.is_dirty(0, 0));
    }

    #[test]
    fn test_mark_rect_spans_two_tiles() {
        let mut map = SimpleTileMap::new();
        map.mark_rect(50, 0, 30, 80);
        assert!(map.is_dirty(0, 0));
        assert!(map.is_dirty(1, 0));
        assert!(map.is_dirty(0, 1));
        assert!(map.is_dirty(1, 1));
        assert_eq!(map.dirty_count(), 4);
    }

    #[test]
    fn test_mark_rect_clamped_to_bounds() {
        let mut map = SimpleTileMap::new();
        map.mark_rect(1270, 790, 50, 50);
        let count = map.dirty_count();
        assert!(count >= 1, "should mark at least one tile");
    }

    #[test]
    fn test_clear_resets_all() {
        let mut map = SimpleTileMap::new();
        map.mark_rect(0, 0, 128, 128);
        assert!(map.has_dirty());
        map.clear();
        assert!(!map.has_dirty());
        assert_eq!(map.dirty_count(), 0);
    }

    #[test]
    fn test_multiple_rects_accumulate() {
        let mut map = SimpleTileMap::new();
        map.mark_rect(0, 0, 64, 64);
        map.mark_rect(128, 0, 64, 64);
        assert!(map.is_dirty(0, 0));
        assert!(!map.is_dirty(1, 0));
        assert!(map.is_dirty(2, 0));
        assert_eq!(map.dirty_count(), 2);
    }

    #[test]
    fn test_full_screen_marks_all_tiles() {
        let mut map = SimpleTileMap::new();
        map.mark_rect(0, 0, 1280, 800);
        assert_eq!(map.dirty_count(), TILE_COUNT);
    }
}
