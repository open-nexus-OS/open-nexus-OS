// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: windowd compositor runtime — the motion layers a client surface declared (RFC-0109
//! §motion layers, TASK-0354 P2) moved frame by frame: a motion starts with the frame that first
//! draws it, every present carries each moving layer's transform (`Command::LayerTransform`, the
//! path of the window transitions, TASK-0336/0338) evaluated for the time the frame is shown
//! (RFC-0110), and a frame is owed while any layer still moves. No app wake and no raster per
//! frame: the planner redraws where a layer was and is, and a moving glass keeps its last blur.
//! A window's moving layer rides the window's own transform too (a drag, a transition); a layer
//! scales about its own centre (TASK-0354 P3); its backdrop fades on its own curve and never
//! moves. A close plays the token back toward where the layer came from, a re-open turns from
//! where it is (TASK-0354 P9); a layer that left keeps its transform at opacity 0 and owes no
//! frame. Instruments (not chain markers): `windowd: motion layer (… exit= tap_us=)` when a
//! segment starts and `windowd: motion done (…)` once when it ends — frames drawn while it moved
//! against the refresh periods in its length, the latest frame past a period, the largest redraw
//! of a frame drawn meanwhile (the damage whose origin is the layer's, TASK-0351 P15).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the motion in `crate::motion_geometry` and `crate::frame_transform` (host); the
//!   lanes' `windowd: motion layer (` and `windowd: motion done (`

use super::surface_layers_encode::DESKTOP_OWNER;
use super::*;
use crate::damage_origin::motion_of_key;
use crate::frame_transform::{FrameTransform, SCALE_ONE};
use crate::layer_geometry::SurfaceLayers;
use crate::motion_geometry::{
    backdrop_at, backdrop_id, drawn_moved, layer_id, moved_at, FrameNote, Said,
};

/// The motion layers of `layers` drawn through their motion's transform at `at` (bit `k`: layer
/// `k`): moving, or left (at opacity 0, never at rest); at rest the others are the surface's own
/// layers again.
pub(super) fn live(layers: &SurfaceLayers, at: u64) -> u8 {
    let mut bits = 0;
    for (k, m) in layers.tree.motions().iter().enumerate() {
        if drawn_moved(m, &layers.motion_states[k].seg, at) {
            bits |= 1 << k;
        }
    }
    bits
}

/// The frame shown at `at` draws `layers`: a motion drawn for the first time starts with it (its
/// first frame shows where it starts), a flip turns, an end is said once. What the frame redraws
/// of a motion layer is the damage whose origin is the layer's (`rects`: each with its origin's
/// key) — TASK-0351 P15: not the frame's sum, which another surface's change swells.
/// `tap_us`: from the last press routed to `owner` to this frame (`frame_causes::tap_us`).
fn stamp(
    layers: &mut SurfaceLayers,
    owner: u32,
    at: u64,
    (period_ns, paced): (u64, bool),
    rects: &[(DamageRect, u32)],
) {
    let drawn = *layers;
    // The bounding box of the motion's rectangles, not their sum: one change named under two
    // keys (CW2's per-layer origins) counted its pixels twice (TASK-0351 P15).
    let said = layers.stamp_by(at, |k| FrameNote {
        period_ns,
        damage_px: rects
            .iter()
            .filter(|(_, key)| motion_of_key(&drawn, owner, *key) == Some(k))
            .map(|(r, _)| *r)
            .reduce(|a, b| a.merge(b))
            .map_or(0, |r| u64::from(r.area())),
        paced,
    });
    for (m, said) in layers.tree.motions().iter().zip(said) {
        match said {
            Said::Nothing => {}
            Said::Started => {
                let _ = debug_println(&alloc::format!(
                    "windowd: motion layer (owner={owner} id={} token={} ms={} dx={} dy={} op={} \
                     scale={} w={} h={} exit={} tap_us={})",
                    m.id,
                    m.token,
                    m.duration_ms,
                    m.from_dx,
                    m.from_dy,
                    m.from_opacity,
                    m.start_scale(),
                    m.w,
                    m.h,
                    u8::from(m.exit),
                    crate::compositor::frame_causes::tap_us(owner, at)
                ));
            }
            Said::Done(d) => {
                let _ = debug_println(&alloc::format!(
                    "windowd: motion done (owner={owner} id={} exit={} frames={} of={} \
                     late_max_us={} damage_px_max={} paced={})",
                    m.id,
                    u8::from(m.exit),
                    d.frames,
                    d.of,
                    d.late_max_us,
                    d.damage_px_max,
                    d.paced
                ));
            }
        }
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
        let seg = &layers.motion_states[k].seg;
        if !drawn_moved(m, seg, at) {
            continue;
        }
        let mv = moved_at(m, seg, at);
        let (x, y, w, h) = m.rest();
        let centre = (origin.0 + (x + w / 2) as i32, origin.1 + (y + h / 2) as i32);
        let moved = window.with_layer((mv.dx, mv.dy), mv.opacity, mv.scale, centre);
        out.push((layer_id(owner, k), moved));
        if m.backdrop().is_some() {
            let fade = backdrop_at(m, seg, at);
            out.push((backdrop_id(owner, k), window.with_layer((0, 0), fade, SCALE_ONE, centre)));
        }
    }
}

impl DisplayServerRuntime {
    /// Every surface's motion layers drawn for the first time start with this frame, shown at
    /// `at` and redrawing `rects` (each with its origin's key); flips turn, ends are said.
    pub(super) fn stamp_motions(&mut self, at: u64, rects: &[(DamageRect, u32)]) {
        let (_, cadence, period) = self.frame_pace();
        let timing = (u64::from(period), cadence > 1);
        stamp(&mut self.desktop_layers, DESKTOP_OWNER, at, timing, rects);
        for idx in 0..self.apps.len() {
            let key = self.window_key(idx);
            stamp(&mut self.apps[idx].layers, key, at, timing, rects);
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
