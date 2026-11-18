// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A frame's plan (TASK-0215 5c-3 P4): the retained set walked back to front — every layer as
//! drawn, a glass layer's cache step decided against the destination below it — and whether the
//! atlas's content must be uploaded first. Executors draw the plan's steps in order: per layer
//! its glass, its shadow, its content. The plan is the frame's own storage, reused frame after
//! frame: its tables grow when a scene has more layers than any before it and are kept, so a
//! settled scene's frame costs the heap nothing.

use alloc::vec::Vec;

use super::damage::Area;
use super::glass::{fold_layer, glass_key, glass_rows, seed, CacheStep, GlassCache, WriteCause};
use super::layers::{FrameLayer, RetainedLayers, MAX_LAYERS, ORIGIN_ABOVE};
use super::occlude::{self, Px};

/// How far a glass's blur reads beyond its rectangle: the widest blur a layer may ask (64, the
/// buffer's bound) and the chain's own margin.
const GLASS_REACH: u32 = 66;

/// Whether `a` and `b` draw the same pixels at the same place — the layer as drawn, its content
/// generation and its buffer's slot left out (a content change, a slot flip included, reaches
/// the screen as the present's damage — RFC-0114 §6: a flip is never a backdrop change).
fn same_drawing(a: &FrameLayer, b: &FrameLayer) -> bool {
    let bare = |l: &FrameLayer| FrameLayer { content_epoch: 0, slot: 0, ..*l };
    bare(a) == bare(b)
}

/// Whether a layer of a surface buffer must be uploaded before it draws (RFC-0114 §6): its
/// buffer's content generation, buffer or slot changed since the layer last drew — or it is new,
/// or the executor lost its uploads. A layout layer is the frame's `upload`, never this.
fn buffer_upload(layer: &FrameLayer, before: Option<FrameLayer>, lost: bool) -> bool {
    layer.source != 0
        && (lost
            || before.is_none_or(|b| {
                (b.source, b.slot, b.content_epoch)
                    != (layer.source, layer.slot, layer.content_epoch)
            }))
}

/// The screen a layer covers: its rectangle and its shadow's halo.
fn extent(l: &FrameLayer) -> Area {
    let pad = if l.shadow_blur > 0 { l.shadow_blur + l.shadow_offset_y.unsigned_abs() } else { 0 };
    Area::rect(l.dst_x, l.dst_y, l.width, l.height).grown(pad)
}

/// The cache rows a glass layer's result takes.
fn rows_of(l: &FrameLayer) -> u32 {
    glass_rows(l.height, super::board::blur_scale(super::board::blur_radius(l.backdrop_blur)))
}

/// An area as a rectangle in pixels; `None` for no area and for the whole screen.
fn px(a: Area) -> Option<Px> {
    match a {
        Area::Rect { x0, y0, x1, y1 } => Some((x0, y0, x1 - x0, y1 - y0)),
        _ => None,
    }
}

/// A glass layer's step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Glass {
    pub cache: CacheStep,
    /// Half-resolution rows its result takes.
    pub rows: u32,
    /// What a write blurs again (TASK-0335 P1): the damage below the glass within its reach
    /// when its rows already hold this backdrop — `Full` for a new backdrop or new rows.
    pub band: Area,
}

/// One layer of the plan.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayerStep {
    /// The layer as drawn (its window's transform applied).
    pub layer: FrameLayer,
    /// Its glass, when it is a glass layer the executor blurs.
    pub glass: Option<Glass>,
    /// Its surface buffer's rows changed since it last drew (RFC-0114 §6): an executor that keeps
    /// a copy of the buffer (virgl's host texture) transfers the layer's rows first.
    pub upload: bool,
}

