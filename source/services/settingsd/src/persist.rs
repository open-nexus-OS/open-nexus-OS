// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0083 async persistence — the pure decision core behind
//! settingsd's statefsd writes. A SET commits in memory and replies
//! immediately; THIS decides when the (whole, latest) prefs blob is sent to
//! statefsd, coalescing rapid changes and backing off on failure. Pure and
//! host-tested: the os_lite loop injects the actual IPC send/recv.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: Unit tests below (coalescing, timeout, backoff, recovery).
//! RFC: docs/rfcs/RFC-0083-settings-distribution-v2-single-authority-versioned-snapshots.md

/// What the loop should do for persistence right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Nothing due — serve requests.
    None,
    /// Send the CURRENT prefs blob to statefsd (the latest state, not a
    /// queued historical one — that is the whole coalescing trick).
    SendPut,
}

/// The persistence state machine. Exactly one PUT is ever in flight; changes
/// arriving meanwhile only re-mark `dirty`, so N rapid SETs collapse into at
/// most one follow-up PUT carrying the final state.
pub struct Persister {
    /// The registry changed since the last PUT was SENT.
    dirty: bool,
    /// When the in-flight PUT was sent (`None` = nothing in flight).
    in_flight_since_ns: Option<u64>,
    /// Consecutive failures (send refused, reply timeout, non-OK status).
    failures: u8,
    /// Backoff gate: no PUT before this instant.
    not_before_ns: u64,
}

impl Persister {
    /// How long a sent PUT may wait for its reply before it counts as failed.
    /// statefsd serializes (policyd round-trip + journal write per request),
    /// so this is generous — but it is a RETRY trigger, not a client stall:
    /// nothing waits on it except this state machine.
    pub const REPLY_TIMEOUT_NS: u64 = 500_000_000;
    /// First-failure backoff; doubles per consecutive failure.
    const BACKOFF_BASE_NS: u64 = 250_000_000;
    /// Backoff ceiling — a broken statefsd is retried forever at this cadence
    /// (state stays live in memory; persistence heals when statefsd does).
    const BACKOFF_CAP_NS: u64 = 4_000_000_000;
    /// Minimum spacing between PUTs even on success. Settings need no
    /// sub-second durability, and statefsd pays a policyd round-trip plus a
    /// journal write PER request — without this floor, a burst of rapid SETs
    /// (the boot keymap selftest does five in 160 ms) turns into a burst of
    /// PUTs that starves statefsd's other clients (measured: the keystored
    /// probes started failing 2-of-3 boots). The floor turns the burst into
    /// one PUT plus one coalesced follow-up.
    pub(crate) const MIN_PUT_INTERVAL_NS: u64 = 500_000_000;

    #[must_use]
    pub const fn new() -> Self {
        Self { dirty: false, in_flight_since_ns: None, failures: 0, not_before_ns: 0 }
    }

    /// A validated value changed — the blob wants persisting. Safe to call
    /// any time, including while a PUT is in flight (that is the coalesce).
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// True when the loop may block indefinitely (no persistence work owed).
    #[must_use]
    pub fn is_idle(&self) -> bool {
        !self.dirty && self.in_flight_since_ns.is_none()
    }

    /// Advance time and report what to do. Expires a timed-out in-flight PUT
    /// (which re-arms `dirty` and starts backoff) before deciding.
    pub fn poll(&mut self, now_ns: u64) -> Action {
        if let Some(sent) = self.in_flight_since_ns {
            if now_ns.saturating_sub(sent) >= Self::REPLY_TIMEOUT_NS {
                self.fail(now_ns);
            } else {
                return Action::None; // one PUT in flight, wait for it
            }
        }
        if self.dirty && now_ns >= self.not_before_ns {
            return Action::SendPut;
        }
        Action::None
    }

    /// The PUT left the building; the blob it carried is the current state.
    pub fn on_put_sent(&mut self, now_ns: u64) {
        self.in_flight_since_ns = Some(now_ns);
        self.dirty = false;
    }

    /// The NONBLOCK send itself was refused (queue full / route missing).
    pub fn on_send_failed(&mut self, now_ns: u64) {
        self.fail(now_ns);
    }

    /// A statefsd reply arrived for the in-flight PUT.
    pub fn on_reply(&mut self, ok: bool, now_ns: u64) {
        self.in_flight_since_ns = None;
        if ok {
            self.failures = 0;
            // Success still spaces the NEXT PUT (`MIN_PUT_INTERVAL_NS`):
            // `dirty` may already be true again (a SET raced the reply), and
            // the follow-up carrying the newer blob should absorb further
            // rapid changes instead of chasing each one.
            self.not_before_ns = now_ns.saturating_add(Self::MIN_PUT_INTERVAL_NS);
        } else {
            self.fail(now_ns);
        }
    }

