// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: a transform as a present carries it (`Command::LayerTransform`, RFC-0101 §8): an
//! offset, an opacity multiplier and a scale in 1/10000 about ONE origin — every layer of a group
//! (a window's body, its regions, its slices; a motion layer's pixels and what it claims) scales
//! about the same point, so the group grows as one surface (TASK-0354 P3; each layer about its
//! own centre, the pieces drifted apart). The pure part, host-tested: the identity, a window's
//! transform from its animation state, a motion layer's inside the window that carries it.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `mod tests` below

/// A scale of one, in the 1/10000 steps a transform states its scale in.
pub(crate) const SCALE_ONE: u32 = 10_000;

/// A transform for the present: moved by (`dx`, `dy`) after scaling by `scale` (1/10000) about
/// `origin` (display pixels), its layers' opacity multiplied by `opacity` (`0..=255`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FrameTransform {
    pub(crate) dx: i32,
    pub(crate) dy: i32,
    pub(crate) opacity: u32,
    pub(crate) scale: u32,
    pub(crate) origin: (i32, i32),
}

impl FrameTransform {
    /// At rest: nothing moved, faded or scaled.
    pub(crate) const IDENTITY: Self =
        Self { dx: 0, dy: 0, opacity: 255, scale: SCALE_ONE, origin: (0, 0) };

    /// Whether it changes nothing it draws (the origin of a unit scale is no change).
    pub(crate) fn is_identity(&self) -> bool {
        self.dx == 0 && self.dy == 0 && self.opacity == 255 && self.scale == SCALE_ONE
    }

    /// A window's transform from its animation state: offset (px), opacity (`0.0..=1.0`) and
    /// scale (`1.0` = identity) about its frame's centre (`frame`: `x, y, w, h` at rest).
    pub(crate) fn of_window(
        (dx, dy): (i32, i32),
        opacity: f32,
        scale: f32,
        (x, y, w, h): (i32, i32, u32, u32),
    ) -> Self {
        Self {
            dx,
            dy,
            opacity: (opacity * 255.0).clamp(0.0, 255.0) as u32,
            scale: (scale * SCALE_ONE as f32).clamp(100.0, 40_000.0) as u32,
            origin: (x + (w / 2) as i32, y + (h / 2) as i32),
        }
    }

    /// A motion layer's transform inside the transform of the surface carrying it (`self`: the
    /// window's, or the identity for the desktop): the offsets add, the opacities and scales
    /// multiply, and the scale is about the layer's own centre (`centre`, display pixels) — exact
    /// for the desktop, whose transform is the identity.
    pub(crate) fn with_layer(
        &self,
        (dx, dy): (i32, i32),
        opacity: u32,
        scale: u32,
        centre: (i32, i32),
    ) -> Self {
        Self {
            dx: self.dx + dx,
            dy: self.dy + dy,
            opacity: self.opacity * opacity.min(255) / 255,
            scale: (u64::from(self.scale) * u64::from(scale) / u64::from(SCALE_ONE))
                .clamp(100, 40_000) as u32,
            origin: centre,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A window opening at 0.94 of its size grows about its frame's centre; at rest it is the
    /// identity whatever its origin.
    #[test]
    fn a_window_scales_about_its_frame_centre() {
        let t = FrameTransform::of_window((0, 12), 0.5, 0.94, (100, 200, 400, 300));
        assert_eq!(t.origin, (300, 350));
        assert_eq!((t.scale, t.opacity, t.dy), (9_400, 127, 12));
        assert!(!t.is_identity());
        let rest = FrameTransform::of_window((0, 0), 1.0, 1.0, (100, 200, 400, 300));
        assert!(rest.is_identity(), "a unit scale about any origin changes nothing");
    }

    /// A motion layer on the desktop: its own offset, opacity and scale about its centre.
    #[test]
    fn a_motion_layer_carries_its_own_scale_about_its_centre() {
        let t = FrameTransform::IDENTITY.with_layer((0, -10), 128, 9_600, (960, 300));
        assert_eq!(
            t,
            FrameTransform { dx: 0, dy: -10, opacity: 128, scale: 9_600, origin: (960, 300) }
        );
        let inside = FrameTransform::of_window((5, 5), 1.0, 0.5, (0, 0, 100, 100));
        let t = inside.with_layer((1, 2), 255, 9_000, (50, 50));
        assert_eq!((t.dx, t.dy, t.scale), (6, 7, 4_500), "offsets add, scales multiply");
    }

    #[test]
    fn test_reject_a_scale_outside_the_wire_s_range() {
        assert_eq!(FrameTransform::of_window((0, 0), 1.0, 0.0, (0, 0, 10, 10)).scale, 100);
        assert_eq!(FrameTransform::of_window((0, 0), 1.0, 9.0, (0, 0, 10, 10)).scale, 40_000);
        let t = FrameTransform::of_window((0, 0), 1.0, 0.01, (0, 0, 10, 10));
        assert_eq!(t.with_layer((0, 0), 255, 100, (0, 0)).scale, 100, "a product below 1 % clamps");
    }
}
