// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Reusable scroll physics — the **production scroll mechanism** any scrollable
//! surface uses (virtual lists, grids, text views, image canvases), not baked
//! into one widget.
//!
//! Model = Android's `Scroller`/`smoothScrollBy` for discrete (mouse-wheel)
//! input: each notch adds a **fixed, predictable** amount to a scroll TARGET, and
//! the position eases toward it with a **decelerate** curve (fast start, smooth
//! ease-out — exponential approach). This is what makes it feel *in control* and
//! natural:
//!   - **Predictable** — N notches always scroll N×; no acceleration "guessing"
//!     that makes the same notch jump a random distance (the old "random" feel).
//!   - **Lands where you scrolled** — eases to the target, never overshoots past
//!     where you stopped (no synthetic momentum coast).
//!   - **Responsive** — DECELERATE (not a spring-from-rest, which eases *in* and
//!     feels laggy): the position moves most on the first frames, so a notch
//!     responds immediately, then glides to rest.
//!   - **Reverses cleanly** — the ease always heads to the *current* target, so a
//!     direction change is instant (no velocity to coast the wrong way).
//!   - **Frame-rate independent** — the per-frame fraction is scaled by real `dt`.
//!
//! (Velocity/friction *fling* is Android's TOUCH model — velocity = the finger
//! release speed the user controls. A wheel has no gesture velocity, so
//! synthesising one is exactly what felt "random"; we don't.)

/// Feel knob — how fast the position approaches the target (decelerate rate).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollConfig {
    /// Approach rate per second: the position closes ~`rate·dt` of the remaining
    /// gap each frame. Higher = snappier. ~16 ⇒ a stop settles in ~0.3 s with a
    /// strong immediate response on the first frame.
    pub rate_per_s: f32,
}

impl Default for ScrollConfig {
    fn default() -> Self {
        Self { rate_per_s: 16.0 }
    }
}

/// 1-D smooth scroller over a content extent. Each wheel notch extends the target
/// by a fixed amount; `tick` eases the position toward it (decelerate).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollMomentum {
    pos: f32,
    target: f32,
    min: f32,
    max: f32,
    config: ScrollConfig,
}

/// Below this gap (px) the ease snaps to the target and stops — sub-pixel, so
/// pixel-identical, but it lets the present pacer go idle promptly (no long tail).
const SETTLE_EPS: f32 = 0.4;
/// Max fraction of the remaining gap to close in ONE tick. Caps the move so a
/// large `dt` (low frame rate / a stall) can't visibly teleport — but it's high
/// enough that even at a low present rate the position still reaches where you
/// scrolled in 2–3 frames (so scroll responds to the FULL amount, not a few px).
/// This replaces a `dt` clamp, which throttled the ease at low FPS and made it
/// crawl regardless of how far you scrolled.
const MAX_STEP_FRAC: f32 = 0.85;

impl ScrollMomentum {
    pub fn new(config: ScrollConfig) -> Self {
        Self { pos: 0.0, target: 0.0, min: 0.0, max: 0.0, config }
    }

    pub fn with_defaults() -> Self {
        Self::new(ScrollConfig::default())
    }

    /// Set the scrollable extent: `max_scroll = (content − viewport).max(0)`,
    /// re-clamping position + target (e.g. content shrank).
    pub fn set_extent(&mut self, viewport: f32, content: f32) {
        self.max = (content - viewport).max(0.0);
        self.pos = self.pos.clamp(self.min, self.max);
        self.target = self.target.clamp(self.min, self.max);
    }

    /// Current scroll offset (px from the top), always within `[0, max]`.
    pub fn offset(&self) -> f32 {
        self.pos
    }

    /// Offset rounded to whole pixels (for integer-pixel render/composite paths).
    pub fn offset_px(&self) -> i32 {
        // `pos` is clamped to `[0, max]` (≥ 0), so `x + 0.5` truncated is
        // round-to-nearest without `libm`.
        (self.pos + 0.5) as i32
    }

    /// The scroll target the position is easing toward.
    pub fn target(&self) -> f32 {
        self.target
    }

    pub fn max_scroll(&self) -> f32 {
        self.max
    }

    /// True while the position is still easing toward the target.
    pub fn is_animating(&self) -> bool {
        self.pos != self.target
    }

