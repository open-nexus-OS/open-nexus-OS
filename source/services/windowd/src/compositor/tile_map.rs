// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Tile-based damage tracking map (64×64 tiles) for the windowd compositor.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 3 unit tests

use super::{TILES_X, TILES_Y, TILE_COUNT, TILE_DIRTY_WORDS, TILE_SIZE};
use crate::live_runtime::DamageRect;

/// Tile-based damage map. Tracks which 64×64 tiles are dirty and need re-rendering.
#[derive(Clone)]
pub(crate) struct TileMap {
    pub(crate) dirty: [u64; TILE_DIRTY_WORDS],
}

impl TileMap {
    pub(crate) fn new() -> Self {
        Self { dirty: [0; TILE_DIRTY_WORDS] }
    }

    pub(crate) fn tile_index(x: u32, y: u32) -> usize {
        (y / TILE_SIZE) as usize * TILES_X + (x / TILE_SIZE) as usize
    }

    pub(crate) fn mark_rect(&mut self, rect: DamageRect) {
        let tx0 = rect.x / TILE_SIZE;
        let ty0 = rect.y / TILE_SIZE;
        let tx1 = (rect.end_x().saturating_sub(1) / TILE_SIZE).min(TILES_X as u32 - 1);
        let ty1 = (rect.end_y().saturating_sub(1) / TILE_SIZE).min(TILES_Y as u32 - 1);
        for ty in ty0..=ty1 {
            for tx in tx0..=tx1 {
                let idx = ty as usize * TILES_X + tx as usize;
                let word = idx / 64;
                let bit = idx % 64;
                self.dirty[word] |= 1u64 << bit;
            }
        }
    }

    pub(crate) fn is_dirty(&self, tx: usize, ty: usize) -> bool {
        let idx = ty * TILES_X + tx;
        let word = idx / 64;
        let bit = idx % 64;
        self.dirty[word] & (1u64 << bit) != 0
    }

    pub(crate) fn clear(&mut self) {
        for w in &mut self.dirty {
            *w = 0;
        }
    }

    pub(crate) fn has_dirty(&self) -> bool {
        self.dirty.iter().any(|w| *w != 0)
    }

    pub(crate) fn dirty_tiles(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        (0..TILE_COUNT).filter_map(|idx| {
            let word = idx / 64;
            let bit = idx % 64;
            (self.dirty[word] & (1u64 << bit) != 0).then(|| (idx % TILES_X, idx / TILES_X))
        })
    }

    pub(crate) fn has_dirty_in_row_range(&self, start_y: u32, end_y: u32) -> bool {
        let ty0 = (start_y / TILE_SIZE) as usize;
        let ty1 = ((end_y.saturating_sub(1)) / TILE_SIZE).min(TILES_Y as u32 - 1) as usize;
        for ty in ty0..=ty1 {
            for tx in 0..TILES_X {
                let idx = ty * TILES_X + tx;
                let word = idx / 64;
                let bit = idx % 64;
                if self.dirty[word] & (1u64 << bit) != 0 {
                    return true;
                }
            }
        }
        false
    }
}
