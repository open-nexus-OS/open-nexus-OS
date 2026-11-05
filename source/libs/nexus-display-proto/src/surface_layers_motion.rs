// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0109 surface layers — the MOTION frame (`OP_SURFACE_MOTION`: the scroll layers'
//! offsets and the wash layers, in effect at once, the newest superseding) and the check that it
//! names only what the structure declares. Split out of `surface_layers.rs` (module-size
//! ratchet); re-exported there, so the wire reads as one module.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable (append-only)
//! TEST_COVERAGE: `surface_layers_tests.rs` (round trip, `test_reject_*` for every bound)

use crate::client_surface::{has_op, header, HEADER_LEN};
use crate::motion_layer::MOTION_PARENT;
use crate::surface_layers::{
    get16, get32, put16, put32, SurfaceTree, MAX_SCROLL_LAYERS, MAX_WASHES, OP_SURFACE_MOTION,
};

/// One wash layer: patch `patch` spread over the rectangle (`x`, `y`, `w`, `h`) in its parent's
/// content coordinates (the base: surface coordinates; scroll layer `k`: relative to its
/// viewport's origin at offset 0; motion layer `m`: relative to its origin at rest), with a corner
/// radius and an opacity, over the parent's content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Wash {
    /// `0` = the base, `k` = scroll layer `k` (1-based), `MOTION_PARENT | m` = motion layer `m`.
    pub parent: u8,
    pub patch: u8,
    pub radius: u8,
    pub opacity: u8,
    pub x: i32,
    pub y: i32,
    pub w: u16,
    pub h: u16,
}

impl Wash {
    /// The motion layer the wash moves with, when its parent is one.
    #[must_use]
    pub fn motion_parent(&self) -> Option<usize> {
        (self.parent & MOTION_PARENT != 0).then_some(usize::from(self.parent & !MOTION_PARENT))
    }
}

/// A surface's motion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SurfaceMotion {
    /// The scroll layers' offsets along their axes, in the structure's order.
    pub offsets: [i32; MAX_SCROLL_LAYERS],
    pub offset_count: usize,
    pub washes: [Wash; MAX_WASHES],
    pub wash_count: usize,
}

impl SurfaceMotion {
    pub fn offsets(&self) -> &[i32] {
        &self.offsets[..self.offset_count.min(MAX_SCROLL_LAYERS)]
    }

    pub fn washes(&self) -> &[Wash] {
        &self.washes[..self.wash_count.min(MAX_WASHES)]
    }
}

const MOTION_HEAD: usize = HEADER_LEN + 8;
const WASH_BYTES: usize = 16;
/// The longest motion frame.
pub const SURFACE_MOTION_MAX_LEN: usize =
    MOTION_HEAD + MAX_SCROLL_LAYERS * 4 + MAX_WASHES * WASH_BYTES;

/// Encodes the motion (counts clamped to their bounds). Returns the frame length.
#[must_use]
pub fn encode_surface_motion(
    surface_id: u32,
    motion: &SurfaceMotion,
    out: &mut [u8; SURFACE_MOTION_MAX_LEN],
) -> usize {
    let (offsets, washes) = (motion.offsets(), motion.washes());
    out[..HEADER_LEN].copy_from_slice(&header(OP_SURFACE_MOTION));
    put32(out, 4, surface_id);
    out[8] = offsets.len() as u8;
    out[9] = washes.len() as u8;
    put16(out, 10, 0);
    let mut pos = MOTION_HEAD;
    for o in offsets {
        put32(out, pos, *o as u32);
        pos += 4;
    }
    for w in washes {
        out[pos] = w.parent;
        out[pos + 1] = w.patch;
        out[pos + 2] = w.radius;
        out[pos + 3] = w.opacity;
        put32(out, pos + 4, w.x as u32);
        put32(out, pos + 8, w.y as u32);
        put16(out, pos + 12, w.w);
        put16(out, pos + 14, w.h);
        pos += WASH_BYTES;
    }
    pos
}

/// Decodes a motion frame: length-exact, counts within their bounds, reserved bytes zero.
/// Returns the surface id. Whether it names existing layers and patches is
/// [`validate_motion`]'s.
#[must_use]
pub fn decode_surface_motion(frame: &[u8], out: &mut SurfaceMotion) -> Option<u32> {
    if !has_op(frame, OP_SURFACE_MOTION) || frame.len() < MOTION_HEAD {
        return None;
    }
    let (offsets, washes) = (usize::from(frame[8]), usize::from(frame[9]));
    if offsets > MAX_SCROLL_LAYERS
        || washes > MAX_WASHES
        || get16(frame, 10) != 0
        || frame.len() != MOTION_HEAD + offsets * 4 + washes * WASH_BYTES
    {
        return None;
    }
    let mut motion = SurfaceMotion::default();
    let mut pos = MOTION_HEAD;
    for slot in motion.offsets.iter_mut().take(offsets) {
        *slot = get32(frame, pos) as i32;
        pos += 4;
    }
    for slot in motion.washes.iter_mut().take(washes) {
        *slot = Wash {
            parent: frame[pos],
            patch: frame[pos + 1],
            radius: frame[pos + 2],
            opacity: frame[pos + 3],
            x: get32(frame, pos + 4) as i32,
            y: get32(frame, pos + 8) as i32,
            w: get16(frame, pos + 12),
            h: get16(frame, pos + 14),
        };
        pos += WASH_BYTES;
    }
    motion.offset_count = offsets;
    motion.wash_count = washes;
    *out = motion;
    Some(get32(frame, 4))
}

/// Why a motion was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionError {
    /// A wash names a patch the structure does not declare.
    MissingPatch,
    /// A wash names a parent the structure does not declare.
    MissingParent,
    /// A wash with no area.
    EmptyWash,
}

/// Whether a motion names only what `tree` declares. Offsets beyond the structure's scroll
/// layers are ignored by the receiver (a motion may race a structure change).
pub fn validate_motion(motion: &SurfaceMotion, tree: &SurfaceTree) -> Result<(), MotionError> {
    for w in motion.washes() {
        if usize::from(w.patch) >= tree.patches().len() {
            return Err(MotionError::MissingPatch);
        }
        let known = match w.motion_parent() {
            Some(m) => m < tree.motions().len(),
            None => usize::from(w.parent) <= tree.scrolls().len(),
        };
        if !known {
            return Err(MotionError::MissingParent);
        }
        if w.w == 0 || w.h == 0 {
            return Err(MotionError::EmptyWash);
        }
    }
    Ok(())
}
