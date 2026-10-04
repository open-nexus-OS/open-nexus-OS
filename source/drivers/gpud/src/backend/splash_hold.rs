// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0093 §5 ("the splash stays until `STATUS_REVEALED`") on a display that switches
//! what it scans — the board's display controller (TASK-0251 P2a step 2): the boot splash holds
//! the glass until windowd asked for the reveal (`OP_REVEAL`); the first present after the ask
//! switches the scan to the desktop, exactly once. A present before the ask is composed into the
//! framebuffer and never shown. A switch that did not take (its words read back otherwise) keeps
//! the splash for good: no reveal is acked for a desktop nobody can see, and nothing retries a
//! switch the hardware refused.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: below (host, `test_reject_*`); the board ladder (`gpud: dc reveal flip ok (`)

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
enum Scan {
    /// The splash holds the glass.
    #[default]
    Splash,
    /// The desktop: the switch took.
    Desktop,
    /// The switch did not take; the splash stays.
    Refused,
}

/// windowd's ask and what the scan shows.
#[derive(Default)]
pub(crate) struct SplashHold {
    asked: bool,
    scan: Scan,
}

impl SplashHold {
    /// windowd asked for the reveal.
    pub(crate) fn ask(&mut self) {
        self.asked = true;
    }

    pub(crate) fn asked(&self) -> bool {
        self.asked
    }

    /// Whether the splash still holds the glass.
    pub(crate) fn holding(&self) -> bool {
        self.scan != Scan::Desktop
    }

    /// Whether the present that just reached the framebuffer must switch the scan.
    pub(crate) fn switch_due(&self) -> bool {
        self.asked && self.scan == Scan::Splash
    }

    /// The switch took (`true`) or its words read back otherwise (`false`).
    pub(crate) fn switched(&mut self, took: bool) {
        self.scan = if took { Scan::Desktop } else { Scan::Refused };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Presents before windowd's ask are composed, never shown: no switch, the splash holds.
    #[test]
    fn test_reject_a_switch_before_the_reveal_ask() {
        let hold = SplashHold::default();
        assert!(!hold.switch_due());
        assert!(hold.holding() && !hold.asked());
    }

    /// The first present after the ask switches; none after it does — the reveal happens once,
    /// even when a restarted windowd asks again.
    #[test]
    fn the_first_present_after_the_ask_switches_exactly_once() {
        let mut hold = SplashHold::default();
        hold.ask();
        assert!(hold.holding(), "the ask alone shows nothing");
        assert!(hold.switch_due());
        hold.switched(true);
        assert!(!hold.holding());
        hold.ask();
        assert!(!hold.switch_due());
    }

    /// A switch whose words read back otherwise is no reveal: the splash stays, the reveal is
    /// never acked, and no later present retries it.
    #[test]
    fn test_reject_a_reveal_after_a_refused_switch() {
        let mut hold = SplashHold::default();
        hold.ask();
        hold.switched(false);
        assert!(hold.holding());
        assert!(!hold.switch_due());
    }
}
