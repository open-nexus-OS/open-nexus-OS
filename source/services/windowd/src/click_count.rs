// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: a press's click count (TASK-0346 P2, RFC-0113 §4): a press within the double-click
//! interval of the last one and within the slop of its place counts one more — two take a word,
//! three a field's whole text; any other press is a first click. Pure, host-tested; windowd counts
//! where it sees every press, before it routes one to a surface.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below

use nexus_display_proto::surface_input::{PointerFacts, CLICKS_MAX};

/// The double-click interval (the desktop default; a setting later — RFC-0113's open question).
pub const DOUBLE_CLICK_NS: u64 = 500_000_000;
/// How far (either axis, display pixels) a second click may land from the first.
pub const SLOP_PX: i32 = 4;

/// The last press: its time, place and count.
#[derive(Clone, Copy, Debug, Default)]
pub struct ClickCounter {
    last: Option<(u64, i32, i32, u8)>,
}

impl ClickCounter {
    #[must_use]
    pub const fn new() -> Self {
        Self { last: None }
    }

    /// A press at `(x, y)` at `now_ns`: its click count (1..=[`CLICKS_MAX`]; past three it stays
    /// three, as a triple click held on).
    pub fn press(&mut self, now_ns: u64, x: i32, y: i32) -> u8 {
        let clicks = match self.last {
            Some((at, lx, ly, n))
                if now_ns.saturating_sub(at) <= DOUBLE_CLICK_NS
                    && (x - lx).abs() <= SLOP_PX
                    && (y - ly).abs() <= SLOP_PX =>
            {
                (n + 1).min(CLICKS_MAX)
            }
            _ => 1,
        };
        self.last = Some((now_ns, x, y, clicks));
        clicks
    }
}

/// The presses windowd routes: the counter and what the last press said (its tap frame carries
/// it — the click count and the modifiers held).
#[derive(Clone, Copy, Debug, Default)]
pub struct Presses {
    counter: ClickCounter,
    /// The last press's facts.
    pub facts: PointerFacts,
}

impl Presses {
    #[must_use]
    pub const fn new() -> Self {
        Self { counter: ClickCounter::new(), facts: PointerFacts { modifiers: 0, clicks: 0 } }
    }

    /// A press at `(x, y)` at `now_ns` with `modifiers` held: its facts, kept for its tap frame.
    pub fn press(&mut self, now_ns: u64, x: i32, y: i32, modifiers: u8) -> PointerFacts {
        self.facts = PointerFacts { modifiers, clicks: self.counter.press(now_ns, x, y) };
        self.facts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: u64 = 1_000_000;

    #[test]
    fn quick_presses_on_one_place_count_up_to_three() {
        let mut c = ClickCounter::new();
        assert_eq!(c.press(0, 100, 100), 1);
        assert_eq!(c.press(200 * MS, 101, 99), 2, "inside the slop and the interval");
        assert_eq!(c.press(400 * MS, 100, 100), 3);
        assert_eq!(c.press(600 * MS, 100, 100), 3, "a fourth stays a triple");
    }

    #[test]
    fn test_reject_a_count_across_the_interval_or_the_slop() {
        let mut c = ClickCounter::new();
        c.press(0, 100, 100);
        assert_eq!(c.press(DOUBLE_CLICK_NS + 1, 100, 100), 1, "too late: a first click");
        assert_eq!(c.press(DOUBLE_CLICK_NS + 2, 100 + SLOP_PX + 1, 100), 1, "too far");
        assert_eq!(c.press(DOUBLE_CLICK_NS + 3, 100 + SLOP_PX + 1, 100), 2);
    }
}
