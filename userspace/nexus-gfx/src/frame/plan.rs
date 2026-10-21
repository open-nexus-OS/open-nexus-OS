// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A frame's plan (TASK-0215 5c-3 P4): the retained set walked back to front — every layer as
//! drawn, a glass layer's cache step decided against the destination below it — and whether the
//! atlas's content must be uploaded first. Executors draw the plan's steps in order: per layer
//! its glass, its shadow, its content. Nothing is allocated: the plan is the frame's own storage,
//! reused frame after frame (gpud's heap never frees).

use super::damage::Area;
use super::glass::{fold_layer, glass_key, glass_rows, seed, CacheStep, GlassCache};
use super::layers::{FrameLayer, RetainedLayers, MAX_LAYERS, ORIGIN_ABOVE};

/// How far a glass's blur reads beyond its rectangle: the widest blur a layer may ask (64, the
/// buffer's bound) and the chain's own margin.
const GLASS_REACH: u32 = 66;

/// Whether `a` and `b` draw the same pixels at the same place — the layer as drawn, its content
/// generation left out (a content change reaches the screen as the present's damage).
fn same_drawing(a: &FrameLayer, b: &FrameLayer) -> bool {
    FrameLayer { content_epoch: 0, ..*a } == FrameLayer { content_epoch: 0, ..*b }
}

/// The screen a layer covers: its rectangle and its shadow's halo.
fn extent(l: &FrameLayer) -> Area {
    let pad = if l.shadow_blur > 0 { l.shadow_blur + l.shadow_offset_y.unsigned_abs() } else { 0 };
    Area::rect(l.dst_x, l.dst_y, l.width, l.height).grown(pad)
}

/// A glass layer's step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Glass {
    pub cache: CacheStep,
    /// Half-resolution rows its result takes.
    pub rows: u32,
    /// What a write blurs again (TASK-0335 P1): the damage below the glass within its reach
    /// when its rows already hold this backdrop — `Full` for a new or reshuffled backdrop.
    pub band: Area,
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
    /// What this frame changes on the screen (RFC-0101 §8): a plane shown this frame must be
    /// redrawn here at least — and wherever it lags earlier frames (`damage::History`).
    pub damage: Area,
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
        // What changed on the screen: the present's damage, every layer drawn otherwise than
        // in the last plan (where it was and where it is), every layer of the last plan that is
        // gone, and every glass blurred again this frame.
        let (named, named_count, full) = self.layers.take_damage();
        let named = &named[..named_count];
        let mut damage = named.iter().fold(Area::None, |a, (r, _)| a.union(*r));
        if full {
            damage = Area::Full;
        }
        // A glass blurs its backdrop again for a change below it: the base's (origin 0), a
        // window's below it in the walk, an unknown origin's; never for its own content, a
        // window's above it or the pointer's.
        let origin_below = |origin: u32, j: usize, own_id: u32| -> bool {
            origin == 0
                || (origin != ORIGIN_ABOVE
                    // Every slice of a window shares its id: a window's own change is never
                    // below one of its own glasses (cycle 41: the second slice counted the
                    // first as below it).
                    && origin != own_id
                    && self.layers.index_of_id(origin).is_none_or(|i| i < j))
        };
        let mut below = seed(display.0, display.1, self.glass.wallpaper_epoch);
        let mut cursor = 0u32;
        let mut count = 0;
        for i in 0..self.layers.len() {
            let Some(layer) = self.layers.effective(i) else { break };
            let before = self.steps.get(i).filter(|_| i < self.count).map(|s| s.layer);
            if before.is_none_or(|b| !same_drawing(&b, &layer)) {
                damage = damage.union(extent(&layer));
                if let Some(b) = before {
                    damage = damage.union(extent(&b));
                }
            }
            let glass = if layer.backdrop_blur == 0
                || layer.width == 0
                || layer.height == 0
                || !self.glass.ready
                || !blurs(layer.backdrop_blur)
            {
                None
            } else {
                let rows = glass_rows(
                    layer.height,
                    super::board::blur_scale(super::board::blur_radius(layer.backdrop_blur)),
                );
                let rect = Area::rect(layer.dst_x, layer.dst_y, layer.width, layer.height);
                let reach = rect.grown(GLASS_REACH);
                // The desktop's own glass regions (id 0: the dock's buttons, the bar's pills)
                // draw the desktop surface's pixels over a blur of the same surface: a change
                // of the desktop within such a region is the region's own content, not its
                // backdrop (cycle 38: those regions were blurred again every frame).
                let own = |r: &Area, o: u32| o == 0 && layer.layer_id == 0 && r.inside(rect);
                // The damage below the glass within its reach: the band it blurs again
                // (TASK-0335 P1). A full present, a new backdrop (the key) or rows at a new
                // place blur the whole glass.
                let band = if full {
                    Area::Full
                } else {
                    named.iter().fold(Area::None, |a, (r, o)| {
                        if origin_below(*o, i, layer.layer_id) && r.touches(reach) && !own(r, *o) {
                            a.union(r.intersect(reach))
                        } else {
                            a
                        }
                    })
                };
                let touched = band != Area::None;
                let key = glass_key(below, &layer);
                let band = if self.glass.would_hit(i, key, cursor) { band } else { Area::Full };
                let cache = self.glass.step(i, key, cursor, rows, touched);
                if !matches!(cache, CacheStep::Off) {
                    cursor = cursor.saturating_add(rows);
                }
                // What the glass shows changes where its blurred backdrop did: the band grown by
                // the blur's reach, within the glass (the whole glass when blurred live or anew).
                match cache {
                    CacheStep::Read(_) => {}
                    CacheStep::Write(_) => {
                        damage = damage.union(band.grown(GLASS_REACH).intersect(rect));
                    }
                    CacheStep::Off => damage = damage.union(rect),
                }
                Some(Glass { cache, rows, band })
            };
            self.steps[count] = LayerStep { layer, glass };
            count += 1;
            below = fold_layer(below, &layer);
        }
        for gone in self.steps[count.min(self.count)..self.count].iter() {
            damage = damage.union(extent(&gone.layer));
        }
        self.count = count;
        self.layers.dirty = false;
        if upload && set_epoch != 0 {
            self.layers.uploaded_epoch = set_epoch;
        }
        Planned { layers: count, upload, damage }
    }

    /// Step `i` of the last plan.
    pub fn step(&self, i: usize) -> Option<LayerStep> {
        self.steps[..self.count].get(i).copied()
    }
}