    /// Wheel/scroll input: extend the TARGET by a FIXED `notch_px` (positive =
    /// down), clamped to bounds. No acceleration — N notches scroll N× — so it is
    /// predictable and stays under the user's control. The position eases to the
    /// target (decelerate). A reversing notch needs no special handling: the ease
    /// simply heads to the new (lower/higher) target from the next frame.
    pub fn scroll_wheel(&mut self, notch_px: f32) -> i32 {
        if notch_px.is_finite() && notch_px != 0.0 {
            self.target = (self.target + notch_px).clamp(self.min, self.max);
        }
        self.offset_px()
    }

    /// Alias for non-wheel callers (no gesture-velocity source) — extends the
    /// target like a notch.
    pub fn fling(&mut self, delta_px: f32) {
        let _ = self.scroll_wheel(delta_px);
    }

    /// Jump immediately by `delta` px, motion stopped (programmatic scroll-to).
    pub fn scroll_by(&mut self, delta: f32) {
        let v = (self.pos + delta).clamp(self.min, self.max);
        self.pos = v;
        self.target = v;
    }

    /// Set the absolute position immediately, motion stopped.
    pub fn set_offset(&mut self, offset: f32) {
        let v = offset.clamp(self.min, self.max);
        self.pos = v;
        self.target = v;
    }

