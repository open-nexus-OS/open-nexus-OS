// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The paged track of a `.scroll(paged)` layer (TASK-0354 P12, design artifacts: PagedView): the
//! drag follows the pointer 1:1, past either end at 35 % (also with a single page), a release
//! turns a page or returns, the wheel turns past 40 units and cools for 420 ms, and every turn
//! and return is a 500 ms glide on the glide curve (0 ms under reduced motion). The decisions are
//! `pager_math`'s; this runs them on the layer's offset — committed as motion, the compositor
//! moves the band, nothing re-rasters. The offset may leave `[0, max]` by at most one viewport
//! (`pager_math::rubber`); windowd intersects it with the band's valid range (RFC-0109).
//!
//! The press path asks through three entry points: [`DslApp::paged_layer_at`] (does a press
//! land in a paged viewport), [`DslApp::pager_drag`] (a frame of a locked drag) and
//! [`DslApp::pager_release`] (its end). One line per gesture and per glide, eight each.

use super::*;
use crate::pager_math::{
    drag_offset, last_page, notch_units, page_of, page_target_px, release_page, rubber, wheel_step,
    PageGlide, WHEEL_COOLDOWN_NS,
};
use nexus_dsl_runtime::theme_tokens::MotionDurationToken;

/// Lines per kind (drag, glide) a process writes.
const LINES: u8 = 8;

/// One paged layer's track: the page it rests on (or glides to), the glide under way, the wheel's
/// accumulator and cooldown, and the counters of the gesture and the glide for their lines.
#[derive(Clone, Copy, Default)]
pub(in crate::probe) struct PagedTrack {
    page: i32,
    glide: Option<PageGlide>,
    wheel_acc: i32,
    cool_until_ns: u64,
    drag_frames: u32,
    drag_over_px: i32,
    glide_frames: u32,
    glide_rastered: u32,
    drag_lines: u8,
    glide_lines: u8,
}

impl PagedTrack {
    /// Whether a glide is under way (the frame pulse stays armed).
    pub(super) fn gliding(&self) -> bool {
        self.glide.is_some()
    }
}

impl DslApp {
    /// The uncovered paged layer whose viewport holds `(x, y)`: a press there may become a drag
    /// of the track, so its tap waits for the release.
    pub(in crate::probe) fn paged_layer_at(&self, x: i32, y: i32) -> Option<usize> {
        self.scroll_layers.iter().position(|l| {
            l.is_some_and(|l| l.axis == Axis::Paged && !l.covered && l.contains(x, y))
        })
    }

    /// One frame of a drag locked to the track of layer `k`, `dx` px from the press (a drag to
    /// the left moves toward the next page): the offset follows, past an end on the rubber band;
    /// a glide under way stops where it is.
    pub(in crate::probe) fn pager_drag(&mut self, k: usize, dx: i32) {
        let Some(mut layer) = self.scroll_layers.get(k).copied().flatten() else { return };
        let (view, max) = (layer.viewport_len(), layer.max_offset());
        let mut track = layer.track;
        track.glide = None;
        let offset = drag_offset(page_target_px(track.page, view, max), dx, max, view);
        track.drag_frames += 1;
        track.drag_over_px = track.drag_over_px.max((-offset).max(offset - max).max(0));
        layer.track = track;
        if offset != layer.offset {
            layer.offset = offset;
            self.motion_dirty = true;
        }
        self.scroll_layers[k] = Some(layer);
    }

    /// The end of a drag locked to the track of layer `k` (`dx` px over `dt_ns`): past 16 % of
    /// the width or after a flick a page turns — `PageNext`/`PagePrev` fire on the container, so
    /// the store follows — else the track returns; either way a glide from where it is. Whether
    /// the model changed. The caller arms the frame pulse while `momentum_active`.
    pub(in crate::probe) fn pager_release(&mut self, k: usize, dx: i32, dt_ns: u64) -> bool {
        let Some(mut layer) = self.scroll_layers.get(k).copied().flatten() else { return false };
        let (view, max) = (layer.viewport_len(), layer.max_offset());
        let from = layer.track.page;
        let to = release_page(from, last_page(max, view), dx, dt_ns, view);
        if layer.track.drag_lines < LINES {
            layer.track.drag_lines += 1;
            raw_marker(&alloc::format!(
                "apphost: page drag (dx={dx} dt_ms={} over_px={} turn={} frames={})",
                dt_ns / 1_000_000,
                layer.track.drag_over_px,
                to - from,
                layer.track.drag_frames
            ));
        }
        layer.track.drag_frames = 0;
        layer.track.drag_over_px = 0;
        self.scroll_layers[k] = Some(layer);
        self.pager_glide_to(k, to, nsec_now());
        match to - from {
            0 => false,
            d if d > 0 => self.fire_at(layer.root, "PageNext"),
            _ => self.fire_at(layer.root, "PagePrev"),
        }
    }

