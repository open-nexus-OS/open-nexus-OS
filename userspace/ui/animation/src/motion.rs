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
    /// Enter from the leading edge: a surface pushed in from the side it is anchored to (the
    /// system toast at the left edge — design artifacts: system toast).
    PushInline = 9,
    /// A menu opens: it fades in while it grows from 0.92, on the spring in the `quick` step,
    /// from the corner it hangs on (design artifacts: menu entrance) — see [`MotionOrigin`].
    MenuIn = 10,
    /// A small part appears or goes: it fades in while it grows from 0.60 on the icon spring in
    /// the `quick` step (design artifacts: a check mark, a radio dot, a PIN dot pop in).
    Pop = 11,
    /// Content swapped in place along the reading axis: on a change of its trigger the node starts
    /// 28 px on the side it travels from, invisible, and glides to rest in the `base` step
    /// (design artifacts: the calendar's month slide — a later month comes from the trailing side).
    SwapInline = 12,
}

/// The point a token's scale grows from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionOrigin {
    /// The moved surface's own centre.
    Centre,
    /// The corner the surface hangs on: on each axis the edge of the surface nearer its
    /// container's edge (the centre on an axis it is centred on) — a menu under its button
    /// grows out of that button's corner, in either writing direction.
    Anchor,
}

