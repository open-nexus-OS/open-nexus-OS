// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the next value of a two-way bound control — ONE pure rule for
//! "the interaction supplies the value" (TASK-0077B P2b, IR v1.8).
//!
//! This used to be an inline special case in `view.rs`: a `Bool` was flipped,
//! and *every other kind* fell into `_ => return Ok(None)`. That silent arm is
//! why the Control Center's brightness and volume sliders never responded to a
//! touch — a `Slider` bound to `$state` had no rule to reach, so the honest
//! thing at the time was to synthesize no handler at all, which is what the
//! widget catalog did.
//!
//! The rule now arrives WITH the handler: the lowering reads the derivation off
//! the widget catalog and writes it into the IR (`Handler.bind` =
//! `BindWrite { target, value }`), so this module never sees a widget kind and
//! an already-compiled `.nxir` keeps its meaning when the runtime changes —
//! the same information-hiding seam `press_offset` uses for the kit's tree
//! shape (`docs/dev/dsl/principles.md` §1, §5 "one state model … no
//! alternates").
//!
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the unit tests below + `dsl_conformance` corpus
//! DOCS: docs/dev/dsl/ir.md (schema changelog), docs/dev/dsl/state.md

use crate::store::Value;
use nexus_dsl_ir::ui_ir_capnp::BindValue;
use nexus_layout_types::{FxPx, Rect};

/// Percent range a [`BindValue::TrackFraction`] control spans. The `Slider`'s
/// value IS this percent (`nexus_widget_slider` clamps 0..=100 and splits its
/// track by `value` vs `100 - value`), so the fraction needs no per-widget
/// scale constant.
const TRACK_PERCENT: i64 = 100;

/// What an interaction hands the bind rule.
///
/// Both arms carry everything their derivation needs and nothing more; there is
/// no ambient state, which is what keeps [`next_value`] a pure function the
/// host can test without a scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interaction<'i> {
    /// A pointer landed at (`x`, `y`) inside the handler's own `rect`. The
    /// point is already in the box's coordinate space — `interact::hit_scrolled`
    /// resolves the scroll transform once and reports the point it used, so a
    /// control inside a scrolled pane cannot disagree with the pixels.
    Point { rect: Rect, x: FxPx, y: FxPx },
    /// The text-entry path supplied a new string (keyboard, IME commit).
    Text(&'i str),
}