/// What the walk decided for the frame as a whole.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Planned {
    /// Layers in the plan (`Frame::step(0..layers)`).
    pub layers: usize,
    /// The glass writes of this frame by cause (TASK-0336 P0).
    pub writes: Writes,
    /// The atlas's content changed since its last upload: upload before the layers draw.
    pub upload: bool,
    /// What this frame changes on the screen (RFC-0101 §8): a plane shown this frame must be
    /// redrawn here at least — and wherever it lags earlier frames (`damage::History`).
    pub damage: Area,
}

/// The glass writes of a frame by cause (`WriteCause`): what made each glass blur again.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Writes {
    pub band: u16,
    pub backdrop: u16,
    pub new: u16,
    pub full: u16,
    /// Of the band writes, those a change of the base reached (origin 0: the desktop's own
    /// surface); the others were reached by a window below the glass.
    pub band_base: u16,
    /// Not writes: the glasses whose rows owe a change this frame because it lies under the
    /// layers above them (TASK-0351 P6) — counted every frame until one shows and pays it.
    pub hidden: u16,
}

impl Writes {
    fn note(&mut self, cause: WriteCause) {
        let n = match cause {
            WriteCause::Band => &mut self.band,
            WriteCause::Backdrop => &mut self.backdrop,
            WriteCause::New => &mut self.new,
            WriteCause::Full => &mut self.full,
        };
        *n = n.saturating_add(1);
    }

    /// Every write of the frame.
    pub fn total(self) -> u32 {
        u32::from(self.band) + u32::from(self.backdrop) + u32::from(self.new) + u32::from(self.full)
    }
}

/// A layer's identity in a plan (RFC-0109): its key and its occurrence among the layers of the
/// same key before it — unique in every set, so unnamed layers (key 0) still match by order.
pub type Ident = (u32, u16);

/// The retained set, the glass cache and the plan they give.
pub struct Frame {
    pub layers: RetainedLayers,
    pub glass: GlassCache,
    steps: Vec<LayerStep>,
    /// The identity of every step of the last plan.
    idents: Vec<Ident>,
    /// The last plan's layers as drawn, kept while this plan overwrites its steps.
    prev: Vec<FrameLayer>,
    /// A plan's working tables, kept between plans: the set as drawn, its identities, the
    /// last plan's step each layer carries on from, and the last plan's steps still drawn.
    drawn: Vec<FrameLayer>,
    ids: Vec<Ident>,
    carried: Vec<Option<usize>>,
    kept: Vec<bool>,
    /// The opaque rectangles above a glass that touch what its write would change.
    above: Vec<Px>,
}

/// `v` emptied with room for `n` entries; how many it has room for (all of them unless the
/// heap refused).
fn room<T>(v: &mut Vec<T>, n: usize) -> usize {
    v.clear();
    let _ = v.try_reserve(n);
    v.capacity().min(n)
}