impl MotionToken {
    /// Every token, in id order (checker + docs iterate this).
    pub const ALL: [MotionToken; 13] = [
        MotionToken::Snappy,
        MotionToken::Smooth,
        MotionToken::Emphasized,
        MotionToken::Fade,
        MotionToken::SlideUp,
        MotionToken::FadeScale,
        MotionToken::Wiggle,
        MotionToken::Pulse,
        MotionToken::SlideDown,
        MotionToken::PushInline,
        MotionToken::MenuIn,
        MotionToken::Pop,
        MotionToken::SwapInline,
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
            MotionToken::PushInline => "pushInline",
            MotionToken::MenuIn => "menuIn",
            MotionToken::Pop => "pop",
            MotionToken::SwapInline => "swapInline",
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
            MotionToken::Snappy
            | MotionToken::Smooth
            | MotionToken::Emphasized
            | MotionToken::Pop => MotionCategory::Value,
            MotionToken::Fade
            | MotionToken::SlideUp
            | MotionToken::SlideDown
            | MotionToken::FadeScale
            | MotionToken::PushInline
            | MotionToken::MenuIn => MotionCategory::Transition,
            MotionToken::Wiggle | MotionToken::Pulse | MotionToken::SwapInline => {
                MotionCategory::Effect
            }
        }
    }

    /// The theme duration step this token eases over (0 ms under a
    /// reduced-motion theme ⇒ the driver jumps to the final frame) — the design
    /// artifacts' timings: the panels and the launcher enter in `quick` (their
    /// 0.28–0.30 s) and so does a menu, a dialog and the system toast in `slow` (0.5 s, a spring).
    #[must_use]
    pub const fn duration(self) -> MotionDurationToken {
        match self {
            MotionToken::Snappy => MotionDurationToken::Swift,
            MotionToken::Pulse
            | MotionToken::Fade
            | MotionToken::SlideUp
            | MotionToken::SlideDown
            | MotionToken::MenuIn
            | MotionToken::Pop => MotionDurationToken::Quick,
            MotionToken::Smooth | MotionToken::SwapInline => MotionDurationToken::Base,
            MotionToken::Emphasized
            | MotionToken::FadeScale
            | MotionToken::PushInline
            | MotionToken::Wiggle => MotionDurationToken::Slow,
        }
    }

    /// The theme curve this token follows (the design artifacts' motion: an
    /// entrance springs or settles, a fade is smooth).
    #[must_use]
    pub const fn curve(self) -> MotionCurveToken {
        match self {
            MotionToken::Snappy => MotionCurveToken::SpringSoft,
            MotionToken::Emphasized
            | MotionToken::FadeScale
            | MotionToken::PushInline
            | MotionToken::MenuIn => MotionCurveToken::Spring,
            MotionToken::SlideUp | MotionToken::SlideDown => MotionCurveToken::Enter,
            MotionToken::Pop => MotionCurveToken::SpringIcon,
            MotionToken::SwapInline => MotionCurveToken::Glide,
            MotionToken::Smooth | MotionToken::Fade | MotionToken::Wiggle | MotionToken::Pulse => {
                MotionCurveToken::Smooth
            }
        }
    }

    /// The easing every driver evaluates — the app's keyframe track and the
    /// compositor's motion layers alike: the token's curve, exactly (a spring's
    /// overshoot included); effects oscillate through explicit keyframes, linear
    /// between them.
    #[must_use]
    pub const fn easing(self) -> Easing {
        match self {
            MotionToken::Wiggle | MotionToken::Pulse => Easing::Linear,
            _ => Easing::Curve(self.curve()),
        }
    }

    /// The primary property the token animates.
    #[must_use]
    pub const fn primary_prop(self) -> AnimProp {
        match self {
            MotionToken::SlideUp | MotionToken::SlideDown | MotionToken::FadeScale => {
                AnimProp::TranslateY
            }
            MotionToken::Wiggle | MotionToken::PushInline | MotionToken::SwapInline => {
                AnimProp::TranslateX
            }
            MotionToken::Pulse => AnimProp::ScaleX,
            // snappy/smooth/emphasized/fade/fadeScale drive opacity first.
            _ => AnimProp::Opacity,
        }
    }

    /// The secondary property: every entrance of the design artifacts grows
    /// in a little while it travels (a panel from 0.97/0.96, a dialog from 0.94,
    /// the toast from 0.96).
    #[must_use]
    pub const fn secondary_prop(self) -> Option<AnimProp> {
        match self {
            MotionToken::FadeScale
            | MotionToken::SlideUp
            | MotionToken::SlideDown
            | MotionToken::PushInline
            | MotionToken::MenuIn
            | MotionToken::Pop => Some(AnimProp::ScaleX),
            _ => None,
        }
    }

    /// The point the token's scale grows from.
    #[must_use]
    pub const fn origin(self) -> MotionOrigin {
        match self {
            MotionToken::MenuIn => MotionOrigin::Anchor,
            _ => MotionOrigin::Centre,
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
            // The travel tokens rest IN place (0) when present; absent, they
            // sit on the side they travel FROM — SlideUp and a dialog below,
            // SlideDown above, the toast at the leading edge.
            AnimProp::TranslateY | AnimProp::TranslateX if present => 0.0,
            AnimProp::TranslateY => match self {
                MotionToken::SlideDown => SLIDE_DOWN_PX,
                MotionToken::FadeScale => DIALOG_RISE_PX,
                MotionToken::SlideUp => SLIDE_PX,
                _ => 0.0,
            },
            AnimProp::TranslateX => match self {
                MotionToken::PushInline => PUSH_INLINE_PX,
                _ => 0.0,
            },
            AnimProp::ScaleX | AnimProp::ScaleY if present => 1.0,
            AnimProp::ScaleX | AnimProp::ScaleY => match self {
                MotionToken::FadeScale => FADE_SCALE_FROM,
                MotionToken::SlideUp => SLIDE_UP_SCALE,
                MotionToken::SlideDown | MotionToken::PushInline => SLIDE_DOWN_SCALE,
                MotionToken::MenuIn => MENU_IN_FROM,
                MotionToken::Pop => POP_FROM,
                _ => 1.0,
            },
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
/// A dialog's rise (px): it comes up 10 px while it grows in (design artifacts:
/// overlay entrance).
pub const DIALOG_RISE_PX: f32 = 10.0;
/// The toast's push (px): in from the leading edge.
pub const PUSH_INLINE_PX: f32 = -16.0;
/// FadeScale's absent-state scale: a dialog grows in from 0.94.
pub const FADE_SCALE_FROM: f32 = 0.94;
/// SlideUp's absent-state scale (a panel rising into place).
pub const SLIDE_UP_SCALE: f32 = 0.97;
/// SlideDown's and the toast's absent-state scale.
pub const SLIDE_DOWN_SCALE: f32 = 0.96;
/// A menu's absent-state scale: it grows in from 0.92.
pub const MENU_IN_FROM: f32 = 0.92;

/// A part's absent-state scale under `pop`: it grows in from 0.60 (design artifacts: a check
/// mark from .6 — a radio dot's .4 is drawn the same under an opacity near 0 on ≤ 8 px).
pub const POP_FROM: f32 = 0.60;
/// `swapInline`'s travel (px): the content comes in from 28 px on the side it travels from.
pub const SWAP_INLINE_PX: f32 = 28.0;

/// Where `swapInline` holds the node at progress `t` (`0..=1`), in px along the reading axis:
/// from `SWAP_INLINE_PX` on the side of travel (`dir` > 0: a later value, from the trailing side;
/// < 0: an earlier one, from the leading side; 0: from the trailing side — a change without a
/// direction) to rest, on the token's glide. The opacity rises on the same progress.
#[must_use]
pub fn swap_px(t: f32, dir: i32) -> f32 {
    let side = if dir < 0 { -1.0 } else { 1.0 };
    let e = MotionToken::SwapInline.easing().ease(t.clamp(0.0, 1.0));
    side * SWAP_INLINE_PX * (1.0 - e)
}

/// The attention shake of `wiggle` at progress `t` (`0..=1`), in px to the side (design
/// artifacts: the lock field's shake, 500 ms): ten equal steps through −1, +3, −6, +6, −6, +6,
/// −6, +3, −1 back to rest, each step eased on `cubic-bezier(.36, .07, .19, .97)` — as a style
/// sheet eases every keyframe interval, not the whole run.
#[must_use]
pub fn shake_px(t: f32) -> f32 {
    const STOPS: [f32; 11] = [0.0, -1.0, 3.0, -6.0, 6.0, -6.0, 6.0, -6.0, 3.0, -1.0, 0.0];
    let steps = t.clamp(0.0, 1.0) * 10.0;
    let i = (steps as usize).min(9);
    let eased = crate::keyframe::cubic_bezier([0.36, 0.07, 0.19, 0.97], steps - i as f32);
    STOPS[i] + (STOPS[i + 1] - STOPS[i]) * eased
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

    /// TASK-0354 P10 (design artifacts: CHK-8, RAD-6, M64): `pop` fades a part in while it grows
    /// from 0.60 on the icon spring in `quick` — its overshoot included.
    #[test]
    fn pop_starts_at_60_percent_and_follows_the_icon_spring() {
        let pop = MotionToken::Pop;
        assert_eq!(MotionToken::from_name("pop"), Some(pop));
        assert_eq!(pop.id(), 11);
        assert_eq!(pop.category(), MotionCategory::Value);
        assert_eq!(pop.duration(), MotionDurationToken::Quick);
        assert_eq!(
            (pop.primary_prop(), pop.secondary_prop()),
            (AnimProp::Opacity, Some(AnimProp::ScaleX))
        );
        assert_eq!(pop.resting(AnimProp::ScaleX, false), 0.60);
        assert_eq!(pop.resting(AnimProp::ScaleX, true), 1.0);
        assert_eq!(pop.resting(AnimProp::Opacity, false), 0.0);
        let curve = MotionCurveToken::SpringIcon.control_points();
        for t in [0.0f32, 0.1, 0.5, 1.0] {
            let e = pop.easing().ease(t);
            assert!((e - crate::keyframe::cubic_bezier(curve, t)).abs() < 1e-6, "t={t}");
        }
        assert!(pop.easing().ease(0.5) > 1.0, "the icon spring overshoots");
        assert_eq!(pop.easing().ease(1.0), 1.0);
    }

    /// TASK-0354 P12 (design artifacts: M10, the calendar's month slide): a later value comes in
    /// 28 px from the trailing side, an earlier one from the leading side, on the glide in `base`.
    #[test]
    fn swap_inline_enters_28_px_from_the_side_of_travel() {
        let s = MotionToken::SwapInline;
        assert_eq!((MotionToken::from_name("swapInline"), s.id()), (Some(s), 12));
        assert_eq!(s.category(), MotionCategory::Effect);
        assert_eq!((s.duration(), s.curve()), (MotionDurationToken::Base, MotionCurveToken::Glide));
        assert_eq!(swap_px(0.0, 1), 28.0);
        assert_eq!(swap_px(0.0, -1), -28.0);
        assert_eq!(swap_px(0.0, 0), 28.0, "no direction: from the trailing side");
        assert_eq!(swap_px(1.0, 1), 0.0);
        let glide = MotionCurveToken::Glide.control_points();
        for t in [0.1f32, 0.5] {
            let want = 28.0 * (1.0 - crate::keyframe::cubic_bezier(glide, t));
            assert!((swap_px(t, 1) - want).abs() < 1e-4 && (swap_px(t, -1) + want).abs() < 1e-4);
        }
        assert!(swap_px(0.5, 1) < 3.0, "the glide is most of the way at half time");
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
        // The entrances grow in (design artifacts); value tokens rest unscaled.
        assert_eq!(MotionToken::FadeScale.resting(AnimProp::ScaleX, false), FADE_SCALE_FROM);
        assert_eq!(MotionToken::Snappy.resting(AnimProp::ScaleX, false), 1.0);
    }

    /// TASK-0354 P3: every token follows its design curve exactly — sampled at named times
    /// against values an independent solver computed from the artifacts' `cubic-bezier` points
    /// (the panels' entrance settles fast; a dialog's spring passes 1 and comes back).
    #[test]
    fn the_tokens_follow_the_design_curves_at_named_times() {
        let at = |token: MotionToken, t: f32| token.easing().ease(t);
        let close = |a: f32, b: f32| (a - b).abs() < 0.005;
        for (t, want) in [(0.1, 0.4944), (0.25, 0.8256), (0.5, 0.9718), (0.75, 0.9977)] {
            assert!(close(at(MotionToken::SlideDown, t), want), "slideDown at {t}");
            assert!(close(at(MotionToken::SlideUp, t), want), "slideUp at {t}");
        }
        for (t, want) in [(0.1, 0.3781), (0.25, 0.7914), (0.5, 1.0465), (0.75, 1.0279)] {
            assert!(close(at(MotionToken::FadeScale, t), want), "fadeScale at {t}");
            assert!(close(at(MotionToken::PushInline, t), want), "pushInline at {t}");
            assert!(close(at(MotionToken::MenuIn, t), want), "menuIn at {t}");
        }
        for (t, want) in [(0.1, 0.0259), (0.25, 0.2366), (0.5, 0.7756), (0.75, 0.9594)] {
            assert!(close(at(MotionToken::Fade, t), want), "fade at {t}");
        }
        for token in MotionToken::ALL {
            assert!(close(at(token, 0.0), 0.0) && close(at(token, 1.0), 1.0), "{token:?} ends");
        }
    }

    /// Every entrance of the design artifacts: where it starts and in which theme step it runs.
    #[test]
    fn every_entrance_starts_where_the_design_draws_it() {
        use AnimProp::{ScaleX, TranslateX, TranslateY};
        let from = |t: MotionToken, p| t.resting(p, false);
        assert_eq!(
            (from(MotionToken::SlideUp, TranslateY), from(MotionToken::SlideUp, ScaleX)),
            (16.0, 0.97)
        );
        assert_eq!(
            (from(MotionToken::SlideDown, TranslateY), from(MotionToken::SlideDown, ScaleX)),
            (-10.0, 0.96)
        );
        assert_eq!(
            (from(MotionToken::FadeScale, TranslateY), from(MotionToken::FadeScale, ScaleX)),
            (10.0, 0.94)
        );
        assert_eq!(
            (from(MotionToken::PushInline, TranslateX), from(MotionToken::PushInline, ScaleX)),
            (-16.0, 0.96)
        );
        assert_eq!(MotionToken::PushInline.primary_prop(), TranslateX);
        // A menu only fades and grows, from 0.92, out of the corner it hangs on.
        assert_eq!(MotionToken::MenuIn.primary_prop(), AnimProp::Opacity);
        assert_eq!(from(MotionToken::MenuIn, ScaleX), 0.92);
        assert_eq!(MotionToken::MenuIn.origin(), MotionOrigin::Anchor);
        assert_eq!(MotionToken::MenuIn.duration(), MotionDurationToken::Quick);
        assert_eq!(MotionToken::MenuIn.curve(), MotionCurveToken::Spring);
        for t in MotionToken::ALL.into_iter().filter(|t| *t != MotionToken::MenuIn) {
            assert_eq!(t.origin(), MotionOrigin::Centre, "{t:?}");
        }
        for t in [
            MotionToken::SlideUp,
            MotionToken::SlideDown,
            MotionToken::FadeScale,
            MotionToken::PushInline,
            MotionToken::MenuIn,
        ] {
            assert_eq!(t.secondary_prop(), Some(ScaleX), "{t:?} grows in while it travels");
            assert_eq!(t.resting(TranslateY, true) + t.resting(TranslateX, true), 0.0);
            assert_eq!(t.resting(ScaleX, true), 1.0);
        }
        assert_eq!(MotionToken::SlideDown.duration(), MotionDurationToken::Quick);
        assert_eq!(MotionToken::SlideUp.duration(), MotionDurationToken::Quick);
        assert_eq!(MotionToken::FadeScale.duration(), MotionDurationToken::Slow);
        assert_eq!(MotionToken::PushInline.duration(), MotionDurationToken::Slow);
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

    /// The shake at its keyframes (every tenth of its 500 ms) and between two of them: eased per
    /// interval — halfway through the step from −6 to +6 it is already past the middle.
    #[test]
    fn the_shake_follows_the_designs_keyframes() {
        let stops = [0.0, -1.0, 3.0, -6.0, 6.0, -6.0, 6.0, -6.0, 3.0, -1.0, 0.0];
        for (k, stop) in stops.iter().enumerate() {
            assert!((shake_px(k as f32 / 10.0) - stop).abs() < 0.01, "stop {k}");
        }
        let mid = shake_px(0.35);
        assert!(mid > 3.0 && mid < 6.0, "eased out of -6 towards +6: {mid}");
        assert_eq!(shake_px(-1.0), 0.0);
        assert_eq!(shake_px(2.0), 0.0);
        assert_eq!(MotionToken::Wiggle.duration(), MotionDurationToken::Slow);
    }
}
