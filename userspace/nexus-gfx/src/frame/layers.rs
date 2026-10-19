// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The retained layer set (TASK-0215 5c-3 P4): the `CompositeLayer` commands of the last present
//! that brought layers, with the scroll and transform overrides recorded since. A present without
//! layers (a pointer move) keeps the set; every frame composes it whole. Moved here from the
//! QEMU path's backend so the board's executor draws the same set.

use crate::command::buffer::Command;

/// The set's capacity: a real desktop — four windows of three band slices and a title overlay
/// each, their glass regions, the dock and the top bar.
pub const MAX_LAYERS: usize = 32;

/// Scroll and transform ids, `1..=MAX_IDS` (`0` = none): a window's slot plus one.
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
    /// The scroll override's id (`0` = not scrollable).
    pub scroll_id: u32,
    /// The scroll band resident in the atlas, the override shifting within it.
    pub scroll_band_top_abs: u32,
    pub scroll_band_h: u32,
    /// The transform override's id (`0` = none); every slice of a window shares it.
    pub layer_id: u32,
    /// The atlas generation of the layer's content (`0` = unknown).
    pub content_epoch: u32,
}

impl FrameLayer {
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
            scroll_id,
            content_w,
            content_h,
            scroll_band_top_abs,
            scroll_band_h,
            layer_id,
            content_epoch,
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
            scroll_id,
            scroll_band_top_abs,
            scroll_band_h,
            layer_id,
            content_epoch,
        })
    }
}

/// A transform override (an identity one is recorded too: windowd retires an override by
/// sending it).
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
    /// The override applied: moved, its opacity multiplied, scaled about its centre — the
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

/// The retained set and its overrides.
pub struct RetainedLayers {
    layers: [FrameLayer; MAX_LAYERS],
    count: usize,
    scroll_rows: [Option<u32>; MAX_IDS],
    transforms: [Option<Transform>; MAX_IDS],
    /// The set changed since the frame last decided its atlas upload.
    pub(super) dirty: bool,
    /// The set generation the atlas was last uploaded at (`0` = none yet).
    pub(super) uploaded_epoch: u32,
}

impl RetainedLayers {
    pub const fn new() -> Self {
        Self {
            layers: [FrameLayer {
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
            }; MAX_LAYERS],
            count: 0,
            scroll_rows: [None; MAX_IDS],
            transforms: [None; MAX_IDS],
            dirty: true,
            uploaded_epoch: 0,
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
            | Command::BlendCursor { .. } => true,
            _ => false,
        })
    }

    /// A present's commands. Layers replace the set — the scroll overrides go with it (the
    /// encode carries the scroll offset), the transform overrides stay (the encode is the
    /// untransformed base; windowd retires an override itself). No layers: the set stays.
    pub fn take_frame(&mut self, cmds: &[Command]) -> Taken {
        if !cmds.iter().any(|c| matches!(c, Command::CompositeLayer { .. })) {
            return Taken { replaced: false, dropped: 0 };
        }
        self.count = 0;
        self.dirty = true;
        self.scroll_rows = [None; MAX_IDS];
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

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// The source row of scroll id `scroll_id` from now on; `false` for id 0 or beyond the table.
    pub fn set_scroll(&mut self, scroll_id: u32, src_row_abs: u32) -> bool {
        match scroll_id.checked_sub(1).and_then(|i| self.scroll_rows.get_mut(i as usize)) {
            Some(slot) => {
                *slot = Some(src_row_abs);
                true
            }
            None => false,
        }
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

    /// Layer `i` as the frame draws it: its transform override applied, its source row the
    /// scroll override's when one is recorded.
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
        if let Some(row) = l
            .scroll_id
            .checked_sub(1)
            .and_then(|id| self.scroll_rows.get(id as usize))
            .copied()
            .flatten()
        {
            l.src_row_abs = row;
        }
        Some(l)
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
