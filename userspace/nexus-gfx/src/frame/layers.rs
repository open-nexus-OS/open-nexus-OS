// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The retained layer set (TASK-0215 5c-3 P4): the `CompositeLayer` commands of the last present
//! that brought layers, with the windows' transforms the presents carry (TASK-0336/0338: presents
//! are the only source of a frame — a scroll or a move is a present). A present without layers
//! keeps the set; every frame composes it whole. Moved here from the QEMU path's backend so the
//! board's executor draws the same set.

use super::damage::Area;
use crate::command::buffer::{Command, SCALE_ONE};

/// The set's capacity — the frame's layer budget: every window its body, its regions, its scroll
/// layers' slices and its washes (RFC-0109), the desktop's base, regions and bar, fair-shared
/// among the windows on screen (windowd never encodes more: it stops at the bound and says so).
pub const MAX_LAYERS: usize = 64;

/// The windows' transforms a set holds: at most one per window it draws — at most one per layer.
/// A transform id is a window's key (RFC-0114 §7: `1..=0xFFFE`, the desktop `0xFFFF`; `0` = none).
const TRANSFORMS_MAX: usize = MAX_LAYERS;

/// One `CompositeLayer` as the frame keeps it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameLayer {
    /// The source's first row in the shared layout (the atlas's rows are absolute).
    pub src_row_abs: u32,
    pub src_x: u32,
    pub width: u32,
    pub height: u32,
    /// The content band's size (`0` = the layer's): a smaller band is spread over the layer.
    pub content_w: u32,
    pub content_h: u32,
    pub dst_x: u32,
    pub dst_y: u32,
    /// `0..=255`.
    pub opacity: u32,
    pub corner_radius: u32,
    pub shadow_blur: u32,
    pub shadow_offset_y: i32,
    pub shadow_alpha: u32,
    /// The glass's blur radius (`0` = no glass).
    pub backdrop_blur: u32,
    /// The scroll layer's band a slice samples (RFC-0109; `0` = not a slice): a backend that
    /// uploads the atlas uploads it whole, so the slice at a later offset samples rows already
    /// there.
    pub scroll_band_top_abs: u32,
    pub scroll_band_h: u32,
    /// The window transform's id (`0` = none); every slice of a window shares it.
    pub layer_id: u32,
    /// The atlas generation of the layer's content (`0` = unknown).
    pub content_epoch: u32,
    /// Opaque inside its rounded rectangle (`LAYER_OPAQUE`): an occluder (TASK-0337).
    pub opaque: bool,
    /// One colour (`LAYER_SOLID`): any part of its band draws any part of it (TASK-0336).
    pub solid: bool,
    /// The layer's identity across presents (RFC-0109; `0` = unnamed): the plan matches a layer
    /// to its last frame's by this key and its occurrence among equal keys.
    pub key: u32,
    /// What the layer samples (RFC-0114 §6): `0` the shared layout (`src_row_abs` absolute);
    /// else a gpud surface buffer — `src_row_abs`, `src_x` and the band are its slot's own.
    pub source: u32,
    /// The slot of `source` sampled (`0` for the layout).
    pub slot: u8,
}

impl FrameLayer {
    /// No layer: every field zero (the sets' and plans' fill).
    pub const EMPTY: Self = Self {
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
        scroll_band_top_abs: 0,
        scroll_band_h: 0,
        layer_id: 0,
        content_epoch: 0,
        opaque: false,
        solid: false,
        key: 0,
        source: 0,
        slot: 0,
    };

