// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The kit widgets' moving parts (TASK-0354 P10, design artifacts: CHK-8, M40, M12, PRG-4): for
//! a widget kind, the child paths of its builder's fixed structure and the motion each carries —
//! a part shown while a prop is on (the builder writes its absent state, its own opacity 0; the
//! host plays the change) or a part that travels to changes of its own place, size or rotation
//! (`.motion` on a part). The same contract as `press_offset` / `child_path`: the paths mirror
//! the widget crates' structure (`GlassToggle::TRACK_PARTS`, `GlassCheckbox::BOX_PARTS`,
//! `Select::CHEVRON_PART`), and `tests/motion_parts.rs` proves each lands on its part. The widget
//! crates stay free of the animation crate.

use crate::store::Value;
use animation::MotionToken;
use nexus_theme_tokens::{MotionCurveToken as Curve, MotionDurationToken as Dur};

/// The motion one part carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartMotion {
    /// Shown while `present`: the token's enter and exit (an `.animate` on the part).
    Presence { token: MotionToken, present: bool },
    /// Travels to its own changes on the pair (a `.motion` on the part).
    Bound { duration: Dur, curve: Curve },
}

/// At most this many moving parts per widget.
pub const MAX_PARTS: usize = 2;

/// The moving parts of a widget of `kind` with `props`: `(child path from the widget, motion)`.
#[must_use]
pub fn motion_parts(
    kind: &str,
    props: &[(alloc::string::String, Value)],
) -> [Option<(&'static [u32], PartMotion)>; MAX_PARTS] {
    let flag = |name: &str| props.iter().any(|(n, v)| n == name && matches!(v, Value::Bool(true)));
    let text = |name: &str| {
        props.iter().find(|(n, _)| n == name).and_then(|(_, v)| match v {
            Value::Str(s) => Some(s.as_str()),
            _ => None,
        })
    };
    let bound = |duration, curve| PartMotion::Bound { duration, curve };
    match kind {
        // The switch (the button appearance has no moving part): the on layer fades, the thumb
        // slides on the soft spring.
        "Toggle" if text("appearance") != Some("button") => [
            Some((
                &[0],
                PartMotion::Presence { token: MotionToken::Fade, present: flag("checked") },
            )),
            Some((&[1], bound(Dur::Quick, Curve::SpringSoft))),
        ],
        // The on layer fades on the swift step, the mark pops.
        "Checkbox" => {
            let present = flag("checked") || flag("indeterminate");
            [
                Some((&[0], PartMotion::Presence { token: MotionToken::Snappy, present })),
                Some((&[0, 0], PartMotion::Presence { token: MotionToken::Pop, present })),
            ]
        }
        // The chevron turns on the swift step.
        "Select" => [Some((&[1], bound(Dur::Swift, Curve::Smooth))), None],
        // The fill follows the value on the soft spring (nothing starts while the slider is held:
        // the host's rule for a part inside the pressed control).
        "Slider" if text("variant") != Some("thumb") => {
            [Some((&[0], bound(Dur::Quick, Curve::SpringSoft))), None]
        }
        // A determinate bar's fill follows the value; with a header it sits under it.
        "ProgressBar" if props.iter().any(|(n, v)| n == "value" && matches!(v, Value::Int(_))) => {
            let header = text("label").is_some() || flag("showValue");
            let fill: &'static [u32] = if header { &[1, 0] } else { &[0] };
            [Some((fill, bound(Dur::Quick, Curve::Smooth))), None]
        }
        _ => [None, None],
    }
}
