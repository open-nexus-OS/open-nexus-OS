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
//! TWO input models, ONE shared scroller — exactly like Android's `OverScroller`
//! (which has both `startScroll` and `fling`):
//!   - **Wheel / discrete** → [`ScrollMomentum::scroll_wheel`]: each notch extends
//!     a fixed TARGET, the position eases toward it (decelerate, no overshoot).
//!     Predictable (N notches = N×), the wheel's "in control" feel.
//!   - **Touch / trackpad inertia** → [`ScrollMomentum::fling`]: a release
//!     VELOCITY (px/s) that coasts and decelerates under viscous friction to a
//!     clean stop, clamped at the edges — Android `OverScroller.fling`.
//! Both feed the same `pos`, so the list/grid/search/text view all reuse one
//! mechanism. (A new wheel notch during a fling cancels the coast and takes over
//! the ease — direction changes stay instant.)

/// Feel knobs — wheel ease rate + touch fling friction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollConfig {
    /// Wheel-ease approach rate per second: the position closes ~`rate·dt` of the
    /// remaining gap each frame. Higher = snappier. ~16 ⇒ a stop settles in ~0.3 s
    /// with a strong immediate response on the first frame.
    pub rate_per_s: f32,
    /// Fling viscous friction (per second): a release velocity decays roughly like
    /// `e^(-friction·t)`, so coast distance ≈ `velocity / friction`. Higher = a
    /// shorter, snappier coast. ~4 ⇒ a brisk flick glides ~0.6–0.8 s — the Android
    /// `OverScroller` touch feel.
    pub fling_friction_per_s: f32,
}

impl Default for ScrollConfig {
    fn default() -> Self {
        Self { rate_per_s: 16.0, fling_friction_per_s: 4.0 }
    }
}

/// 1-D scroller over a content extent. Wheel notches extend a target the position
/// eases toward; a fling injects a velocity that coasts under friction. `tick`
/// advances whichever is active.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollMomentum {
    pos: f32,
    target: f32,
    /// Live fling velocity (px/s). Non-zero ⇒ coasting; the ease branch is inert
    /// while a fling is active (target tracks `pos`).
    velocity: f32,
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

/// Below this speed (px/s) a fling snaps to a clean stop (no sub-pixel crawl tail).
const FLING_V_MIN: f32 = 8.0;
/// Fixed fling integration sub-step (seconds). The fling integrator sub-steps the
/// frame's `dt` at this granularity so the coast is frame-rate independent (same
/// `h` whether `tick` is called at 120 Hz or 60 Hz) without needing `exp`/`powf`.
const FLING_SUBSTEP_S: f32 = 0.002;
/// Cap on sub-steps per `tick` so a huge `dt` (a stall / very low FPS) can't spin
/// a long loop; `h` just grows a little past `FLING_SUBSTEP_S` in that rare case.
const FLING_MAX_SUBSTEPS: i32 = 64;

