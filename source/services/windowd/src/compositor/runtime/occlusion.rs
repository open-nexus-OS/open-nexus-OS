// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: A window hidden behind others costs the frame nothing (TASK-0351 P1): before the
//! scene is encoded, the windows are walked FRONT TO BACK, and a window whose body and shadow lie
//! wholly under the union of the opaque rectangles of the windows above it is left out of the
//! frame — no body, no declared layers, no caret, no layer of the budget, no read of its surface.
//! What a body hides is the planner's rule (RFC-0101 §8, `nexus_gfx::frame::occlude`): the
//! inside of its rounded rectangle — its cross, the middle column and the middle row, every pixel
//! of which is at coverage one — when it is drawn opaque: a solid body, or a glass body whose
//! blurred back the GPU draws, at full opacity and at the identity transform. A body in motion (a
//! transition, a drag), fading, or spread over a resizing frame hides nothing and is always drawn.
//! The coverage test is exact (`occlude::covered`: the union, never one occluder); what lies off
//! the display counts for nothing (the extent is clipped to it). On the CPU path (no GPU context:
//! the headless lanes) a glass body is blended by the executor and hides nothing, so nothing is
//! culled there — the line is the GPU path's (virgl, the board).
//! OWNERS: @ui @gpu
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the rule's halves on the host in nexus-gfx (`cross`, `covered`); the model of
//!   the cascade under a filled window in `surface_presentation.rs`; the many-windows lane
//!   (`windowd: windows occluded (n=`); the board
//! INVARIANTS: a window left out is one no pixel of which the frame would show; a culled window
//!   keeps its transform, its input and its surface — it is not hidden, only not drawn.

use alloc::vec::Vec;

use nexus_gfx::frame::occlude::{self, Px};

use crate::compositor::shell_window::GlassCompositeParams;

/// The identity transform on the wire: no offset, full opacity, scale 100 %.
pub(super) const IDENTITY: (i32, i32, u32, u32) = (0, 0, 255, 100);

/// What a window's body takes from the screen and what it hides.
pub(super) struct Footprint {
    /// The body with its shadow, on the display: what must be covered for the window to cost
    /// nothing.
    pub(super) extent: Px,
    /// What the body hides of the windows below it: its cross (`None`s: a translucent body).
    pub(super) occluder: [Option<Px>; 2],
}

impl Footprint {
    /// The footprint of a body drawn at `transform` on `display`; `None` for a body in motion,
    /// spread over a resizing frame or off the display — such a body is drawn whatever lies above
    /// it and hides nothing.
    pub(super) fn of(
        p: &GlassCompositeParams,
        transform: (i32, i32, u32, u32),
        display: (u32, u32),
    ) -> Option<Self> {
        if transform != IDENTITY || p.content_w != 0 || p.w == 0 || p.h == 0 {
            return None;
        }
        let pad = if p.shadow_alpha > 0 && p.shadow_blur > 0 {
            p.shadow_blur + p.shadow_offset_y.unsigned_abs()
        } else {
            0
        };
        let halo = (p.x.saturating_sub(pad), p.y.saturating_sub(pad), p.w + 2 * pad, p.h + 2 * pad);
        let extent = occlude::intersection(halo, (0, 0, display.0, display.1))?;
        // The body is opaque when solid, or when the GPU composes its glass (its blurred back is
        // written over what lies below) — at full opacity.
        let opaque = (p.solid || p.gpu_glass) && p.opacity == 255;
        let occluder =
            if opaque { occlude::cross((p.x, p.y, p.w, p.h), p.radius) } else { [None, None] };
        Some(Self { extent, occluder })
    }
}

/// Walks the windows FRONT TO BACK (`front_to_back`: the body and its transform per window, the
/// topmost first) and marks in `culled` (one flag per window slot, cleared here) every window
/// whose extent the windows above it cover on the `display`. `occluders` is the scratch of
/// opaque rectangles. Returns how many were left out.
pub(super) fn cull<'a>(
    front_to_back: impl Iterator<Item = (usize, &'a GlassCompositeParams, (i32, i32, u32, u32))>,
    display: (u32, u32),
    slots: usize,
    culled: &mut Vec<bool>,
    occluders: &mut Vec<Px>,
) -> usize {
    culled.clear();
    culled.resize(slots, false);
    occluders.clear();
    let mut n = 0;
    for (slot, p, transform) in front_to_back {
        let Some(fp) = Footprint::of(p, transform, display) else { continue };
        if occlude::covered(fp.extent, occluders) {
            if let Some(c) = culled.get_mut(slot) {
                *c = true;
                n += 1;
            }
            continue;
        }
        occluders.extend(fp.occluder.into_iter().flatten());
    }
    n
}