    /// Advance the ease by real elapsed `dt_ns` (frame-rate independent): close a
    /// `rate·dt` fraction of the remaining gap (decelerate), clamp, and snap when
    /// sub-pixel. Returns true while still moving.
    pub fn tick(&mut self, dt_ns: u64) -> bool {
        // Hardening: a non-finite state must never stick "animating" (else the
        // present pacer never idles → hang). Snap to a clean stop.
        if !self.pos.is_finite() || !self.target.is_finite() {
            self.pos = self.min;
            self.target = self.min;
            return false;
        }
        if self.pos == self.target {
            return false;
        }
        // Frame-rate independent: close `rate·dt` of the gap — at 120 Hz a small,
        // smooth step; at a low present rate a larger step (capped) so the content
        // still reaches the target promptly instead of crawling a few px/frame.
        let dt = (dt_ns as f64 * 1e-9) as f32;
        let frac = (self.config.rate_per_s * dt).clamp(0.0, MAX_STEP_FRAC);
        self.pos += (self.target - self.pos) * frac;
        self.pos = self.pos.clamp(self.min, self.max);
        if (self.target - self.pos).abs() < SETTLE_EPS {
            self.pos = self.target; // sub-pixel: snap + end
        }
        self.pos != self.target
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME_120: u64 = 8_333_333;

    fn settle(s: &mut ScrollMomentum) -> u32 {
        let mut frames = 0;
        while s.tick(FRAME_120) && frames < 5000 {
            frames += 1;
        }
        frames
    }

    #[test]
    fn notch_is_fixed_and_predictable() {
        // The defining "in control" property: N notches scroll EXACTLY N× one
        // notch — no acceleration, no randomness. Lands at the target, no overshoot.
        let step = 60.0;
        let one = {
            let mut s = ScrollMomentum::with_defaults();
            s.set_extent(600.0, 1_000_000.0);
            s.scroll_wheel(step);
            settle(&mut s);
            s.offset()
        };
        let five = {
            let mut s = ScrollMomentum::with_defaults();
            s.set_extent(600.0, 1_000_000.0);
            for _ in 0..5 {
                s.scroll_wheel(step);
            }
            settle(&mut s);
            s.offset()
        };
        std::eprintln!("1 notch = {one}px, 5 notches = {five}px (ratio {:.2})", five / one);
        assert!((one - step).abs() < 0.5, "one notch lands exactly at its step: {one}");
        assert!((five - step * 5.0).abs() < 0.5, "five notches = 5× (linear, predictable): {five}");
    }

    #[test]
    fn decelerates_responsive_no_overshoot() {
        // Decelerate: biggest move on the FIRST frame (responsive, not a laggy
        // ease-in), monotonic, never past the target, settles promptly.
        let mut s = ScrollMomentum::with_defaults();
        s.set_extent(600.0, 1_000_000.0);
        let target = 300.0;
        s.scroll_wheel(target);
        let first = {
            s.tick(FRAME_120);
            s.offset()
        };
        std::eprintln!("first-frame move = {first}px of {target}");
        assert!(first > 20.0, "responsive: big first-frame move, not a slow ease-in ({first})");
        let (mut prev, mut frames) = (first, 1u32);
        while s.tick(FRAME_120) && frames < 5000 {
            let p = s.offset();
            assert!(p >= prev - 0.01, "monotonic toward target: {p} < {prev}");
            assert!(p <= target + 0.01, "no overshoot: {p} > {target}");
            // Decelerating: each step no larger than the previous (exponential).
            prev = p;
            frames += 1;
        }
        std::eprintln!("settled in {frames} frames ({}ms)", frames * 8);
        assert!(frames >= 4 && frames <= 90, "smooth + prompt: {frames}f");
        assert!((s.offset() - target).abs() < 0.5, "settles AT target");
    }

    #[test]
    fn reversal_is_instant() {
        // Scroll down (settle), then a notch up → heads up immediately, no coast.
        let mut s = ScrollMomentum::with_defaults();
        s.set_extent(600.0, 1_000_000.0);
        for _ in 0..6 {
            s.scroll_wheel(60.0);
        }
        settle(&mut s);
        let before = s.offset();
        assert!((before - 360.0).abs() < 0.5);
        s.scroll_wheel(-60.0);
        assert!(s.target() < before, "target moved up");
        s.tick(FRAME_120);
        assert!(s.offset() < before, "reversal heads up on the very next frame");
        settle(&mut s);
        assert!((s.offset() - 300.0).abs() < 0.5, "lands at the reversed target (300)");
    }

    #[test]
    fn frame_rate_independent() {
        fn travel(dt: u64) -> f32 {
            let mut s = ScrollMomentum::with_defaults();
            s.set_extent(600.0, 1_000_000.0);
            s.scroll_wheel(300.0);
            let mut f = 0;
            while s.tick(dt) && f < 5000 {
                f += 1;
            }
            s.offset()
        }
        let a = travel(8_333_333); // 120 Hz
        let b = travel(16_666_667); // 60 Hz
        std::eprintln!("frame-rate: 120Hz={a}px 60Hz={b}px");
        assert!((a - b).abs() < 0.5, "same destination regardless of frame rate: {a} vs {b}");
    }

    #[test]
    fn reaches_full_target_even_at_low_frame_rate() {
        // Regression: at a low present rate (render-bound windowd ~3 FPS) the ease
        // must still travel the FULL scrolled distance, not crawl a few px/frame.
        let mut s = ScrollMomentum::with_defaults();
        s.set_extent(600.0, 1_000_000.0);
        s.scroll_wheel(900.0); // scrolled far
        let dt_3fps = 333_000_000u64;
        let mut f = 0;
        while s.tick(dt_3fps) && f < 50 {
            f += 1;
        }
        std::eprintln!("3 FPS: reached {}px of 900 in {f} frames", s.offset());
        assert!(
            (s.offset() - 900.0).abs() < 0.5,
            "reaches the full target at 3 FPS: {}",
            s.offset()
        );
        assert!(f <= 6, "and gets there in a few frames, not crawling: {f}");
    }

    #[test]
    fn clamps_at_edges() {
        let mut s = ScrollMomentum::with_defaults();
        s.set_extent(600.0, 1_000.0); // max_scroll = 400
        for _ in 0..20 {
            s.scroll_wheel(200.0);
        }
        settle(&mut s);
        assert_eq!(s.offset_px(), 400, "clamps at bottom");
        s.set_extent(600.0, 700.0); // max_scroll = 100 (content shrank)
        assert_eq!(s.offset_px(), 100, "re-clamps stranded offset");
    }

    #[test]
    fn hardening_non_finite_never_hangs() {
        let mut s = ScrollMomentum::with_defaults();
        s.set_extent(600.0, 100_000.0);
        s.scroll_wheel(f32::NAN);
        s.scroll_wheel(f32::INFINITY);
        let mut f = 0;
        while s.tick(FRAME_120) && f < 5000 {
            f += 1;
        }
        assert!(!s.is_animating(), "must settle, not hang ({f}f)");
        assert!(s.offset().is_finite() && (0.0..=s.max_scroll()).contains(&s.offset()));
        s.scroll_wheel(60.0);
        settle(&mut s);
        assert!((s.offset() - 60.0).abs() < 0.5, "works after poison");
    }
}
