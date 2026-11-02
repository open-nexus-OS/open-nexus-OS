// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The frame planner's test vocabulary, shared by `frame_plan.rs` and `frame_damage.rs`: a
//! layer, a layer changed in place, a glass, a ready frame, a step's cache and a present's damage.

use nexus_gfx::command::buffer::Command;
use nexus_gfx::core::types::TileRect;
use nexus_gfx::frame::{CacheStep, Frame, FrameLayer};

pub const DISPLAY: (u32, u32) = (1920, 1080);

pub fn layer(x: u32, y: u32, w: u32, h: u32) -> Command {
    Command::CompositeLayer {
        src_row_abs: 5400,
        src_x: 0,
        width: w,
        height: h,
        dst_x: x,
        dst_y: y,
        opacity: 255,
        corner_radius: 0,
        shadow_blur: 0,
        shadow_offset_y: 0,
        shadow_alpha: 0,
        backdrop_blur: 0,
        content_w: 0,
        content_h: 0,
        scroll_band_top_abs: 0,
        scroll_band_h: 0,
        layer_id: 0,
        content_epoch: 0,
        flags: 0,
        key: 0,
        source: 0,
    }
}

pub fn with(cmd: Command, f: impl FnOnce(&mut FrameLayer)) -> Command {
    let mut l = FrameLayer::from_command(&cmd).expect("a layer");
    f(&mut l);
    Command::CompositeLayer {
        src_row_abs: l.src_row_abs,
        src_x: l.src_x,
        width: l.width,
        height: l.height,
        dst_x: l.dst_x,
        dst_y: l.dst_y,
        opacity: l.opacity,
        corner_radius: l.corner_radius,
        shadow_blur: l.shadow_blur,
        shadow_offset_y: l.shadow_offset_y,
        shadow_alpha: l.shadow_alpha,
        backdrop_blur: l.backdrop_blur,
        content_w: l.content_w,
        content_h: l.content_h,
        scroll_band_top_abs: l.scroll_band_top_abs,
        scroll_band_h: l.scroll_band_h,
        layer_id: l.layer_id,
        content_epoch: l.content_epoch,
        flags: (if l.opaque { nexus_gfx::command::buffer::LAYER_OPAQUE } else { 0 })
            | (if l.solid { nexus_gfx::command::buffer::LAYER_SOLID } else { 0 }),
        key: l.key,
        source: l.source,
    }
}

pub fn glass(cmd: Command, blur: u32) -> Command {
    with(cmd, |l| l.backdrop_blur = blur)
}

pub fn ready_frame(rows: u32) -> Frame {
    let mut f = Frame::new(rows);
    f.glass.ready = true;
    f
}

pub fn every_blur(_: u32) -> bool {
    true
}

pub fn cache_of(f: &Frame, i: usize) -> Option<CacheStep> {
    f.step(i).and_then(|s| s.glass).map(|g| g.cache)
}

/// A present's damage rectangle (from the compositor itself, origin 0).
pub fn damage(x: u32, y: u32, w: u32, h: u32) -> Command {
    Command::Damage { rect: TileRect { x, y, width: w, height: h }, origin: 0 }
}