    /// The layer a `CompositeLayer` command describes; `None` for any other command.
    pub fn from_command(cmd: &Command) -> Option<Self> {
        let Command::CompositeLayer {
            src_row_abs,
            src_x,
            width,
            height,
            dst_x,
            dst_y,
            opacity,
            corner_radius,
            shadow_blur,
            shadow_offset_y,
            shadow_alpha,
            backdrop_blur,
            content_w,
            content_h,
            scroll_band_top_abs,
            scroll_band_h,
            layer_id,
            content_epoch,
            flags,
            key,
            source,
        } = *cmd
        else {
            return None;
        };
        Some(Self {
            src_row_abs,
            src_x,
            width,
            height,
            content_w,
            content_h,
            dst_x,
            dst_y,
            opacity,
            corner_radius,
            shadow_blur,
            shadow_offset_y,
            shadow_alpha,
            backdrop_blur,
            scroll_band_top_abs,
            scroll_band_h,
            layer_id,
            content_epoch,
            opaque: flags & super::super::command::buffer::LAYER_OPAQUE != 0,
            solid: flags & super::super::command::buffer::LAYER_SOLID != 0,
            key,
            source,
            slot: crate::command::layer_wire::layer_slot(flags),
        })
    }
}

/// A window's transform as a present carries it (windowd sends every window's in every present,
/// the identity at rest — so a slot's next tenant never inherits a transform).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Transform {
    pub dx: i16,
    pub dy: i16,
    /// Multiplies the layer's opacity (`255` = unchanged).
    pub opacity: u8,
    /// Scale in 1/10000 (`SCALE_ONE` = unchanged) about `origin`.
    pub scale: u16,
    /// The point every layer of the transform scales about (display pixels): a group's layers
    /// stay together while it grows.
    pub origin: (i16, i16),
}

/// `a / b` rounded to the nearest integer, halves away from zero (`b > 0`).
fn div_round(a: i64, b: i64) -> i64 {
    if a >= 0 {
        (a + b / 2) / b
    } else {
        -((-a + b / 2) / b)
    }
}

impl Transform {
    /// At rest: nothing moved, faded or scaled.
    pub const IDENTITY: Self =
        Self { dx: 0, dy: 0, opacity: 255, scale: SCALE_ONE as u16, origin: (0, 0) };

    /// The transform applied: scaled about its origin, then moved, its opacity multiplied — the
    /// content band keeps its size, so the executor spreads it over the scaled layer.
    pub fn apply(&self, l: &mut FrameLayer) {
        l.opacity = l.opacity * u32::from(self.opacity) / 255;
        let (mut x, mut y) = (i64::from(l.dst_x), i64::from(l.dst_y));
        if u32::from(self.scale) != SCALE_ONE && self.scale > 0 {
            let s = i64::from(self.scale);
            let one = i64::from(SCALE_ONE);
            let nw = div_round(i64::from(l.width) * s, one).max(1) as u32;
            let nh = div_round(i64::from(l.height) * s, one).max(1) as u32;
            if l.content_w == 0 {
                l.content_w = l.width;
            }
            if l.content_h == 0 {
                l.content_h = l.height;
            }
            let (ox, oy) = (i64::from(self.origin.0), i64::from(self.origin.1));
            x = ox + div_round((x - ox) * s, one);
            y = oy + div_round((y - oy) * s, one);
            l.width = nw;
            l.height = nh;
        }
        l.dst_x = (x + i64::from(self.dx)).max(0) as u32;
        l.dst_y = (y + i64::from(self.dy)).max(0) as u32;
    }
}

/// What a present's commands did to the set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Taken {
    /// The present brought layers: they are the set now.
    pub replaced: bool,
    /// Layers past the capacity, not kept (a missing window on screen: the caller says so).
    pub dropped: usize,
}

/// The retained set and the windows' transforms.
pub struct RetainedLayers {
    layers: [FrameLayer; MAX_LAYERS],
    count: usize,
    /// `(transform id, transform)` of the windows the last presents named.
    transforms: [(u32, Transform); TRANSFORMS_MAX],
    transform_count: usize,
    /// The set changed since the frame last decided its atlas upload.
    pub(super) dirty: bool,
    /// The set generation the atlas was last uploaded at (`0` = none yet).
    pub(super) uploaded_epoch: u32,
    /// The executor lost what it had uploaded of the surface buffers (RFC-0114): the next plan
    /// uploads every buffer layer.
    pub(super) buffers_lost: bool,
    /// What the last present said changed on the screen (RFC-0101 §8): its `Damage` commands,
    /// each with its origin, up to `MAX_DAMAGE`; `damage_full` when a present brought layers and
    /// named no damage, or named more than kept. Taken by the plan.
    pub(super) damage: [(Area, u32); MAX_DAMAGE],
    pub(super) damage_count: usize,
    pub(super) damage_full: bool,
    /// The count and the whole-screen flag of the damage the last plan took.
    last: (usize, bool),
}

