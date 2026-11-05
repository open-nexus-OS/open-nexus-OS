// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: windowd compositor runtime — the motion layers a client surface declared (RFC-0109
//! §motion layers, TASK-0354 P2) moved frame by frame: a motion starts with the frame that first
//! draws it, every present carries each moving layer's transform (`Command::LayerTransform`, the
//! path of the window transitions, TASK-0336/0338) evaluated for the time the frame is shown
//! (RFC-0110), and a frame is owed while any layer still moves. No app wake and no raster per
//! frame: the planner redraws where a layer was and is, and a moving glass keeps its last blur.
//! A window's moving layer rides the window's own transform too (a drag, a transition); a layer
//! scales about its own centre (TASK-0354 P3); its backdrop fades in on its own curve and never
//! moves.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the motion in `crate::motion_geometry` and `crate::frame_transform` (host); the
//!   lanes' `windowd: motion layer (`

use super::surface_layers_encode::DESKTOP_OWNER;
use super::*;
use crate::frame_transform::{FrameTransform, SCALE_ONE};
use crate::layer_geometry::SurfaceLayers;
use crate::motion_geometry::{backdrop_at, backdrop_id, in_flight, layer_id, moved_at};

/// The motion layers of `layers` moving at `at` (bit `k`: layer `k`): their layers carry the
/// motion's transform id; at rest they are the surface's own layers again.
pub(super) fn live(layers: &SurfaceLayers, at: u64) -> u8 {
    let mut bits = 0;
    for (k, m) in layers.tree.motions().iter().enumerate() {
        if in_flight(m, layers.starts[k], at) {
            bits |= 1 << k;
        }
    }
    bits
}

/// A motion drawn for the first time starts at `at` (its first frame shows where it starts); the
/// frame being drawn is the surface's last drawn one.
fn stamp(layers: &mut SurfaceLayers, owner: u32, at: u64) {
    layers.drawn_at = at;
    for k in 0..layers.tree.motions().len() {
        let m = layers.tree.motions()[k];
        if layers.starts[k] != 0 || !in_flight(&m, 0, at) {
            continue;
        }
        layers.starts[k] = at.max(1);
        let _ = debug_println(&alloc::format!(
            "windowd: motion layer (owner={owner} id={} token={} ms={} dx={} dy={} op={} \
             scale={} w={} h={})",
            m.id,
            m.token,
            m.duration_ms,
            m.from_dx,
            m.from_dy,
            m.from_opacity,
            m.start_scale(),
            m.w,
            m.h
        ));
    }
}

/// Each moving layer of `layers` with its transform at `at`, appended to `out` — composed with
/// the transform of the window it is in (`window`; its surface's origin on the display:
/// `origin`), scaled about the layer's own centre; a layer's backdrop fades on its own curve in
/// place.
fn push(
    layers: &SurfaceLayers,
    owner: u32,
    (window, origin): (FrameTransform, (i32, i32)),
    at: u64,
    out: &mut Vec<(u32, FrameTransform)>,
) {
    for (k, m) in layers.tree.motions().iter().enumerate() {
        let start = layers.starts[k];
        if !in_flight(m, start, at) {
            continue;
        }
        let mv = moved_at(m, start, at);
        let (x, y, w, h) = m.rest();
        let centre = (origin.0 + (x + w / 2) as i32, origin.1 + (y + h / 2) as i32);
        let moved = window.with_layer((mv.dx, mv.dy), mv.opacity, mv.scale, centre);
        out.push((layer_id(owner, k), moved));
        if m.backdrop().is_some() {
            let fade = backdrop_at(m, start, at);
            out.push((backdrop_id(owner, k), window.with_layer((0, 0), fade, SCALE_ONE, centre)));
        }
    }
}

impl DisplayServerRuntime {
    /// Every surface's motion layers drawn for the first time start with this frame, shown at
    /// `at`.
    pub(super) fn stamp_motions(&mut self, at: u64) {
        stamp(&mut self.desktop_layers, DESKTOP_OWNER, at);
        for idx in 0..self.apps.len() {
            let key = self.window_key(idx);
            stamp(&mut self.apps[idx].layers, key, at);
        }
    }

    /// Each moving motion layer's transform for the present being built, appended to `out`: the
    /// desktop's as its motion draws it, a window's composed with the window's own transform.
    pub(super) fn motion_transforms(&self, out: &mut Vec<(u32, FrameTransform)>, at: u64) {
        push(&self.desktop_layers, DESKTOP_OWNER, (FrameTransform::IDENTITY, (0, 0)), at, out);
        for idx in 0..self.apps.len() {
            if self.apps[idx].surface_id.is_none() {
                continue;
            }
            let window = self.wire_transform(idx);
            let origin = (self.apps[idx].win.x, self.apps[idx].win.y);
            push(&self.apps[idx].layers, self.window_key(idx), (window, origin), at, out);
        }
    }

    /// Whether a motion layer owes a frame: not drawn yet, or still moving in the last frame drawn
    /// — until a frame draws it at rest (`motion_geometry::frame_owed`).
    pub(crate) fn motions_owed(&self) -> bool {
        self.desktop_layers.motion_frame_owed()
            || self.apps.iter().any(|a| a.layers.motion_frame_owed())
    }
}