/// The next value for a two-way bound field, or `None` when this interaction
/// cannot produce one for this rule.
///
/// `None` is a real answer, not a swallowed error: a `Text` interaction on a
/// `ToggleBool` field, or a pointer on a field holding the wrong type, must
/// write NOTHING rather than a made-up value. The caller leaves the store
/// untouched and reports no damage.
#[must_use]
pub fn next_value(rule: BindValue, current: Option<&Value>, at: &Interaction) -> Option<Value> {
    match (rule, at) {
        // The toggle contract: the interaction carries no value, the CURRENT
        // one is inverted. A field that is not a Bool has no flip.
        (BindValue::ToggleBool, Interaction::Point { .. }) => match current {
            Some(Value::Bool(b)) => Some(Value::Bool(!b)),
            _ => None,
        },
        (BindValue::Text, Interaction::Text(text)) => {
            Some(Value::Str(alloc::string::String::from(*text)))
        }
        // The point's position ACROSS the control's own box, over the box's
        // TRAVEL — the distance between its first and last pixel, not its
        // width. A rect is half-open, so a `width`-wide track spans pixels
        // `0..=width-1`: dividing by `width` would make the far end read 99
        // and put 100 on a pixel the control does not own, which is a slider
        // whose maximum cannot be reached. A box with no travel (zero width,
        // or a single pixel after a layout that had no room) cannot say where
        // the point is, and no value is the honest answer for that.
        (BindValue::TrackFraction, Interaction::Point { rect, x, .. }) => {
            let travel = i64::from(rect.width.0) - 1;
            if travel <= 0 {
                return None;
            }
            // Rounded to the NEAREST percent, not floored: a floor biases the
            // whole scale down by up to one step, so the visual centre of a
            // 400px track reads 49 and a user who aims at a value lands below
            // it. Both operands are non-negative after the clamp.
            let offset = i64::from(x.0) - i64::from(rect.x.0);
            let scaled = offset.clamp(0, travel) * TRACK_PERCENT + travel / 2;
            Some(Value::Int(scaled / travel))
        }
        // Wrong interaction for the rule: text typed at a slider, a pointer at
        // a text field's Change bind. Nothing is written.
        (BindValue::ToggleBool | BindValue::TrackFraction, Interaction::Text(_))
        | (BindValue::Text, Interaction::Point { .. }) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::String;

    fn track(x: i32, w: i32) -> Rect {
        Rect::new(FxPx::new(x), FxPx::new(0), FxPx::new(w), FxPx::new(28))
    }

    fn at(rect: Rect, x: i32) -> Interaction<'static> {
        Interaction::Point { rect, x: FxPx::new(x), y: FxPx::new(14) }
    }

    #[test]
    fn a_tap_flips_the_bool_and_nothing_else() {
        let rect = track(0, 100);
        let flip = |v| next_value(BindValue::ToggleBool, Some(&v), &at(rect, 50));
        assert_eq!(flip(Value::Bool(false)), Some(Value::Bool(true)));
        assert_eq!(flip(Value::Bool(true)), Some(Value::Bool(false)));
        assert_eq!(flip(Value::Int(3)), None, "an Int has no flip");
        assert_eq!(
            next_value(BindValue::ToggleBool, None, &at(rect, 50)),
            None,
            "an unset field has no current value to invert"
        );
    }

    /// The whole point of the package: a point across the track IS the value.
    #[test]
    fn the_fraction_is_the_point_across_the_control_s_own_box() {
        let rect = track(200, 400); // pixels 200..=599, travel 399
        let v = |x| next_value(BindValue::TrackFraction, Some(&Value::Int(0)), &at(rect, x));
        assert_eq!(v(200), Some(Value::Int(0)), "first pixel");
        assert_eq!(v(599), Some(Value::Int(100)), "last pixel");
        assert_eq!(v(300), Some(Value::Int(25)));
    }

    /// Rounding, not flooring: both pixels at the visual centre of an
    /// even-width track must read the middle of the range. Flooring put the
    /// left one at 49 and shifted every value in between down a step.
    #[test]
    fn the_percent_rounds_to_the_nearest_step() {
        let rect = track(0, 400); // travel 399 — no exact centre pixel
        let v = |x| next_value(BindValue::TrackFraction, Some(&Value::Int(0)), &at(rect, x));
        assert_eq!(v(199), Some(Value::Int(50)), "left centre pixel");
        assert_eq!(v(200), Some(Value::Int(50)), "right centre pixel");
    }

    /// Hit slop means the point can legitimately land OUTSIDE the box; the
    /// value must saturate at the ends rather than run negative or past 100.
    #[test]
    fn a_point_outside_the_box_clamps_instead_of_escaping_the_range() {
        let rect = track(200, 400);
        let v = |x| next_value(BindValue::TrackFraction, Some(&Value::Int(0)), &at(rect, x));
        assert_eq!(v(150), Some(Value::Int(0)), "left of the track");
        assert_eq!(v(9_000), Some(Value::Int(100)), "far right");
    }

    /// Both ends of the range must be reachable by aiming at the control's own
    /// pixels. A rect is half-open, so the last pixel a track owns is
    /// `x + width - 1`: measuring the fraction over `width` instead of over the
    /// TRAVEL put 100 on a pixel outside the box and capped every real tap at
    /// 99 — a slider that can never be turned up all the way.
    #[test]
    fn both_ends_of_the_range_are_reachable_from_inside_the_box() {
        for width in [2, 3, 28, 400, 1_920] {
            let rect = track(0, width);
            let v = |x| next_value(BindValue::TrackFraction, Some(&Value::Int(0)), &at(rect, x));
            assert_eq!(v(0), Some(Value::Int(0)), "first pixel of a {width}px track");
            assert_eq!(v(width - 1), Some(Value::Int(100)), "last pixel of a {width}px track");
        }
    }

    #[test]
    fn test_reject_track_without_travel_writes_nothing() {
        for width in [0, 1] {
            let rect = track(200, width);
            assert_eq!(
                next_value(BindValue::TrackFraction, Some(&Value::Int(40)), &at(rect, 200)),
                None,
                "a {width}px track cannot say where the point is"
            );
        }
    }

    /// A rule and an interaction that do not belong together must write
    /// NOTHING — never a default, never the current value re-written.
    #[test]
    fn test_reject_mismatched_interaction_writes_nothing() {
        let rect = track(0, 100);
        let text = Interaction::Text("42");
        assert_eq!(next_value(BindValue::ToggleBool, Some(&Value::Bool(true)), &text), None);
        assert_eq!(next_value(BindValue::TrackFraction, Some(&Value::Int(0)), &text), None);
        assert_eq!(
            next_value(BindValue::Text, Some(&Value::Str(String::from("x"))), &at(rect, 50)),
            None,
            "a pointer carries no text"
        );
    }

    #[test]
    fn text_entry_writes_what_it_carries() {
        assert_eq!(
            next_value(BindValue::Text, None, &Interaction::Text("hello")),
            Some(Value::Str(String::from("hello"))),
            "the current value is irrelevant — the interaction brought its own"
        );
    }
}