/// Damage rectangles a present's set keeps (windowd names at most seven).
pub const MAX_DAMAGE: usize = 8;

/// The origin of damage above every layer (the pointer): below no glass.
pub const ORIGIN_ABOVE: u32 = u32::MAX;

impl RetainedLayers {
    pub const fn new() -> Self {
        Self {
            layers: [FrameLayer::EMPTY; MAX_LAYERS],
            count: 0,
            transforms: [(0, Transform::IDENTITY); TRANSFORMS_MAX],
            transform_count: 0,
            dirty: true,
            uploaded_epoch: 0,
            buffers_lost: true,
            damage: [(Area::None, 0); MAX_DAMAGE],
            damage_count: 0,
            damage_full: false,
            last: (0, false),
        }
    }

    /// Whether the plan draws everything a present of `cmds` shows: its layers (no more than the
    /// set holds); the CPU's recipe beside each glass layer — the backdrop restored from the
    /// retained plane (`BlitSurface`, also the damage blits), blurred (`BlurBackdrop`) and
    /// copied to or from its cache (`BlitAbsolute`), which the plan's own glass supersedes (the
    /// layer encoder emits them with every glass `CompositeLayer`, cycle 30); and the pointer
    /// (the display's own — a hardware layer or the executor's overlay). A fill, a shadow of its
    /// own or any other command is drawn by the CPU alone: the plan would lose it.
    pub fn covers(cmds: &[Command]) -> bool {
        let mut layers = 0;
        cmds.iter().all(|c| match c {
            Command::CompositeLayer { .. } => {
                layers += 1;
                layers <= MAX_LAYERS
            }
            Command::BlitSurface { .. }
            | Command::BlurBackdrop { .. }
            | Command::BlitAbsolute { .. }
            | Command::BlendCursor { .. }
            | Command::Damage { .. }
            | Command::LayerTransform { .. } => true,
            _ => false,
        })
    }

    /// A present's commands. Layers replace the set (a scrolled body comes with its new source
    /// row) and the windows' transforms with it: a present with layers names every window's
    /// (the encode is the untransformed base) — a window it names none of is at rest, and a
    /// closed window's transform goes with it (RFC-0114 §7: keys are never a table index). No
    /// layers: the set stays, a transform it names updates. The present's damage is recorded for
    /// the plan: its `Damage` rectangles' union — none for `TileRect::NONE` — or everywhere when
    /// layers came without one.
    pub fn take_frame(&mut self, cmds: &[Command]) -> Taken {
        // The present's transforms first: they hold from this present's layers on (TASK-0336),
        // whether or not the present brings layers.
        if cmds.iter().any(|c| matches!(c, Command::CompositeLayer { .. })) {
            self.transform_count = 0;
        }
        for c in cmds {
            if let Command::LayerTransform {
                layer_id,
                dx,
                dy,
                opacity,
                scale,
                origin_x,
                origin_y,
            } = *c
            {
                let i16_of = |v: i32| v.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
                let t = Transform {
                    dx: i16_of(dx),
                    dy: i16_of(dy),
                    opacity: opacity.min(255) as u8,
                    scale: scale.clamp(100, 40_000) as u16,
                    origin: (i16_of(origin_x), i16_of(origin_y)),
                };
                let _ = self.set_transform(layer_id, t);
            }
        }
        let mut named = false;
        for c in cmds {
            if let Command::Damage { rect, origin } = c {
                named = true;
                let area = Area::rect(rect.x, rect.y, rect.width, rect.height);
                match self.damage.get_mut(self.damage_count) {
                    Some(slot) if area != Area::None => {
                        *slot = (area, *origin);
                        self.damage_count += 1;
                    }
                    Some(_) => {}
                    None => self.damage_full = true,
                }
            }
        }
        let replaced = cmds.iter().any(|c| matches!(c, Command::CompositeLayer { .. }));
        if replaced && !named {
            self.damage_full = true;
        }
        if !replaced {
            return Taken { replaced: false, dropped: 0 };
        }
        self.count = 0;
        self.dirty = true;
        let mut dropped = 0;
        for layer in cmds.iter().filter_map(FrameLayer::from_command) {
            match self.layers.get_mut(self.count) {
                Some(slot) => {
                    *slot = layer;
                    self.count += 1;
                }
                None => dropped += 1,
            }
        }
        Taken { replaced: true, dropped }
    }

