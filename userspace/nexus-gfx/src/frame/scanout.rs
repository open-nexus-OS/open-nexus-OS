// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Direct scanout (RFC-0101 §8, amended 2026-11-04; TASK-0351 P3): the planner's
//! decision that a frame needs no compose — its topmost layer is an opaque, untransformed window
//! body at radius 0 without a shadow or glass, from a surface of its own, covering the display
//! whole. Such a frame's picture IS that surface: the display flips it as the plane instead of
//! a slot of the ring, and the slots stay as they are. The decision is pure and host-tested;
//! the flip of a contiguous scanout-class surface is the device server's (P9, the board).
//! OWNERS: @ui @gpu
//! STATUS: Functional (the decision; the flip follows on the board)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `tests/frame_scanout.rs`
//! INVARIANTS: only the topmost layer qualifies (nothing is above it by construction); a layer
//!   in motion, translucent, rounded, shadowed, glassed, from the shared layout, or short of
//!   the display never does.

use super::layers::RetainedLayers;

/// The index of the layer a frame can scan out directly, or `None` when the frame composes.
pub fn direct(layers: &RetainedLayers, display: (u32, u32)) -> Option<usize> {
    let i = layers.len().checked_sub(1)?;
    let l = layers.effective(i)?;
    let body_plain = l.opaque
        && l.opacity == 255
        && l.corner_radius == 0
        && l.shadow_blur == 0
        && l.shadow_alpha == 0
        && l.backdrop_blur == 0
        && l.scroll_band_h == 0;
    let whole = l.dst_x == 0
        && l.dst_y == 0
        && l.width == display.0
        && l.height == display.1
        && (l.content_w == 0 || l.content_w == l.width)
        && (l.content_h == 0 || l.content_h == l.height);
    let own = l.source != 0 && !layers.in_motion(l.layer_id);
    (body_plain && whole && own).then_some(i)
}
