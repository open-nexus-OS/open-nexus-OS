// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The compositor loop's one-shot wake (TASK-0354): the loop sleeps on its waitset and
//! owes itself a wake only at a deadline the screen changes by — the caret's next blink, the
//! wait ring's next sprite, the ring's failsafe. One kernel timer carries the earliest of them
//! (`runtime/caret.rs` arms it). Nothing here is clocked by a present's completion: on a display
//! without a vertical blank a completion returns in microseconds, and a ring stepped by
//! completions turned a launch's wait into a spin that starved the launching app.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: host unit tests below; the visible lanes (`windowd: cursor ring on`, a launch
//!   that surfaces while the ring turns)

/// The wait ring's sprite at `now_ns` and when the next one is due: `step_ns` per sprite,
/// `frames` sprites per turn. The next deadline lies after `now_ns` (short of the clock's end).
pub(crate) fn ring_step(now_ns: u64, step_ns: u64, frames: u8) -> (u8, u64) {
    let step_ns = step_ns.max(1);
    let n = now_ns / step_ns;
    let sprite = (n % u64::from(frames.max(1))) as u8;
    (sprite, n.saturating_add(1).saturating_mul(step_ns))
}

/// The earliest of two owed deadlines (`None`: nothing owed).
pub(crate) fn earliest(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, None) => a,
        (None, b) => b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STEP: u64 = 90_000_000;

    #[test]
    fn a_sprite_holds_for_one_step_and_names_the_next() {
        assert_eq!(ring_step(0, STEP, 8), (0, STEP));
        assert_eq!(ring_step(STEP - 1, STEP, 8), (0, STEP));
        assert_eq!(ring_step(STEP, STEP, 8), (1, 2 * STEP));
        assert_eq!(ring_step(8 * STEP + 5, STEP, 8), (0, 9 * STEP));
    }

    #[test]
    fn the_next_sprite_is_always_ahead_of_now() {
        for now in [0, 1, STEP - 1, STEP, STEP + 1, 123_456_789_012] {
            assert!(ring_step(now, STEP, 8).1 > now, "a deadline at or before now spins");
        }
    }

    #[test]
    fn a_degenerate_ring_never_divides_by_zero() {
        assert_eq!(ring_step(5, 0, 0), (0, 6));
        assert_eq!(ring_step(u64::MAX, STEP, 8).1, u64::MAX);
    }

    #[test]
    fn the_wake_carries_the_earliest_owed_deadline() {
        assert_eq!(earliest(None, None), None);
        assert_eq!(earliest(Some(7), None), Some(7));
        assert_eq!(earliest(None, Some(9)), Some(9));
        assert_eq!(earliest(Some(7), Some(3)), Some(3));
    }
}