impl Frame {
    /// A frame whose glass cache has planes of `glass_rows` half-resolution rows.
    pub const fn new(glass_rows: u32) -> Self {
        Self {
            layers: RetainedLayers::new(),
            glass: GlassCache::new(glass_rows),
            steps: Vec::new(),
            idents: Vec::new(),
            prev: Vec::new(),
            drawn: Vec::new(),
            ids: Vec::new(),
            carried: Vec::new(),
            kept: Vec::new(),
            above: Vec::new(),
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
        // The last plan by identity (RFC-0109): a layer is compared with — and keeps the glass
        // rows of — the layer of its key in the last plan, wherever either stands in its list.
        let last = self.steps.len();
        let (mut drawn, mut ids, mut carried, mut kept, mut above) = (
            core::mem::take(&mut self.drawn),
            core::mem::take(&mut self.ids),
            core::mem::take(&mut self.carried),
            core::mem::take(&mut self.kept),
            core::mem::take(&mut self.above),
        );
        // The plan's tables grow to the set. A heap that refuses them plans no layer and
        // names the whole screen: nothing of the last plan can be compared.
        let want = self.layers.len().min(MAX_LAYERS);
        self.prev.clear();
        let fits = room(&mut drawn, want) == want
            && room(&mut ids, want) == want
            && room(&mut carried, want) == want
            && room(&mut kept, last) == last
            && room(&mut above, want) == want
            && self.prev.try_reserve(last).is_ok()
            && self.steps.try_reserve(want.saturating_sub(last)).is_ok()
            && self.idents.try_reserve(want.saturating_sub(last)).is_ok();
        let n = if fits {
            self.prev.extend(self.steps.iter().map(|s| s.layer));
            want
        } else {
            damage = Area::Full;
            0
        };
        drawn.extend((0..n).map(|i| self.layers.effective(i).unwrap_or(FrameLayer::EMPTY)));
        // A layer's identity: its key and its occurrence among the layers of that key before it.
        ids.extend(drawn.iter().enumerate().map(|(i, l)| {
            let before = drawn[..i].iter().filter(|b| b.key == l.key).count();
            (l.key, before.min(usize::from(u16::MAX)) as u16)
        }));
        kept.resize(self.prev.len(), false);
        for id in &ids {
            let from = self.idents.iter().position(|was| was == id);
            if let Some(j) = from.filter(|j| *j < kept.len()) {
                kept[j] = true;
            }
            carried.push(from);
        }
        self.glass.carry(&carried);
        self.steps.clear();
        let wallpaper = seed(display.0, display.1, self.glass.wallpaper_epoch);
        let mut writes = Writes::default();
        for (i, &layer) in drawn.iter().enumerate() {
            let before = carried[i].map(|j| self.prev[j]);
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
                let rows = rows_of(&layer);
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
                let reaches = |r: &Area, o: u32| {
                    origin_below(o, i, layer.layer_id) && r.touches(reach) && !own(r, o)
                };
                let band = if full {
                    Area::Full
                } else {
                    named.iter().fold(Area::None, |a, (r, o)| {
                        if reaches(r, *o) {
                            a.union(r.intersect(reach))
                        } else {
                            a
                        }
                    })
                };
                let by_base = !full && named.iter().any(|(r, o)| *o == 0 && reaches(r, *o));
                // What the rows still owe — a write put off while nobody could see it — is
                // part of the band until a write pays it.
                let (now, owed) = (band != Area::None, self.glass.owed(i));
                let band = band.union(owed);
                let touched = band != Area::None;
                // The backdrop is what lies below the glass within its reach (RFC-0109): a
                // layer drawn elsewhere — a scroll layer's slice in another window, a hover's
                // wash across the screen — changes nothing this glass blurs.
                let below = self
                    .steps
                    .iter()
                    .filter(|s| extent(&s.layer).touches(reach))
                    .fold(wallpaper, |h, s| fold_layer(h, &s.layer));
                let key = glass_key(below, &layer);
                let miss = self.glass.miss(i, key, rows);
                let band = if miss == WriteCause::Band { band } else { Area::Full };
                // Blur at rest (TASK-0336 P3): a glass of a window in motion, or one whose
                // reach only a moving window touched, keeps its last blur — what nobody can
                // see sharp is not computed; one write when the motion rests.
                let moving = self.layers.in_motion(layer.layer_id)
                    || (now
                        && owed == Area::None
                        && named.iter().all(|(r, o)| {
                            !(origin_below(*o, i, layer.layer_id) && r.touches(reach))
                                || (*o != 0 && *o != ORIGIN_ABOVE && self.layers.in_motion(*o))
                        }));
                // Cost follows what is seen (TASK-0351 P6): a write changes the glass where its
                // backdrop did, grown by the blur's reach. Where opaque layers above hide all
                // of that — a window under the windows opened over it — nothing on the screen
                // would change: the write is put off, the rows owe it, and the frame that
                // uncovers it pays. Twenty cascaded windows blurred three or four glasses a
                // frame for a change one of them showed (`board--2026-11-18T13-29-53`).
                let shows = match band {
                    Area::Rect { .. } => band.grown(GLASS_REACH).intersect(rect),
                    _ => rect,
                };
                let hidden = (touched || miss != WriteCause::Band)
                    && px(shows).is_some_and(|q| {
                        above.clear();
                        for (j, l) in drawn.iter().enumerate().skip(i + 1) {
                            // A glass above hides with its drawn back: one that holds its rows
                            // (it reads or writes them this frame, never draws without).
                            let back = l.backdrop_blur != 0
                                && blurs(l.backdrop_blur)
                                && self.glass.keep(j, rows_of(l)).is_some();
                            let hides = occlude::opaque_rect(l, back);
                            above.extend(hides.filter(|o| occlude::intersects(*o, q)));
                        }
                        occlude::covered(q, &above)
                    });
                let put_off = if hidden { self.glass.put_off(i, key, rows, band) } else { None };
                let cache = match self.glass.keep(i, rows).filter(|_| moving && !full) {
                    Some(kept) => kept,
                    None => match put_off {
                        Some(read) => {
                            writes.hidden = writes.hidden.saturating_add(1);
                            read
                        }
                        None => self.glass.step(i, key, rows, touched),
                    },
                };
                // What the glass shows changes where its blurred backdrop did: the band grown by
                // the blur's reach, within the glass (the whole glass when blurred live or anew).
                match cache {
                    CacheStep::Read(_) => {}
                    CacheStep::Write(_) => {
                        writes.note(if full { WriteCause::Full } else { miss });
                        if miss == WriteCause::Band && by_base {
                            writes.band_base = writes.band_base.saturating_add(1);
                        }
                        damage = damage.union(band.grown(GLASS_REACH).intersect(rect));
                    }
                    CacheStep::Off => damage = damage.union(rect),
                }
                Some(Glass { cache, rows, band })
            };
            let upload = buffer_upload(&layer, before, self.layers.buffers_lost);
            self.steps.push(LayerStep { layer, glass, upload });
        }
        // A layer of the last plan no layer of this one carries is gone: where it was changes.
        for (gone, _) in self.prev.iter().zip(kept.iter()).filter(|(_, k)| !**k) {
            damage = damage.union(extent(gone));
        }
        let count = self.steps.len();
        self.idents.clear();
        self.idents.extend_from_slice(&ids);
        (self.drawn, self.ids, self.carried, self.kept) = (drawn, ids, carried, kept);
        self.above = above;
        self.layers.dirty = false;
        self.layers.buffers_lost = false;
        if upload && set_epoch != 0 {
            self.layers.uploaded_epoch = set_epoch;
        }
        Planned { layers: count, writes, upload, damage }
    }

    /// The cache grew by a plane after the last plan found it short (`GlassCache::short`): the
    /// plan's glasses without rows take theirs and are written whole this frame — their
    /// rectangles are in the plan's damage already. Returns how many were placed (writes of
    /// cause `New`); what still finds no rows is short again.
    pub fn place_short(&mut self) -> u16 {
        self.glass.add_plane();
        let mut placed = 0;
        for (i, step) in self.steps.iter_mut().enumerate() {
            let Some(g) = step.glass.as_mut().filter(|g| g.cache == CacheStep::Off) else {
                continue;
            };
            if let Some(y) = self.glass.retry(i, g.rows) {
                (g.cache, g.band) = (CacheStep::Write(y), Area::Full);
                placed += 1;
            }
        }
        placed
    }

    /// Step `i` of the last plan.
    pub fn step(&self, i: usize) -> Option<LayerStep> {
        self.steps.get(i).copied()
    }

    /// The cache step of step `i`'s glass, when it is one the executor blurs.
    pub fn cache_step(&self, i: usize) -> Option<CacheStep> {
        self.steps.get(i).and_then(|s| s.glass).map(|g| g.cache)
    }
}