    /// Consecutive-failure count (for bounded markers).
    #[must_use]
    pub fn failures(&self) -> u8 {
        self.failures
    }

    fn fail(&mut self, now_ns: u64) {
        self.in_flight_since_ns = None;
        self.dirty = true; // whatever was in flight is NOT durable
        self.failures = self.failures.saturating_add(1);
        let shift = u32::from(self.failures.min(4)) - 1;
        let backoff = (Self::BACKOFF_BASE_NS << shift).min(Self::BACKOFF_CAP_NS);
        self.not_before_ns = now_ns.saturating_add(backoff);
    }
}

impl Default for Persister {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: u64 = 1_000_000;

    #[test]
    fn idle_until_dirty_then_sends_once() {
        let mut p = Persister::new();
        assert!(p.is_idle());
        assert_eq!(p.poll(0), Action::None);
        p.mark_dirty();
        assert!(!p.is_idle());
        assert_eq!(p.poll(1 * MS), Action::SendPut);
        p.on_put_sent(1 * MS);
        assert_eq!(p.poll(2 * MS), Action::None, "one PUT in flight, no second");
        p.on_reply(true, 3 * MS);
        assert!(p.is_idle(), "acked and clean");
    }

    /// The coalescing contract: N rapid SETs while a PUT is in flight produce
    /// exactly ONE follow-up PUT (carrying the latest blob), not N.
    #[test]
    fn rapid_sets_coalesce_into_one_followup_put() {
        let mut p = Persister::new();
        p.mark_dirty();
        assert_eq!(p.poll(0), Action::SendPut);
        p.on_put_sent(0);
        for _ in 0..10 {
            p.mark_dirty(); // ten changes race the in-flight PUT
        }
        assert_eq!(p.poll(1 * MS), Action::None, "still in flight");
        p.on_reply(true, 2 * MS);
        assert_eq!(p.poll(2 * MS), Action::None, "spaced: no PUT storm on statefsd");
        let after = 2 * MS + Persister::MIN_PUT_INTERVAL_NS;
        assert_eq!(p.poll(after), Action::SendPut, "ONE follow-up after the floor");
        p.on_put_sent(after);
        p.on_reply(true, after + MS);
        assert!(p.is_idle());
    }

    /// A SET during persist must never be blocked — the machine's whole
    /// reason to exist. `is_idle` false = the loop uses a bounded timeout
    /// wait, but the machine itself never asks anyone to wait on statefsd.
    #[test]
    fn a_reply_timeout_rearms_dirty_and_backs_off() {
        let mut p = Persister::new();
        p.mark_dirty();
        assert_eq!(p.poll(0), Action::SendPut);
        p.on_put_sent(0);
        // Reply never comes; past the timeout the PUT counts as failed.
        assert_eq!(p.poll(Persister::REPLY_TIMEOUT_NS + MS), Action::None, "backoff gates");
        assert_eq!(p.failures(), 1);
        // After the backoff the same (latest) blob goes again.
        let later = Persister::REPLY_TIMEOUT_NS + 251 * MS;
        assert_eq!(p.poll(later), Action::SendPut);
    }

    #[test]
    fn backoff_doubles_and_caps_and_recovery_resets() {
        let mut p = Persister::new();
        let mut now = 0u64;
        p.mark_dirty();
        for expect_failures in 1..=6u8 {
            assert_eq!(p.poll(now), Action::SendPut);
            p.on_put_sent(now);
            p.on_reply(false, now);
            assert_eq!(p.failures(), expect_failures);
            // Jump past any backoff (cap is 4s).
            now += 5_000 * MS;
        }
        // One success heals everything.
        assert_eq!(p.poll(now), Action::SendPut);
        p.on_put_sent(now);
        p.on_reply(true, now);
        assert_eq!(p.failures(), 0);
        assert!(p.is_idle());
    }

    #[test]
    fn send_refusal_is_a_failure_with_backoff() {
        let mut p = Persister::new();
        p.mark_dirty();
        assert_eq!(p.poll(0), Action::SendPut);
        p.on_send_failed(0);
        assert_eq!(p.poll(1 * MS), Action::None, "backoff after refused send");
        assert!(!p.is_idle(), "still owes a PUT");
        assert_eq!(p.poll(300 * MS), Action::SendPut);
    }
}
