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
    /// Enter/leave by sliding DOWN into place — the drop-down counterpart of
    /// [`MotionToken::SlideUp`], for a surface anchored BELOW its trigger
    /// (a top-bar panel falls out of the bar; it does not rise into it).
    SlideDown = 8,
}

impl MotionToken {
    /// Every token, in id order (checker + docs iterate this).
    pub const ALL: [MotionToken; 9] = [
        MotionToken::Snappy,
        MotionToken::Smooth,
        MotionToken::Emphasized,
        MotionToken::Fade,
        MotionToken::SlideUp,
        MotionToken::FadeScale,
        MotionToken::Wiggle,
        MotionToken::Pulse,
        MotionToken::SlideDown,
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
            MotionToken::SlideDown => "slideDown",
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
            MotionToken::Fade
            | MotionToken::SlideUp
            | MotionToken::SlideDown
            | MotionToken::FadeScale => MotionCategory::Transition,
            MotionToken::Wiggle | MotionToken::Pulse => MotionCategory::Effect,
        }
    }

    /// The theme duration step this token eases over (0 ms under a
    /// reduced-motion theme ⇒ the driver jumps to the final frame).
    #[must_use]
    pub const fn duration(self) -> MotionDurationToken {
        match self {
            MotionToken::Snappy => MotionDurationToken::Swift,
            MotionToken::Pulse
            | MotionToken::Fade
            | MotionToken::FadeScale
            | MotionToken::SlideDown => MotionDurationToken::Quick,
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
            MotionToken::SlideUp | MotionToken::SlideDown => MotionCurveToken::Glide,
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
            | MotionToken::SlideDown
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
            MotionToken::SlideUp | MotionToken::SlideDown => AnimProp::TranslateY,
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

    /// Where `prop` RESTS under this token for a driving value that is
    /// `present` (non-zero) or absent (zero) — the `.animate(token, value:)`
    /// contract, and the seed a host writes on first sight of the node.
    ///
    /// **`.animate` is a PRESENT/ABSENT binding, not "move when the value
    /// changes".** Read the opacity arm literally: an absent value rests at
    /// `0.0`, so a node carrying an opacity-primary token
    /// (`snappy`/`smooth`/`emphasized`/`fade`/`fadeScale`) and a driving value
    /// of 0 is INVISIBLE, and stays invisible until the value becomes
    /// non-zero. That is the intended semantic — and it is also the one that
    /// blanks a whole card when the `value:` expression cannot be folded to a
    /// number, which is why the folding side must refuse such an expression
    /// rather than pass 0 (see the DSL runtime's `stamp_anim`).
    ///
    /// Lives here, on the token, because both the app-host seed path and the
    /// app conformance tests have to answer this question the same way — the
    /// host copy used to be the only one, inside a RISC-V-only module no host
    /// test could reach.
    #[must_use]
    pub fn resting(self, prop: AnimProp, present: bool) -> f32 {
        match prop {
            AnimProp::Opacity => {
                if present {
                    1.0
                } else {
                    0.0
                }
            }
            // Both slide tokens rest IN place (0) when present; absent, they
            // sit on the side they travel FROM — SlideUp below, SlideDown
            // above.
            AnimProp::TranslateY => {
                if present {
                    0.0
                } else if matches!(self, MotionToken::SlideDown) {
                    SLIDE_DOWN_PX
                } else {
                    SLIDE_PX
                }
            }
            AnimProp::ScaleX | AnimProp::ScaleY => {
                if matches!(self, MotionToken::FadeScale) && !present {
                    FADE_SCALE_FROM
                } else {
                    1.0
                }
            }
            _ => prop.identity(),
        }
    }
}

/// SlideUp travel (px): the offset a slide-in transition starts from.
pub const SLIDE_PX: f32 = 16.0;
/// SlideDown travel (px): a drop-down starts ABOVE its resting place, so the
/// offset is negative — and shorter, because it falls out of a 36px bar rather
/// than rising from the bottom of the screen.
pub const SLIDE_DOWN_PX: f32 = -10.0;
/// FadeScale's absent-state scale (grows to 1.0 on enter, per animation.md).
pub const FADE_SCALE_FROM: f32 = 0.92;

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

    /// The trap, written down: on every opacity-primary token, a driving value
    /// of ZERO rests the node fully TRANSPARENT. `.animate(snappy, value: x)`
    /// therefore means "visible while x != 0" — it is not "animate on change".
    ///
    /// This cost a day on the calculator: a `Str` `value:` folded to 0, the
    /// display card seeded to opacity 0, and the transform cascaded to every
    /// box inside it — a blank card while the store, the layout and the text
    /// runs were all provably correct. The sentence was true the whole time
    /// and written nowhere.
    #[test]
    fn a_zero_driving_value_rests_hidden() {
        for token in MotionToken::ALL {
            if token.primary_prop() != AnimProp::Opacity {
                continue;
            }
            assert_eq!(
                token.resting(AnimProp::Opacity, false),
                0.0,
                "{token:?}: an absent value must rest hidden"
            );
            assert_eq!(
                token.resting(AnimProp::Opacity, true),
                1.0,
                "{token:?}: a present value must rest fully drawn"
            );
        }
    }

    /// The travel tokens rest IN PLACE when present and offset when absent —
    /// and SlideDown falls from above, so its offset is the negative one.
    #[test]
    fn travel_tokens_rest_in_place_when_present() {
        assert_eq!(MotionToken::SlideUp.resting(AnimProp::TranslateY, true), 0.0);
        assert_eq!(MotionToken::SlideUp.resting(AnimProp::TranslateY, false), SLIDE_PX);
        assert_eq!(MotionToken::SlideDown.resting(AnimProp::TranslateY, false), SLIDE_DOWN_PX);
        assert!(SLIDE_DOWN_PX < 0.0, "a drop-down starts above its resting place");
        // Only fadeScale scales in; every other token rests unscaled.
        assert_eq!(MotionToken::FadeScale.resting(AnimProp::ScaleX, false), FADE_SCALE_FROM);
        assert_eq!(MotionToken::Snappy.resting(AnimProp::ScaleX, false), 1.0);
    }

    /// A property the token does not drive rests at ITS identity, never at 0 —
    /// a scale must not be seeded to zero by a token that only fades.
    #[test]
    fn an_undriven_property_rests_at_its_own_identity() {
        assert_eq!(AnimProp::Opacity.identity(), 1.0);
        assert_eq!(AnimProp::ScaleX.identity(), 1.0);
        assert_eq!(AnimProp::TranslateX.identity(), 0.0);
        assert_eq!(MotionToken::Fade.resting(AnimProp::BlurRadius, false), 0.0);
    }
}
