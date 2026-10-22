// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The retained layer set (TASK-0215 5c-3 P4): the `CompositeLayer` commands of the last present
//! that brought layers, with the windows' transforms the presents carry (TASK-0336/0338: presents
//! are the only source of a frame — a scroll or a move is a present). A present without layers
//! keeps the set; every frame composes it whole. Moved here from the QEMU path's backend so the
//! board's executor draws the same set.

use super::damage::Area;
use crate::command::buffer::Command;

/// The set's capacity: a real desktop — four windows, each its body, its regions, its scroll
/// layers' slices and its washes (RFC-0109), the desktop's base, regions and bar. windowd never
/// encodes more (it stops at the bound and says so).
pub const MAX_LAYERS: usize = 64;

/// Transform ids, `1..=MAX_IDS` (`0` = none): a window's slot plus one.
pub const MAX_IDS: usize = 8;

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
    /// Scale about the layer's centre, percent (`100` = unchanged).
    pub scale_pct: u16,
}

impl Transform {
    /// The transform applied: moved, its opacity multiplied, scaled about its centre — the
    /// content band keeps its size, so the executor spreads it over the scaled layer.
    pub fn apply(&self, l: &mut FrameLayer) {
        l.dst_x = (l.dst_x as i32 + i32::from(self.dx)).max(0) as u32;
        l.dst_y = (l.dst_y as i32 + i32::from(self.dy)).max(0) as u32;
        l.opacity = l.opacity * u32::from(self.opacity) / 255;
        if self.scale_pct != 100 && self.scale_pct > 0 {
            let nw = (l.width * u32::from(self.scale_pct) / 100).max(1);
            let nh = (l.height * u32::from(self.scale_pct) / 100).max(1);
            if l.content_w == 0 {
                l.content_w = l.width;
            }
            if l.content_h == 0 {
                l.content_h = l.height;
            }
            l.dst_x = (l.dst_x as i32 + (l.width as i32 - nw as i32) / 2).max(0) as u32;
            l.dst_y = (l.dst_y as i32 + (l.height as i32 - nh as i32) / 2).max(0) as u32;
            l.width = nw;
            l.height = nh;
        }
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
    transforms: [Option<Transform>; MAX_IDS],
    /// The set changed since the frame last decided its atlas upload.
    pub(super) dirty: bool,
    /// The set generation the atlas was last uploaded at (`0` = none yet).
    pub(super) uploaded_epoch: u32,
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
            transforms: [None; MAX_IDS],
            dirty: true,
            uploaded_epoch: 0,
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
    /// row); the transforms stay (the encode is the untransformed base, each present carries
    /// every window's). No layers: the set stays. The present's damage is recorded for the plan:
    /// its `Damage` rectangles' union — none for `TileRect::NONE` — or everywhere when layers
    /// came without one.
    pub fn take_frame(&mut self, cmds: &[Command]) -> Taken {
        // The present's transforms first: they hold from this present's layers on (TASK-0336),
        // whether or not the present brings layers.
        for c in cmds {
            if let Command::LayerTransform { layer_id, dx, dy, opacity, scale_pct } = *c {
                let t = Transform {
                    dx: dx.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16,
                    dy: dy.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16,
                    opacity: opacity.min(255) as u8,
                    scale_pct: scale_pct.clamp(1, 400) as u16,
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
        self.transforms = [None; MAX_IDS];
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

    /// The transform of layer id `layer_id` from now on; `false` for id 0 or beyond the table.
    pub fn set_transform(&mut self, layer_id: u32, t: Transform) -> bool {
        match layer_id.checked_sub(1).and_then(|i| self.transforms.get_mut(i as usize)) {
            Some(slot) => {
                *slot = Some(t);
                true
            }
            None => false,
        }
    }

    /// The uploaded atlas is no longer trusted (a batch carrying its upload was abandoned): the
    /// next frame uploads it again.
    pub fn invalidate_upload(&mut self) {
        self.uploaded_epoch = 0;
        self.dirty = true;
    }

    /// Layer `i` as the frame draws it: its window's transform applied.
    pub fn effective(&self, i: usize) -> Option<FrameLayer> {
        let mut l = *self.layers.get(i).filter(|_| i < self.count)?;
        if let Some(t) = l
            .layer_id
            .checked_sub(1)
            .and_then(|id| self.transforms.get(id as usize))
            .copied()
            .flatten()
        {
            t.apply(&mut l);
        }
        Some(l)
    }

    /// Whether the window of transform id `layer_id` is in motion: a transform that is not the
    /// identity (moved, scaled or faded). A glass of a window in motion keeps its last blur
    /// (TASK-0336 P3: blur at rest) — what moves is not re-blurred until it stops.
    pub fn in_motion(&self, layer_id: u32) -> bool {
        layer_id
            .checked_sub(1)
            .and_then(|id| self.transforms.get(id as usize))
            .copied()
            .flatten()
            .is_some_and(|t| t.dx != 0 || t.dy != 0 || t.opacity != 255 || t.scale_pct != 100)
    }

    /// The highest content generation of the set (`0`: no layer is stamped).
    pub(super) fn set_epoch(&self) -> u32 {
        self.layers[..self.count].iter().map(|l| l.content_epoch).max().unwrap_or(0)
    }
}

impl Default for RetainedLayers {
    fn default() -> Self {
        Self::new()
    }
}