    /// A wheel event of `notches` detents over paged layer `k`: past 40 units a page turns and
    /// the wheel cools for 420 ms (the trigger the caller fires), else it accumulates.
    pub(super) fn paged_wheel(&mut self, k: usize, notches: i32, now: u64) -> Option<&'static str> {
        let mut layer = self.scroll_layers.get(k).copied().flatten()?;
        let (view, max) = (layer.viewport_len(), layer.max_offset());
        let (acc, turn) = wheel_step(
            layer.track.wheel_acc,
            notch_units(notches),
            now < layer.track.cool_until_ns,
        );
        layer.track.wheel_acc = acc;
        let dir = turn?;
        let to = (layer.track.page + dir).clamp(0, last_page(max, view));
        layer.track.cool_until_ns = now.saturating_add(WHEEL_COOLDOWN_NS);
        self.scroll_layers[k] = Some(layer);
        if to == layer.track.page {
            return None;
        }
        self.pager_glide_to(k, to, now);
        Some(if dir > 0 { "PageNext" } else { "PagePrev" })
    }

    /// Starts layer `k`'s glide from the offset it has to page `page` (a turn, a return, a dot
    /// tap): the theme's `slow` step on the glide curve, at once under reduced motion.
    pub(super) fn pager_glide_to(&mut self, k: usize, page: i32, now: u64) {
        let Some(mut layer) = self.scroll_layers.get(k).copied().flatten() else { return };
        let (view, max) = (layer.viewport_len(), layer.max_offset());
        let page = page.clamp(0, last_page(max, view));
        let to = page_target_px(page, view, max);
        let ms = tokens_for(self.theme_mode).motion_ms(MotionDurationToken::Slow);
        layer.track.page = page;
        layer.track.glide = Some(PageGlide {
            from_px: layer.offset,
            to_px: to,
            start_ns: now,
            dur_ns: u64::from(ms).saturating_mul(1_000_000),
        });
        self.scroll_layers[k] = Some(layer);
    }

    /// One frame of layer `k`'s glide at `now`: the offset it has then, and at the end its line.
    pub(super) fn paged_tick(&mut self, k: usize, now: u64) {
        let Some(mut layer) = self.scroll_layers.get(k).copied().flatten() else { return };
        let Some(glide) = layer.track.glide else { return };
        let before = self.pending;
        let (at, done) = glide.at(now);
        let offset = rubber(at, layer.max_offset(), layer.viewport_len());
        if offset != layer.offset {
            layer.offset = offset;
            self.motion_dirty = true;
        }
        layer.track.glide_frames += 1;
        if done {
            layer.track.glide = None;
            if layer.track.glide_lines < LINES {
                layer.track.glide_lines += 1;
                raw_marker(&alloc::format!(
                    "apphost: page glide (from={} to={} ms={} frames={} rastered={})",
                    glide.from_px,
                    glide.to_px,
                    glide.dur_ns / 1_000_000,
                    layer.track.glide_frames,
                    layer.track.glide_rastered
                ));
            }
            layer.track.glide_frames = 0;
            layer.track.glide_rastered = 0;
            // The page the track rests on is the one the offset shows.
            layer.track.page = page_of(offset, layer.viewport_len());
        }
        self.scroll_layers[k] = Some(layer);
        self.rehover();
        if let Some(mut l) = self.scroll_layers[k] {
            l.track.glide_rastered += u32::from(self.pending != before);
            self.scroll_layers[k] = Some(l);
        }
    }
}