    /// The set emptied, its transforms with it (an executor's own scene drawn before the
    /// compositor's first — the next present with layers replaces nothing).
    pub fn clear(&mut self) {
        self.count = 0;
        self.dirty = true;
        self.transform_count = 0;
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// The present's damage, taken: its rectangles with their origins (none when a present named
    /// none, or everything when it brought layers and named none).
    pub(super) fn take_damage(&mut self) -> ([(Area, u32); MAX_DAMAGE], usize, bool) {
        let out = (self.damage, self.damage_count, self.damage_full);
        self.last = (self.damage_count, self.damage_full);
        self.damage_count = 0;
        self.damage_full = false;
        out
    }

    /// The damage the last plan took: its rectangles with their origins (an executor's line).
    pub fn last_damage(&self) -> (&[(Area, u32)], usize, bool) {
        (&self.damage[..], self.last.0, self.last.1)
    }

    /// The index of the first layer carrying transform id `layer_id`, if one does.
    pub fn index_of_id(&self, layer_id: u32) -> Option<usize> {
        self.layers[..self.count].iter().position(|l| l.layer_id == layer_id)
    }

    /// The transform of layer id `layer_id` from now on; `false` for id 0, or when the set holds
    /// a transform for as many windows as it can draw.
    pub fn set_transform(&mut self, layer_id: u32, t: Transform) -> bool {
        if layer_id == 0 {
            return false;
        }
        let held = &mut self.transforms[..self.transform_count];
        if let Some(slot) = held.iter_mut().find(|(id, _)| *id == layer_id) {
            slot.1 = t;
            return true;
        }
        match self.transforms.get_mut(self.transform_count) {
            Some(slot) => {
                *slot = (layer_id, t);
                self.transform_count += 1;
                true
            }
            None => false,
        }
    }

    /// The transform of layer id `layer_id`, if a present named one.
    fn transform_of(&self, layer_id: u32) -> Option<Transform> {
        let held = &self.transforms[..self.transform_count];
        held.iter().find(|(id, _)| *id == layer_id && layer_id != 0).map(|(_, t)| *t)
    }

    /// The uploaded atlas is no longer trusted (a batch carrying its upload was abandoned): the
    /// next frame uploads it again.
    pub fn invalidate_upload(&mut self) {
        self.uploaded_epoch = 0;
        self.buffers_lost = true;
        self.dirty = true;
    }

    /// Layer `i` as the frame draws it: its window's transform applied.
    pub fn effective(&self, i: usize) -> Option<FrameLayer> {
        let mut l = *self.layers.get(i).filter(|_| i < self.count)?;
        if let Some(t) = self.transform_of(l.layer_id) {
            t.apply(&mut l);
        }
        Some(l)
    }

    /// Whether the window of transform id `layer_id` is in motion: a transform that is not the
    /// identity (moved, scaled or faded). A glass of a window in motion keeps its last blur
    /// (TASK-0336 P3: blur at rest) — what moves is not re-blurred until it stops.
    pub fn in_motion(&self, layer_id: u32) -> bool {
        self.transform_of(layer_id).is_some_and(|t| {
            t.dx != 0 || t.dy != 0 || t.opacity != 255 || u32::from(t.scale) != SCALE_ONE
        })
    }

    /// The highest content generation of the set (`0`: no layer is stamped).
    pub(super) fn set_epoch(&self) -> u32 {
        let layout = self.layers[..self.count].iter().filter(|l| l.source == 0);
        layout.map(|l| l.content_epoch).max().unwrap_or(0)
    }
}

impl Default for RetainedLayers {
    fn default() -> Self {
        Self::new()
    }
}
