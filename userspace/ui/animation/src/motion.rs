// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The curated **motion token** vocabulary — the SSOT the DSL
//! `.animate`/`.transition`/`.effect` token argument validates against and the
//! runtime binding resolves to concrete physics. One small, closed catalog
//! (no free-form CSS keyframes, no `--animate-*` vars) per
//! `docs/dev/ui/foundations/animation.md`.
//! OWNERS: @ui @runtime
//! STATUS: In progress (TASK-0062/0075 DSL animation binding)
//! API_STABILITY: Unstable
//!
//! Each token maps to a THEME token pair — a [`MotionDurationToken`] and a
//! [`MotionCurveToken`] (`nexus-theme-tokens`, the motion SSOT) — plus the
//! primary [`AnimProp`] it drives. The theme owns the concrete ms/curve so a
//! reduced-motion theme zeroes durations (drivers treat 0 as "jump to the
//! final frame"); reduced motion is therefore part of every token's contract.

use crate::keyframe::Easing;
use crate::property::AnimProp;
use nexus_theme_tokens::{MotionCurveToken, MotionDurationToken};

/// Which motion category a token expresses by nature (value/transition/effect).
/// The *modifier* (`.animate`/`.transition`/`.effect`) drives actual behavior;
/// this is the token's documented home category (animation.md "Motion
/// categories").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionCategory {
    /// State/value-driven property change (`.animate`).
    Value,
    /// Insert/remove/open/close lifecycle motion (`.transition`).
    Transition,
    /// Bounded attention effect (`.effect`).
    Effect,
}

/// The curated motion tokens (animation.md "Recommended v1 scope"). The `u8`
/// discriminant is the STABLE wire id the runtime stamps into an animation
/// intent and the host resolves back — APPEND-ONLY (never reorder).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MotionToken {
    /// Fast, lightly-overshooting value change (micro-feedback).
    Snappy = 0,
    /// Calm ease-in-out value change.
    Smooth = 1,
    /// Slower, spring-forward value change for hero moments.
    Emphasized = 2,
    /// Opacity cross-fade.
    Fade = 3,
    /// Enter/leave by sliding up into place.
    SlideUp = 4,
    /// Enter/leave by fading while scaling in from 0.92.
    FadeScale = 5,
    /// Bounded left/right attention wiggle.
    Wiggle = 6,
    /// Bounded scale pulse.
    Pulse = 7,
}

impl MotionToken {
    /// Every token, in id order (checker + docs iterate this).
    pub const ALL: [MotionToken; 8] = [
        MotionToken::Snappy,
        MotionToken::Smooth,
        MotionToken::Emphasized,
        MotionToken::Fade,
        MotionToken::SlideUp,
        MotionToken::FadeScale,
        MotionToken::Wiggle,
        MotionToken::Pulse,
    ];

    /// Canonical `.nx` token name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            MotionToken::Snappy => "snappy",
            MotionToken::Smooth => "smooth",
            MotionToken::Emphasized => "emphasized",
            MotionToken::Fade => "fade",
            MotionToken::SlideUp => "slideUp",
            MotionToken::FadeScale => "fadeScale",
            MotionToken::Wiggle => "wiggle",
            MotionToken::Pulse => "pulse",
        }
    }

    /// Resolve a token name (checker + runtime emit share this).
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.name() == name)
    }

    /// The stable wire id (intent stamp).
    #[must_use]
    pub const fn id(self) -> u8 {
        self as u8
    }

    /// Resolve a wire id back to a token (host side of the intent stamp).
    #[must_use]
    pub fn from_id(id: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.id() == id)
    }

    /// The token's documented home category.
    #[must_use]
    pub const fn category(self) -> MotionCategory {
        match self {
            MotionToken::Snappy | MotionToken::Smooth | MotionToken::Emphasized => {
                MotionCategory::Value
            }
            MotionToken::Fade | MotionToken::SlideUp | MotionToken::FadeScale => {
                MotionCategory::Transition
            }
            MotionToken::Wiggle | MotionToken::Pulse => MotionCategory::Effect,
        }
    }

    /// The theme duration step this token eases over (0 ms under a
    /// reduced-motion theme ⇒ the driver jumps to the final frame).
    #[must_use]
    pub const fn duration(self) -> MotionDurationToken {
        match self {
            MotionToken::Snappy => MotionDurationToken::Swift,
            MotionToken::Pulse | MotionToken::Fade | MotionToken::FadeScale => {
                MotionDurationToken::Quick
            }
            MotionToken::Smooth | MotionToken::SlideUp | MotionToken::Wiggle => {
                MotionDurationToken::Base
            }
            MotionToken::Emphasized => MotionDurationToken::Slow,
        }
    }

    /// The theme curve vocabulary this token belongs to (documentation +
    /// future GPU-side spring seeding; the CPU keyframe path uses
    /// [`Self::easing`]).
    #[must_use]
    pub const fn curve(self) -> MotionCurveToken {
        match self {
            MotionToken::Snappy => MotionCurveToken::SpringSoft,
            MotionToken::Emphasized | MotionToken::FadeScale => MotionCurveToken::Spring,
            MotionToken::SlideUp => MotionCurveToken::Glide,
            MotionToken::Smooth | MotionToken::Fade | MotionToken::Wiggle | MotionToken::Pulse => {
                MotionCurveToken::Smooth
            }
        }
    }

    /// The deterministic CPU easing for the keyframe path.
    #[must_use]
    pub const fn easing(self) -> Easing {
        match self {
            // Springs approximate to ease-out on the deterministic CPU track
            // (overshoot is the GPU spring path, Track C).
            MotionToken::Snappy
            | MotionToken::Emphasized
            | MotionToken::SlideUp
            | MotionToken::FadeScale => Easing::EaseOut,
            MotionToken::Smooth | MotionToken::Fade => Easing::EaseInOut,
            // Effects oscillate through explicit keyframes: linear between them.
            MotionToken::Wiggle | MotionToken::Pulse => Easing::Linear,
        }
    }

    /// The primary property the token animates.
    #[must_use]
    pub const fn primary_prop(self) -> AnimProp {
        match self {
            MotionToken::SlideUp => AnimProp::TranslateY,
            MotionToken::Wiggle => AnimProp::TranslateX,
            MotionToken::Pulse => AnimProp::ScaleX,
            // snappy/smooth/emphasized/fade/fadeScale drive opacity first.
            _ => AnimProp::Opacity,
        }
    }

    /// The secondary property (`fadeScale` scales while it fades).
    #[must_use]
    pub const fn secondary_prop(self) -> Option<AnimProp> {
        match self {
            MotionToken::FadeScale => Some(AnimProp::ScaleX),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for t in MotionToken::ALL {
            assert_eq!(MotionToken::from_name(t.name()), Some(t));
            assert_eq!(MotionToken::from_id(t.id()), Some(t));
        }
        assert_eq!(MotionToken::from_name("nope"), None);
    }

    #[test]
    fn ids_are_stable_and_dense() {
        // Append-only contract: ids are the array index.
        for (i, t) in MotionToken::ALL.into_iter().enumerate() {
            assert_eq!(t.id() as usize, i);
        }
    }

    #[test]
    fn fade_is_opacity_value_motion() {
        assert_eq!(MotionToken::Fade.primary_prop(), AnimProp::Opacity);
        assert_eq!(MotionToken::Fade.secondary_prop(), None);
        assert_eq!(MotionToken::FadeScale.secondary_prop(), Some(AnimProp::ScaleX));
    }
}
