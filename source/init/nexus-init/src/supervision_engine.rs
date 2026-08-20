// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The restart/backoff/crash-loop engine (TASK-0049B PR-B2 — the
//! ACTION half, RFC-0087 §2). Pure and host-tested: time is an injected
//! `now_ns`, the exit reason is ADR-0056 kernel truth, and every decision is
//! a value — the OS layer only spawns and prints. Rules: a `clean` exit
//! NEVER counts toward the crash window (RestartPolicy::OnFailure lets it
//! rest; Always respawns it without burning budget); failures push into a
//! bounded window and back off exponentially; `threshold` failures inside
//! `window_s` park the service (`Blocked`, terminal until an operator/
//! policy unblocks in a later PR).
//! OWNERS: @runtime @reliability
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: host unit tests below (schedule/window/cap/clean rules)
//! ADR: docs/adr/0057-service-restart-capability-re-resolve.md

use crate::service_supervision::{BackoffSpec, RestartPolicy};
use nexus_abi::ExitReason;

/// Bounded crash-history depth; the SSOT threshold must fit inside it
/// (compile-time-ish guard in `new`).
const MAX_WINDOW: usize = 8;

/// Engine state for one supervised child.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineState {
    /// Child is (assumed) alive.
    Running,
    /// Child exited cleanly under `OnFailure` — at rest, no restart due.
    Stopped,
    /// Restart scheduled at `due_ns` (backoff delay applied).
    WaitingRestart {
        /// Monotonic deadline after which the restart is due.
        due_ns: u64,
    },
    /// Crash-looped: parked for the boot (RFC-0087 `blocked`).
    Blocked,
}

/// The decision returned for an observed exit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineDecision {
    /// Nothing to do (clean exit at rest, or already blocked).
    Rest,
    /// Schedule a restart: fire when `now_ns >= due_ns`. `attempt` is
    /// 1-based within the current crash window (drives the marker).
    RestartAt {
        /// Monotonic deadline for the respawn.
        due_ns: u64,
        /// 1-based attempt number inside the crash window.
        attempt: u32,
    },
    /// The crash-loop cap fired — park the service and say so once.
    Blocked,
}

/// Restart/backoff/crash-loop state machine for ONE supervised child.
#[derive(Clone, Copy, Debug)]
pub struct SupervisedChild {
    /// Current lifecycle state.
    pub state: EngineState,
    restart: RestartPolicy,
    backoff: BackoffSpec,
    /// Monotonic timestamps of counted failures (bounded ring, newest last).
    window: [u64; MAX_WINDOW],
    /// Valid entries in `window`.
    window_len: u8,
}

impl SupervisedChild {
    /// New supervised child (assumed running). Thresholds beyond the ring
    /// depth clamp to it — a silent larger window would under-count.
    pub fn new(restart: RestartPolicy, backoff: BackoffSpec) -> Self {
        let mut b = backoff;
        if b.threshold as usize > MAX_WINDOW {
            b.threshold = MAX_WINDOW as u8;
        }
        Self {
            state: EngineState::Running,
            restart,
            backoff: b,
            window: [0; MAX_WINDOW],
            window_len: 0,
        }
    }

    /// Feed an observed exit (ADR-0056 reason + injected monotonic time).
    pub fn on_exit(&mut self, reason: ExitReason, code: i32, now_ns: u64) -> EngineDecision {
        if self.state == EngineState::Blocked {
            return EngineDecision::Rest;
        }
        let clean = reason == ExitReason::Clean && code == 0;
        if clean {
            return match self.restart {
                // Clean exits never burn backoff budget (RFC-0087 §2).
                RestartPolicy::Always => {
                    self.state = EngineState::WaitingRestart { due_ns: now_ns };
                    EngineDecision::RestartAt { due_ns: now_ns, attempt: 0 }
                }
                RestartPolicy::OnFailure | RestartPolicy::Never => {
                    self.state = EngineState::Stopped;
                    EngineDecision::Rest
                }
            };
        }
        if self.restart == RestartPolicy::Never {
            self.state = EngineState::Stopped;
            return EngineDecision::Rest;
        }
        // Prune failures older than the window, then count this one.
        let window_ns = (self.backoff.window_s as u64).saturating_mul(1_000_000_000);
        let cutoff = now_ns.saturating_sub(window_ns);
        let mut kept: [u64; MAX_WINDOW] = [0; MAX_WINDOW];
        let mut n = 0usize;
        for i in 0..self.window_len as usize {
            if self.window[i] >= cutoff {
                kept[n] = self.window[i];
                n += 1;
            }
        }
        if n < MAX_WINDOW {
            kept[n] = now_ns;
            n += 1;
        }
        self.window = kept;
        self.window_len = n as u8;
        if n as u8 >= self.backoff.threshold {
            self.state = EngineState::Blocked;
            return EngineDecision::Blocked;
        }
        // Attempt k (1-based) delays initial * factor^(k-1), clamped.
        let attempt = n as u32;
        let mut delay = self.backoff.initial_ms as u64;
        for _ in 1..attempt {
            delay = delay.saturating_mul(self.backoff.factor as u64);
            if delay >= self.backoff.max_ms as u64 {
                delay = self.backoff.max_ms as u64;
                break;
            }
        }
        let due_ns = now_ns.saturating_add(delay.saturating_mul(1_000_000));
        self.state = EngineState::WaitingRestart { due_ns };
        EngineDecision::RestartAt { due_ns, attempt }
    }

    /// `true` when a scheduled restart is due.
    pub fn restart_due(&self, now_ns: u64) -> bool {
        matches!(self.state, EngineState::WaitingRestart { due_ns } if now_ns >= due_ns)
    }

