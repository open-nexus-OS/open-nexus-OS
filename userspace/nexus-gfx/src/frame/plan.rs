// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A frame's plan (TASK-0215 5c-3 P4): the retained set walked back to front — every layer as
//! drawn, a glass layer's cache step decided against the destination below it — and whether the
//! atlas's content must be uploaded first. Executors draw the plan's steps in order: per layer
//! its glass, its shadow, its content. Nothing is allocated: the plan is the frame's own storage,
//! reused frame after frame (gpud's heap never frees).

use super::glass::{fold_layer, glass_key, glass_rows, seed, CacheStep, GlassCache};
use super::layers::{FrameLayer, RetainedLayers, MAX_LAYERS};

/// A glass layer's step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Glass {
    pub cache: CacheStep,
    /// Half-resolution rows its result takes.
    pub rows: u32,
}

/// One layer of the plan.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayerStep {
    /// The layer as drawn (overrides applied).
    pub layer: FrameLayer,
    /// Its glass, when it is a glass layer the executor blurs.
    pub glass: Option<Glass>,
}

/// What the walk decided for the frame as a whole.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Planned {
    /// Layers in the plan (`Frame::step(0..layers)`).
    pub layers: usize,
    /// The atlas's content changed since its last upload: upload before the layers draw.
    pub upload: bool,
}

/// The retained set, the glass cache and the plan they give.
pub struct Frame {
    pub layers: RetainedLayers,
    pub glass: GlassCache,
    steps: [LayerStep; MAX_LAYERS],
    count: usize,
}

impl Frame {
    /// A frame whose glass cache holds `glass_rows` half-resolution rows.
    pub const fn new(glass_rows: u32) -> Self {
        Self {
            layers: RetainedLayers::new(),
            glass: GlassCache::new(glass_rows),
            steps: [LayerStep {
                layer: FrameLayer {
                    src_row_abs: 0,
                    src_x: 0,
                    width: 0,
                    height: 0,
                    content_w: 0,
                    content_h: 0,
                    dst_x: 0,
                    dst_y: 0,
                    opacity: 0,
                    corner_radius: 0,
                    shadow_blur: 0,
                    shadow_offset_y: 0,
                    shadow_alpha: 0,
                    backdrop_blur: 0,
                    scroll_id: 0,
                    scroll_band_top_abs: 0,
                    scroll_band_h: 0,
                    layer_id: 0,
                    content_epoch: 0,
                },
                glass: None,
            }; MAX_LAYERS],
            count: 0,
        }
    }

    /// Plans the frame on a display of `display` pixels. `blurs(radius)`: whether the executor
    /// blurs at that radius — a glass it cannot blur gets no step and takes no cache slot. The
    /// set counts as clean afterwards, its generation as uploaded when the plan asks for the
    /// upload: an executor that loses the upload says so (`RetainedLayers::invalidate_upload`).
    pub fn plan(&mut self, display: (u32, u32), blurs: &dyn Fn(u32) -> bool) -> Planned {
        let set_epoch = self.layers.set_epoch();
        let upload = if set_epoch != 0 {
            self.layers.dirty && set_epoch != self.layers.uploaded_epoch
        } else {
            self.layers.dirty
        };
        let mut below = seed(display.0, display.1, self.glass.wallpaper_epoch);
        let mut cursor = 0u32;
        let mut count = 0;
        for i in 0..self.layers.len() {
            let Some(layer) = self.layers.effective(i) else { break };
            let glass = if layer.backdrop_blur == 0
                || layer.width == 0
                || layer.height == 0
                || !self.glass.ready
                || !blurs(layer.backdrop_blur)
            {
                None
            } else {
                let rows = glass_rows(layer.height);
                let cache = self.glass.step(i, glass_key(below, &layer), cursor, rows);
                if !matches!(cache, CacheStep::Off) {
                    cursor = cursor.saturating_add(rows);
                }
                Some(Glass { cache, rows })
            };
            self.steps[count] = LayerStep { layer, glass };
            count += 1;
            below = fold_layer(below, &layer);
        }
        self.count = count;
        self.layers.dirty = false;
        if upload && set_epoch != 0 {
            self.layers.uploaded_epoch = set_epoch;
        }
        Planned { layers: count, upload }
    }

    /// Step `i` of the last plan.
    pub fn step(&self, i: usize) -> Option<LayerStep> {
        self.steps[..self.count].get(i).copied()
    }
}
