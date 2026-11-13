// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the kit's sweep loops on a time grid (TASK-0354 P11) — where the part a widget
//! animates forever is at a given time: the indeterminate progress pip crosses its track in
//! 1.1 s on the smooth curve, a skeleton's shimmer band crosses its block in 1.4 s ease-in-out
//! (the design artifacts' timings), each starting again at the leading edge. A position is a
//! function of the clock alone: no spring to re-fire, nothing allocated, every loop of a kind in
//! phase, and the same time gives the same pixel on every run. Same one-cfg shape as
//! `motion_math`: `probe/anim.rs` writes the position into the part's transform on the frame
//! pulse.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `mod tests` below

use animation::keyframe::cubic_bezier;
use nexus_dsl_runtime::{LOOP_SHIMMER, LOOP_SWEEP};
use nexus_theme_tokens::MotionCurveToken;

/// One sweep loop: how long a crossing takes and the timing curve it follows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Sweep {
    /// One crossing (ns).
    pub(crate) period_ns: u64,
    /// The crossing's `cubic-bezier` control points.
    pub(crate) curve: [f32; 4],
}

/// The indeterminate progress pip: 1.1 s on the theme's smooth curve.
pub(crate) const PIP: Sweep =
    Sweep { period_ns: 1_100_000_000, curve: MotionCurveToken::Smooth.control_points() };
/// A skeleton's shimmer band: 1.4 s, ease-in-out (the style sheets' curve of that name).
pub(crate) const SHIMMER: Sweep = Sweep { period_ns: 1_400_000_000, curve: [0.42, 0.0, 0.58, 1.0] };

/// The sweep a loop kind runs (`nexus_dsl_runtime::LOOP_*`); `None`: the kind does not sweep.
pub(crate) fn sweep_of(kind: i32) -> Option<Sweep> {
    match kind {
        LOOP_SWEEP => Some(PIP),
        LOOP_SHIMMER => Some(SHIMMER),
        _ => None,
    }
}

impl Sweep {
    /// Where the part is at `now_ns` on a travel of `travel` px: `0` at the start of every
    /// crossing, `travel` at its end (never past either).
    pub(crate) fn at(&self, now_ns: u64, travel: i32) -> i32 {
        if travel <= 0 {
            return 0;
        }
        let t = (now_ns % self.period_ns) as f32 / self.period_ns as f32;
        let x = (travel as f32 * cubic_bezier(self.curve, t) + 0.5) as i32;
        x.clamp(0, travel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pip at named times of its 1.1 s crossing of a 200-px travel: the smooth curve's
    /// values (`cubic-bezier(0.4, 0, 0.2, 1)` — 0.0259 at a tenth, 0.2366 at a quarter, 0.7756 at
    /// half, 0.9594 at three quarters), and the same place one period later.
    #[test]
    fn the_pip_crosses_in_1100_ms_on_the_smooth_curve() {
        let ms = |ms: u64| PIP.at(ms * 1_000_000, 200);
        assert_eq!((ms(0), ms(110), ms(275), ms(550), ms(825)), (0, 5, 47, 155, 192));
        assert_eq!(ms(1_099), 200);
        assert_eq!(ms(1_100), 0, "it starts again at the leading edge");
        assert_eq!(ms(1_100 + 550), ms(550));
    }

    /// The shimmer at named times of its 1.4 s crossing: ease-in-out is symmetric about the
    /// middle.
    #[test]
    fn the_shimmer_crosses_in_1400_ms_ease_in_out() {
        let ms = |ms: u64| SHIMMER.at(ms * 1_000_000, 200);
        assert_eq!((ms(0), ms(700), ms(1_400)), (0, 100, 0));
        assert_eq!(ms(350) + ms(1_050), 200, "symmetric: a quarter and three quarters");
        assert!(ms(350) < 50, "slow at the start: {}", ms(350));
        assert_eq!(ms(1_400 + 350), ms(350));
    }

    /// A position never leaves the travel, and a track with no room holds the part still.
    #[test]
    fn a_position_stays_on_its_travel() {
        for sweep in [PIP, SHIMMER] {
            for step in 0..=2_000u64 {
                let x = sweep.at(step * 1_500_000, 137);
                assert!((0..=137).contains(&x), "{x} at step {step}");
            }
            assert_eq!(sweep.at(123_456_789, 0), 0);
            assert_eq!(sweep.at(123_456_789, -4), 0);
        }
    }

    /// Only the two sweep kinds sweep.
    #[test]
    fn the_loop_kinds_map_to_their_sweeps() {
        use nexus_dsl_runtime::{LOOP_BREATHE, LOOP_CAROUSEL};
        assert_eq!(sweep_of(LOOP_SWEEP), Some(PIP));
        assert_eq!(sweep_of(LOOP_SHIMMER), Some(SHIMMER));
        assert_eq!(sweep_of(LOOP_BREATHE), None);
        assert_eq!(sweep_of(LOOP_CAROUSEL), None);
    }
}