impl ScrollMomentum {
    pub fn new(config: ScrollConfig) -> Self {
        Self { pos: 0.0, target: 0.0, velocity: 0.0, min: 0.0, max: 0.0, config }
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
        // A coast can't continue once clamped to an edge.
        if self.pos <= self.min || self.pos >= self.max {
            self.velocity = 0.0;
        }
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

    /// True while the position is still easing toward the target OR coasting from
    /// a fling — i.e. the present pacer must keep ticking until motion settles.
    pub fn is_animating(&self) -> bool {
        self.pos != self.target || self.velocity != 0.0
    }

    /// Wheel/scroll input: extend the TARGET by a FIXED `notch_px` (positive =
    /// down), clamped to bounds. No acceleration — N notches scroll N× — so it is
    /// predictable and stays under the user's control. The position eases to the
    /// target (decelerate). A reversing notch needs no special handling: the ease
    /// simply heads to the new (lower/higher) target from the next frame.
    pub fn scroll_wheel(&mut self, notch_px: f32) -> i32 {
        if notch_px.is_finite() && notch_px != 0.0 {
            // A wheel notch cancels any in-flight coast and takes over the ease,
            // so a deliberate notch (incl. a reversal) responds instantly.
            self.velocity = 0.0;
            self.target = (self.target + notch_px).clamp(self.min, self.max);
        }
        self.offset_px()
    }

    /// Android `OverScroller.fling`: inject a release **velocity** (px/s, positive
    /// = down). The position coasts and decelerates under viscous friction to a
    /// clean stop, clamped at the edges. Successive flings before the coast settles
    /// **accumulate** velocity (a fast repeated flick travels farther). For
    /// touch/trackpad inertia; wheel input uses [`Self::scroll_wheel`].
    pub fn fling(&mut self, velocity_px_s: f32) {
        if !velocity_px_s.is_finite() || velocity_px_s == 0.0 {
            return;
        }
        // Fling is authoritative: cancel a pending ease so the coast owns `pos`.
        self.target = self.pos;
        self.velocity += velocity_px_s;
    }

    /// Jump immediately by `delta` px, motion stopped (programmatic scroll-to).
    pub fn scroll_by(&mut self, delta: f32) {
        let v = (self.pos + delta).clamp(self.min, self.max);
        self.pos = v;
        self.target = v;
        self.velocity = 0.0;
    }

    /// Set the absolute position immediately, motion stopped.
    pub fn set_offset(&mut self, offset: f32) {
        let v = offset.clamp(self.min, self.max);
        self.pos = v;
        self.target = v;
        self.velocity = 0.0;
    }

    /// Advance the ease by real elapsed `dt_ns` (frame-rate independent): close a
    /// `rate·dt` fraction of the remaining gap (decelerate), clamp, and snap when
    /// sub-pixel. Returns true while still moving.
    pub fn tick(&mut self, dt_ns: u64) -> bool {
        // Hardening: a non-finite state must never stick "animating" (else the
        // present pacer never idles → hang). Snap to a clean stop.
        if !self.pos.is_finite() || !self.target.is_finite() || !self.velocity.is_finite() {
            self.pos = self.min;
            self.target = self.min;
            self.velocity = 0.0;
            return false;
        }
        let dt = (dt_ns as f64 * 1e-9) as f32;
        // FLING: coast + viscous friction, integrated at a fixed sub-step so the
        // distance is frame-rate independent (same `h` at 120 Hz or 60 Hz). Edge
        // hit or sub-threshold speed → clean stop. Takes priority over the ease.
        if self.velocity != 0.0 {
            // Round the sub-step count up without `f32::ceil` (libm, not in
            // no_std): truncating then +1 always over-covers `dt` (an exact
            // multiple just gets one extra, finer sub-step — harmless).
            let steps = ((dt / FLING_SUBSTEP_S) as i32 + 1).clamp(1, FLING_MAX_SUBSTEPS);
            let h = dt / steps as f32;
            for _ in 0..steps {
                self.pos += self.velocity * h;
                self.velocity *= 1.0 - (self.config.fling_friction_per_s * h).min(1.0);
                if self.pos <= self.min {
                    self.pos = self.min;
                    self.velocity = 0.0;
                    break;
                }
                if self.pos >= self.max {
                    self.pos = self.max;
                    self.velocity = 0.0;
                    break;
                }
                if self.velocity.abs() < FLING_V_MIN {
                    self.velocity = 0.0;
                    break;
                }
            }
            self.pos = self.pos.clamp(self.min, self.max);
            self.target = self.pos; // keep the ease branch inert; target = landing
            return self.velocity != 0.0;
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
    fn fling_coasts_decelerates_and_settles() {
        // A release velocity coasts forward, decelerates (ease-out: biggest step
        // first), stays monotonic + in bounds, and stops cleanly (no crawl tail).
        let mut s = ScrollMomentum::with_defaults();
        s.set_extent(600.0, 1_000_000.0);
        s.fling(3000.0); // brisk downward flick (px/s)
        let (mut frames, mut prev, mut peak, mut peak_frame) = (0u32, 0.0f32, 0.0f32, 0u32);
        while s.tick(FRAME_120) && frames < 5000 {
            let p = s.offset();
            let step = p - prev;
            assert!(step >= -0.01, "monotonic down during a down-fling: {step}");
            if step > peak {
                peak = step;
                peak_frame = frames;
            }
            prev = p;
            frames += 1;
        }
        std::eprintln!("fling(3000): {frames}f, {}px, peak {peak}px @f{peak_frame}", s.offset());
        assert!(frames >= 8, "coasts over many frames (a glide), not instant: {frames}");
        assert!(frames <= 360, "settles in a sane time: {frames}f");
        assert!(peak_frame <= frames / 3, "decelerates (ease-out): peak early");
        assert!(!s.is_animating(), "stops cleanly");
        assert!(s.offset() > 0.0, "actually moved");
    }

    #[test]
    fn faster_fling_travels_farther() {
        fn coast(v: f32) -> f32 {
            let mut s = ScrollMomentum::with_defaults();
            s.set_extent(600.0, 10_000_000.0);
            s.fling(v);
            let mut f = 0;
            while s.tick(FRAME_120) && f < 5000 {
                f += 1;
            }
            s.offset()
        }
        let slow = coast(1000.0);
        let fast = coast(4000.0);
        std::eprintln!("fling coast — 1000px/s={slow}px, 4000px/s={fast}px");
        assert!(fast > slow * 3.0, "4× velocity must coast much farther: {fast} vs {slow}");
    }

    #[test]
    fn fling_accumulates_before_settling() {
        // Successive flicks before the coast settles add velocity (the wheel-spin /
        // repeated-swipe case) → farther than a single flick.
        let single = {
            let mut s = ScrollMomentum::with_defaults();
            s.set_extent(600.0, 10_000_000.0);
            s.fling(1500.0);
            while s.tick(FRAME_120) {}
            s.offset()
        };
        let triple = {
            let mut s = ScrollMomentum::with_defaults();
            s.set_extent(600.0, 10_000_000.0);
            s.fling(1500.0);
            s.fling(1500.0);
            s.fling(1500.0);
            while s.tick(FRAME_120) {}
            s.offset()
        };
        std::eprintln!("accumulate — 1×={single}px, 3×={triple}px");
        assert!(triple > single * 2.0, "accumulated flings coast farther: {triple} vs {single}");
    }

    #[test]
    fn fling_is_frame_rate_independent() {
        fn coast(dt: u64) -> f32 {
            let mut s = ScrollMomentum::with_defaults();
            s.set_extent(600.0, 10_000_000.0);
            s.fling(3000.0);
            let mut f = 0;
            while s.tick(dt) && f < 10_000 {
                f += 1;
            }
            s.offset()
        }
        let a = coast(8_333_333); // 120 Hz
        let b = coast(16_666_667); // 60 Hz
        std::eprintln!("fling frame-rate: 120Hz={a}px 60Hz={b}px");
        assert!((a - b).abs() <= a / 20.0 + 2.0, "same coast regardless of frame rate: {a} vs {b}");
    }

    #[test]
    fn fling_clamps_and_stops_at_edge() {
        let mut s = ScrollMomentum::with_defaults();
        s.set_extent(600.0, 1_000.0); // max_scroll = 400
        s.fling(50_000.0); // way more than enough to reach the bottom
        let mut f = 0;
        while s.tick(FRAME_120) && f < 5000 {
            f += 1;
        }
        assert_eq!(s.offset_px(), 400, "stops clamped at the bottom edge");
        assert!(!s.is_animating(), "no residual coast past the edge");
    }

    #[test]
    fn wheel_notch_cancels_an_active_fling() {
        // A deliberate wheel notch during a coast takes over (ease to the new
        // target) — the coast does not keep dragging the position.
        let mut s = ScrollMomentum::with_defaults();
        s.set_extent(600.0, 1_000_000.0);
        s.fling(4000.0);
        s.tick(FRAME_120); // begin coasting
        let p = s.offset();
        s.scroll_wheel(-30.0); // reverse notch
        assert!(s.target() < p + 1.0, "target heads to the notch, not the coast landing");
        s.tick(FRAME_120);
        // No residual downward velocity dragging us further down past the notch ease.
        let settle_frames = {
            let mut f = 0;
            while s.tick(FRAME_120) && f < 5000 {
                f += 1;
            }
            f
        };
        assert!(settle_frames < 200, "settles to the notch target promptly: {settle_frames}f");
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
