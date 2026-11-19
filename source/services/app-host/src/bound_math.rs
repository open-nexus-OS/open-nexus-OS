// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the pure decision behind `.motion(duration, curve)` (TASK-0354 P10,
//! docs/dev/dsl/modifiers.md §`.motion`): a node is laid out where it belongs; when its rest —
//! place, size, rotation, opacity — changes, the host paints it with a deviation that makes it
//! look like it still sits where it was, and the deviation decays to identity on the declared
//! pair (FLIP). A change that arrives mid-flight starts from where the node is seen, so it never
//! jumps. `probe/anim/bound.rs` keeps the rest table and drives the tracks; the arithmetic lives
//! here so host tests reach it (`probe/` is RISC-V-only).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `bound_math/tests.rs`

/// A node's rest as laid out: its rectangle, rotation (degrees clockwise) and effective opacity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Rest {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) w: i32,
    pub(crate) h: i32,
    pub(crate) rot: i16,
    pub(crate) opacity: u8,
}

/// What the node is painted with on top of its rest: a translation of its centre, a scale per
/// axis, a rotation, and an opacity factor over a base (`base`: the effective opacity the box is
/// painted at instead of its own — set while an opacity travels DOWN, so it can fade from where
/// it was). Identity = nothing to paint differently.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Deviation {
    pub(crate) dx: f32,
    pub(crate) dy: f32,
    pub(crate) sx: f32,
    pub(crate) sy: f32,
    pub(crate) rot: f32,
    pub(crate) opacity: f32,
    pub(crate) base: Option<u8>,
}

impl Deviation {
    /// Nothing differs from the rest.
    pub(crate) const IDENTITY: Deviation =
        Deviation { dx: 0.0, dy: 0.0, sx: 1.0, sy: 1.0, rot: 0.0, opacity: 1.0, base: None };

    /// The opacity the node is SEEN at over a rest effective opacity `rest`.
    pub(crate) fn seen_opacity(&self, rest: u8) -> f32 {
        f32::from(self.base.unwrap_or(rest)) * self.opacity
    }
}

/// Below this a geometric difference is no change (sub-pixel: a re-layout's rounding).
const EPS_PX: f32 = 0.5;

/// The deviation to start when a node's rest moves from `old` to `new` while it is still painted
/// with `carry` over `old` (identity when it had arrived). `None` = nothing changed that the eye
/// would see. The opacity track (the `opacity`/`base` pair) ends at identity: up from where it
/// is seen to the new rest, or — the new rest lower — from 1 over a base of where it is seen down
/// to `new / base`, after which the caller clears both.
pub(crate) fn deviation(old: Rest, new: Rest, carry: Deviation) -> Option<Deviation> {
    let centre = |r: Rest| (r.x as f32 + r.w as f32 / 2.0, r.y as f32 + r.h as f32 / 2.0);
    let ((ox, oy), (nx, ny)) = (centre(old), centre(new));
    let ratio = |seen: f32, now: i32| if now > 0 { seen / now as f32 } else { 1.0 };
    let dev = Deviation {
        dx: ox + carry.dx - nx,
        dy: oy + carry.dy - ny,
        sx: ratio(old.w as f32 * carry.sx, new.w),
        sy: ratio(old.h as f32 * carry.sy, new.h),
        rot: f32::from(old.rot) + carry.rot - f32::from(new.rot),
        opacity: 1.0,
        base: None,
    };
    let seen = carry.seen_opacity(old.opacity);
    let dev = match seen - f32::from(new.opacity) {
        d if d.abs() < 1.0 => dev,
        // Up: over the new rest, from the share it is seen at.
        d if d < 0.0 => Deviation { opacity: seen / f32::from(new.opacity), ..dev },
        // Down: over a base of where it is seen, from all of it.
        _ => Deviation { base: Some(seen.clamp(0.0, 255.0) as u8), ..dev },
    };
    let moved = dev.dx.abs() >= EPS_PX
        || dev.dy.abs() >= EPS_PX
        || (dev.sx - 1.0).abs() * new.w.max(1) as f32 >= EPS_PX
        || (dev.sy - 1.0).abs() * new.h.max(1) as f32 >= EPS_PX
        || dev.rot.abs() >= 0.5
        || dev.opacity < 1.0
        || dev.base.is_some();
    moved.then_some(dev)
}

/// The opacity factor a down-travel ends at: the new rest over the base it started from.
pub(crate) fn opacity_target(dev: &Deviation, rest: u8) -> f32 {
    match dev.base {
        Some(base) if base > 0 => (f32::from(rest) / f32::from(base)).clamp(0.0, 1.0),
        _ => 1.0,
    }
}

#[cfg(test)]
mod tests;