    /// The respawn happened — back to (assumed) running.
    pub fn on_restarted(&mut self) {
        if matches!(self.state, EngineState::WaitingRestart { .. }) {
            self.state = EngineState::Running;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service_supervision::DEFAULT_BACKOFF;

    const S: u64 = 1_000_000_000;
    const MS: u64 = 1_000_000;

    fn fault() -> ExitReason {
        ExitReason::Fault { cause: 13 }
    }

    #[test]
    fn backoff_schedule_matches_contract_and_clamps() {
        // Space failures 16s apart: window (60s) keeps them all, so the
        // attempt number climbs and the delays follow 500→1000→2000→4000.
        let mut c = SupervisedChild::new(RestartPolicy::OnFailure, DEFAULT_BACKOFF);
        let mut expect = [500u64, 1000, 2000, 4000];
        for (k, exp) in expect.iter_mut().enumerate() {
            let now = (k as u64) * 16 * S;
            match c.on_exit(fault(), -22, now) {
                EngineDecision::RestartAt { due_ns, attempt } => {
                    assert_eq!(attempt, k as u32 + 1);
                    assert_eq!(due_ns, now + *exp * MS, "attempt {}", k + 1);
                }
                other => panic!("expected restart, got {other:?}"),
            }
            c.on_restarted();
        }
        // 5th failure inside the window would block — use a spread-out 5th
        // instead (old ones pruned) to see the clamp at max_ms.
        let mut c = SupervisedChild::new(
            RestartPolicy::OnFailure,
            BackoffSpec { initial_ms: 500, factor: 2, max_ms: 1500, window_s: 60, threshold: 5 },
        );
        for k in 0..3 {
            let _ = c.on_exit(fault(), -22, (k as u64) * 16 * S);
            c.on_restarted();
        }
        match c.on_exit(fault(), -22, 3 * 16 * S) {
            EngineDecision::RestartAt { due_ns, attempt } => {
                assert_eq!(attempt, 4);
                // 500*2^3 = 4000 clamps to 1500.
                assert_eq!(due_ns, 3 * 16 * S + 1500 * MS);
            }
            other => panic!("expected clamped restart, got {other:?}"),
        }
    }

    #[test]
    fn crash_loop_inside_window_blocks_exactly_at_threshold() {
        let mut c = SupervisedChild::new(RestartPolicy::OnFailure, DEFAULT_BACKOFF);
        for k in 0..4u64 {
            match c.on_exit(fault(), -22, k * S) {
                EngineDecision::RestartAt { attempt, .. } => assert_eq!(attempt, k as u32 + 1),
                other => panic!("unexpected {other:?}"),
            }
            c.on_restarted();
        }
        assert_eq!(c.on_exit(fault(), -22, 4 * S), EngineDecision::Blocked);
        assert_eq!(c.state, EngineState::Blocked);
        // Terminal: further exits stay at rest.
        assert_eq!(c.on_exit(fault(), -22, 5 * S), EngineDecision::Rest);
    }

    #[test]
    fn old_failures_age_out_of_the_window() {
        let mut c = SupervisedChild::new(RestartPolicy::OnFailure, DEFAULT_BACKOFF);
        for k in 0..4u64 {
            let _ = c.on_exit(fault(), -22, k * S);
            c.on_restarted();
        }
        // 61s later the 4 old failures are outside the 60s window: this
        // failure counts as attempt 1 again, not as the blocking 5th.
        match c.on_exit(fault(), -22, 65 * S) {
            EngineDecision::RestartAt { attempt, due_ns } => {
                assert_eq!(attempt, 1);
                assert_eq!(due_ns, 65 * S + 500 * MS);
            }
            other => panic!("expected fresh window, got {other:?}"),
        }
    }

    #[test]
    fn clean_exits_never_count_and_respect_policy() {
        // OnFailure: clean exit rests, burns no budget.
        let mut c = SupervisedChild::new(RestartPolicy::OnFailure, DEFAULT_BACKOFF);
        assert_eq!(c.on_exit(ExitReason::Clean, 0, S), EngineDecision::Rest);
        assert_eq!(c.state, EngineState::Stopped);
        // Always: clean exit respawns immediately with attempt 0 (no budget).
        let mut c = SupervisedChild::new(RestartPolicy::Always, DEFAULT_BACKOFF);
        match c.on_exit(ExitReason::Clean, 0, S) {
            EngineDecision::RestartAt { due_ns, attempt } => {
                assert_eq!((due_ns, attempt), (S, 0));
            }
            other => panic!("expected immediate respawn, got {other:?}"),
        }
        // An `error` exit (voluntary non-zero) DOES count.
        let mut c = SupervisedChild::new(RestartPolicy::OnFailure, DEFAULT_BACKOFF);
        match c.on_exit(ExitReason::Error, 42, S) {
            EngineDecision::RestartAt { attempt, .. } => assert_eq!(attempt, 1),
            other => panic!("expected restart on error exit, got {other:?}"),
        }
        // Never: failures rest too.
        let mut c = SupervisedChild::new(RestartPolicy::Never, DEFAULT_BACKOFF);
        assert_eq!(c.on_exit(fault(), -22, S), EngineDecision::Rest);
        assert_eq!(c.state, EngineState::Stopped);
    }

    #[test]
    fn restart_due_and_ack_roundtrip() {
        let mut c = SupervisedChild::new(RestartPolicy::OnFailure, DEFAULT_BACKOFF);
        let _ = c.on_exit(fault(), -22, 0);
        assert!(!c.restart_due(499 * MS));
        assert!(c.restart_due(500 * MS));
        c.on_restarted();
        assert_eq!(c.state, EngineState::Running);
        assert!(!c.restart_due(u64::MAX));
    }
}
