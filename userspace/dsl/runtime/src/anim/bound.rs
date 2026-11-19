// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! `.motion(duration, curve)` as an intent (TASK-0354 P10, docs/dev/dsl/modifiers.md §`.motion`):
//! an [`AnimKind::Bound`](super::AnimKind::Bound) whose `token` packs the pair (duration in the
//! high nibble, curve in the low) and whose `value` is the node's identity
//! ([`transition_identity`](super::transition_identity) of its emitted path) — the host keys the
//! node's rest record by it, so a sibling added elsewhere does not move it and the two branches
//! of an `if` at one place are the same node. The names are the theme's two closed vocabularies,
//! in lockstep with the compiler's `registry::{MOTION_DURATIONS, MOTION_CURVES}` (the runtime does
//! not link the compiler; `tests/token_vocabulary_lockstep.rs` holds the two together).

use nexus_theme_tokens::{MotionCurveToken, MotionDurationToken};

/// The duration names, in the theme's order.
pub const DURATION_NAMES: [&str; 5] = ["instant", "swift", "quick", "base", "slow"];
/// The curve names, in the theme's order.
pub const CURVE_NAMES: [&str; 6] =
    ["spring", "springSoft", "springIcon", "smooth", "glide", "enter"];

const DURATIONS: [MotionDurationToken; 5] = [
    MotionDurationToken::Instant,
    MotionDurationToken::Swift,
    MotionDurationToken::Quick,
    MotionDurationToken::Base,
    MotionDurationToken::Slow,
];

const CURVES: [MotionCurveToken; 6] = [
    MotionCurveToken::Spring,
    MotionCurveToken::SpringSoft,
    MotionCurveToken::SpringIcon,
    MotionCurveToken::Smooth,
    MotionCurveToken::Glide,
    MotionCurveToken::Enter,
];

/// The duration a `.motion` names (`None`: not in the vocabulary — the checker rejects it).
#[must_use]
pub fn duration_named(name: &str) -> Option<MotionDurationToken> {
    let i = DURATION_NAMES.iter().position(|n| *n == name)?;
    DURATIONS.get(i).copied()
}

/// The curve a `.motion` names (`None`: not in the vocabulary).
#[must_use]
pub fn curve_named(name: &str) -> Option<MotionCurveToken> {
    let i = CURVE_NAMES.iter().position(|n| *n == name)?;
    CURVES.get(i).copied()
}

/// The pair packed into an intent's `token`.
#[must_use]
pub fn pack_bound(duration: MotionDurationToken, curve: MotionCurveToken) -> u8 {
    let d = DURATIONS.iter().position(|x| *x == duration).unwrap_or(0) as u8;
    let c = CURVES.iter().position(|x| *x == curve).unwrap_or(0) as u8;
    (d << 4) | c
}

/// The pair an intent's `token` carries (`None`: not a packed pair).
#[must_use]
pub fn unpack_bound(token: u8) -> Option<(MotionDurationToken, MotionCurveToken)> {
    let d = DURATIONS.get(usize::from(token >> 4))?;
    let c = CURVES.get(usize::from(token & 0x0f))?;
    Some((*d, *c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_named_pair_round_trips() {
        for d in DURATION_NAMES {
            for c in CURVE_NAMES {
                let (dt, ct) = (duration_named(d).expect(d), curve_named(c).expect(c));
                assert_eq!(unpack_bound(pack_bound(dt, ct)), Some((dt, ct)), "{d} {c}");
            }
        }
    }

    #[test]
    fn the_vocabularies_follow_the_theme_order() {
        assert_eq!(duration_named("quick"), Some(MotionDurationToken::Quick));
        assert_eq!(curve_named("springSoft"), Some(MotionCurveToken::SpringSoft));
    }

    #[test]
    fn test_reject_a_token_that_is_no_packed_pair() {
        assert_eq!(unpack_bound(0x50), None, "duration index 5 does not exist");
        assert_eq!(unpack_bound(0x06), None, "curve index 6 does not exist");
        assert_eq!(duration_named("fast"), None);
        assert_eq!(curve_named("bounce"), None);
    }
}
