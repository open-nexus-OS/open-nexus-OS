// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The glass cache (moved from the QEMU path's backend, TASK-0215 5c-3 P4): a glass layer is
//! blurred again only when its backdrop changed.
//!
//! WHY: every frame composes the whole set, so every glass layer would pay its snapshot and two
//! blur passes on every pointer move and keystroke — windowd's own `BackdropCache` never reaches
//! a GPU executor (its fields stay in the CPU path's commands).
//!
//! MODEL: a glass layer's blurred backdrop is a function of everything composited below it.
//! Walking the set back to front, a running FNV-1a hash folds each drawn layer's inputs as drawn
//! (its overrides applied, its content generation included); a glass layer's key is that fold and
//! its own rectangle and radii. The same key at the same packed place: the cached rows are read
//! (one draw). Otherwise the blur is written into its rows and read from them. Rows pack in walk
//! order (half-resolution results); a layer that no longer fits blurs live, uncached — the cache
//! bounds cost, never correctness.

use super::layers::{FrameLayer, MAX_LAYERS};

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// One FNV-1a fold step.
pub fn fold(h: u64, v: u64) -> u64 {
    (h ^ v).wrapping_mul(FNV_PRIME)
}

/// A frame's seed: the display's size and the wallpaper's generation (what every frame starts
/// from).
pub fn seed(display_w: u32, display_h: u32, wallpaper_epoch: u32) -> u64 {
    fold(
        fold(FNV_OFFSET, (u64::from(display_w) << 32) | u64::from(display_h)),
        u64::from(wallpaper_epoch),
    )
}

/// Folds one drawn layer (as drawn: overrides applied) into the destination so far. Every layer
/// folds — an opaque one changes what a glass above it blurs as much as a glass does.
pub fn fold_layer(h: u64, l: &FrameLayer) -> u64 {
    let a = (u64::from(l.src_row_abs) << 32) | u64::from(l.src_x);
    let b = (u64::from(l.width) << 32) | u64::from(l.height);
    let c = (u64::from(l.dst_x) << 32) | u64::from(l.dst_y);
    let d = (u64::from(l.opacity) << 32) | u64::from(l.content_epoch);
    fold(fold(fold(fold(h, a), b), c), d)
}

/// A glass layer's key: the destination below it, its rectangle, its blur and corner radius.
pub fn glass_key(below: u64, l: &FrameLayer) -> u64 {
    fold(
        fold(below, (u64::from(l.dst_x) << 32) | u64::from(l.dst_y)),
        (u64::from(l.width) << 40)
            | (u64::from(l.height) << 16)
            | (u64::from(l.backdrop_blur) << 8)
            | u64::from(l.corner_radius),
    )
}

/// Half-resolution rows a glass's result takes: half its height, rounded up, and one texel of
/// margin above and below (the layer's bilinear fetches at the glass's edges read blurred
/// backdrop, never a neighbour's rows).
pub fn glass_rows(height: u32) -> u32 {
    height.div_ceil(2) + 2
}

/// What a glass layer does this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheStep {
    /// The backdrop is unchanged: one draw from the cache rows starting here.
    Read(u32),
    /// The backdrop changed: blurred into the cache rows here, then drawn from them.
    Write(u32),
    /// No rows left (or no cache): blurred live, nothing kept.
    Off,
}

/// The cache's bookkeeping, a slot per layer index — windowd encodes the scene back to front
/// deterministically, so a stable scene keeps its slots and a changed one changes its keys.
pub struct GlassCache {
    keys: [u64; MAX_LAYERS],
    ys: [u32; MAX_LAYERS],
    valid: [bool; MAX_LAYERS],
    /// The cache's height in half-resolution rows.
    rows: u32,
    /// Bumped whenever the wallpaper the frames start from is uploaded again.
    pub wallpaper_epoch: u32,
    /// The executor's cache exists (its one-shot setup succeeded).
    pub ready: bool,
}

impl GlassCache {
    /// A cache of `rows` half-resolution rows, not ready yet.
    pub const fn new(rows: u32) -> Self {
        Self {
            keys: [0; MAX_LAYERS],
            ys: [0; MAX_LAYERS],
            valid: [false; MAX_LAYERS],
            rows,
            wallpaper_epoch: 0,
            ready: false,
        }
    }

    /// Read, write or off for glass layer `slot` with key `key` and `h` rows at the packing
    /// cursor `cursor_y` (the caller advances it by `h` on a read or a write). A hit needs the
    /// same key at the same place: a reshuffled set writes instead of reading rows that now
    /// belong to another layer.
    pub fn step(&mut self, slot: usize, key: u64, cursor_y: u32, h: u32) -> CacheStep {
        if !self.ready || slot >= MAX_LAYERS || cursor_y.saturating_add(h) > self.rows {
            if let Some(v) = self.valid.get_mut(slot) {
                *v = false;
            }
            return CacheStep::Off;
        }
        let hit = self.valid[slot] && self.keys[slot] == key && self.ys[slot] == cursor_y;
        self.keys[slot] = key;
        self.ys[slot] = cursor_y;
        self.valid[slot] = true;
        if hit {
            CacheStep::Read(cursor_y)
        } else {
            CacheStep::Write(cursor_y)
        }
    }
}
