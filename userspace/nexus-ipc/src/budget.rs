// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: The ONE wait primitive for request/response IPC (RFC-0093 §1, TASK-0324 P7).
//! A reply is WAITED for — the kernel wakes the waiter when the frame arrives — never
//! polled: every helper here blocks in the transport with the REMAINING budget of an
//! absolute deadline. The deadline is a liveness bound (how long a silent peer is still
//! presumed alive), never a scheduling guess. Before P7 this module was the opposite:
//! `retry_ipc_until` spun `NonBlocking` attempts with `yield_()` against a wall clock, so a
//! live peer that answered late looked exactly like a dead one under host load
//! (`SELFTEST: statefs enc roundtrip FAIL`, 2026-09-11) and every caller burned a core for
//! the wait. That helper and `Clock::yield_now` are deleted; `scripts/check-wait-not-poll.sh`
//! keeps the pattern out of the tree.
//!
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal (crate public, but intended for in-tree use)
//! TEST_COVERAGE: Unit tests (host) — incl. `test_reject_recv_past_deadline`,
//!   `test_reject_wouldblock_transport_is_not_waited_on`, `test_reject_foreign_frame_is_not_a_reply`

use core::time::Duration;

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
extern crate alloc;

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
use alloc::vec::Vec;
#[cfg(not(all(nexus_env = "os", feature = "os-lite")))]
use std::vec::Vec;

use crate::{Client, IpcError, Result, Wait};

/// Maximum number of nonce-mismatched replies tolerated while waiting for a correlated response.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NonceMismatchBudget(u32);

impl NonceMismatchBudget {
    /// Creates a nonce-mismatch budget.
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    /// Returns the raw budget value.
    pub const fn raw(self) -> u32 {
        self.0
    }
}

/// Outcome of a bounded routing attempt.
#[must_use = "routing outcomes must be handled explicitly"]
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteRetryOutcome {
    /// Route resolved successfully.
    Success {
        /// Send slot returned by samgrd routing.
        send_slot: u32,
        /// Receive slot returned by samgrd routing.
        recv_slot: u32,
    },
    /// Route operation timed out under budget.
    Timeout,
    /// ADR-0057: the target service is registered but currently DEAD (the
    /// supervisor marked it stale). The whole budget was spent retrying —
    /// the restarted instance did not come back inside the deadline. Callers
    /// treat this like Timeout for flow control but keep the identity for
    /// diagnosis (ADR-0054: never collapse it into Rejected).
    TargetStale,
    /// Too many nonce mismatches were observed.
    NonceMismatchBudgetExceeded,
    /// The responder answered with a non-OK route status (`nexus_abi::routing::STATUS_*`),
    /// or the ask/answer was malformed (`STATUS_MALFORMED`). Carried so a status relay
    /// (bundlemgrd's `OP_ROUTE_STATUS`) needs no private copy of the exchange.
    Rejected {
        /// The route status the responder returned.
        status: u8,
    },
    /// Low-level IPC/runtime failure.
    Ipc(IpcError),
}

/// Clock source deadlines are computed against. It only tells the time: a wait is
/// event-driven (the transport blocks until the frame arrives or the deadline passes),
/// so there is nothing to yield for.
pub trait Clock {
    /// Returns the current time in nanoseconds, or `None` if not available.
    fn now_ns(&self) -> Option<u64>;
}

/// OS clock backed by `nexus_abi::nsec()` — the same clock the kernel checks IPC
/// deadlines against.
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
pub struct OsClock;

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
impl Clock for OsClock {
    fn now_ns(&self) -> Option<u64> {
        nexus_abi::nsec().ok()
    }
}

/// Host clock backed by `std::time::Instant`.
#[cfg(nexus_env = "host")]
pub struct HostClock {
    start: std::time::Instant,
}

#[cfg(nexus_env = "host")]
impl HostClock {
    /// Creates a new host clock.
    pub fn new() -> Self {
        Self { start: std::time::Instant::now() }
    }
}

#[cfg(nexus_env = "host")]
impl Default for HostClock {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(nexus_env = "host")]
impl Clock for HostClock {
    fn now_ns(&self) -> Option<u64> {
        let elapsed = self.start.elapsed();
        Some(
            elapsed
                .as_secs()
                .saturating_mul(1_000_000_000)
                .saturating_add(elapsed.subsec_nanos() as u64),
        )
    }
}

fn duration_to_ns(d: Duration) -> u64 {
    d.as_secs().saturating_mul(1_000_000_000).saturating_add(d.subsec_nanos() as u64)
}

/// Computes a deadline timestamp based on `clock.now_ns() + budget`.
pub fn deadline_after(clock: &impl Clock, budget: Duration) -> Result<u64> {
    let now = clock.now_ns().ok_or(IpcError::Unsupported)?;
    Ok(now.saturating_add(duration_to_ns(budget)))
}

/// The budget still left before `deadline_ns` — the amount a transport may block for.
/// `Err(Timeout)` once the deadline has passed, so a wait never starts with a zero budget
/// (which the kernel would read as "no deadline").
pub fn remaining(clock: &impl Clock, deadline_ns: u64) -> Result<Duration> {
    let now = clock.now_ns().ok_or(IpcError::Unsupported)?;
    if now >= deadline_ns {
        return Err(IpcError::Timeout);
    }
    Ok(Duration::from_nanos(deadline_ns - now))
}

/// Sends `frame` on `client`, blocking in the transport until it is queued or `deadline_ns`
/// passes. A transport that reports `Timeout` before the clock says so is re-entered with
/// the remaining budget; every other error is the caller's.
pub fn send_until(
    clock: &impl Clock,
    client: &impl Client,
    frame: &[u8],
    deadline_ns: u64,
) -> Result<()> {
    loop {
        let left = remaining(clock, deadline_ns)?;
        match client.send(frame, Wait::Timeout(left)) {
            Err(IpcError::Timeout) => continue,
            other => return other,
        }
    }
}

/// Receives ONE frame from `client`, blocking in the transport until it arrives or
/// `deadline_ns` passes. `WouldBlock` is surfaced as-is: it means the transport did not
/// wait, and nothing here ever retries a non-waiting transport (that would be the poll).
pub fn recv_until(clock: &impl Clock, client: &impl Client, deadline_ns: u64) -> Result<Vec<u8>> {
    loop {
        let left = remaining(clock, deadline_ns)?;
        match client.recv(Wait::Timeout(left)) {
            Err(IpcError::Timeout) => continue,
            other => return other,
        }
    }
}

/// Waits for the frame `accept` recognises (its nonce, its magic — the caller's
/// correlation rule), dropping every other frame that arrives on the shared inbox
/// meanwhile. Each wait blocks; the deadline is the liveness bound.
pub fn recv_matching_until<T>(
    clock: &impl Clock,
    client: &impl Client,
    deadline_ns: u64,
    mut accept: impl FnMut(&[u8]) -> Option<T>,
) -> Result<T> {
    loop {
        let frame = recv_until(clock, client, deadline_ns)?;
        if let Some(v) = accept(&frame) {
            return Ok(v);
        }
    }
}

/// [`send_until`] with a relative budget.
pub fn send_budgeted(
    clock: &impl Clock,
    client: &impl Client,
    frame: &[u8],
    budget: Duration,
) -> Result<()> {
    let deadline_ns = deadline_after(clock, budget)?;
    send_until(clock, client, frame, deadline_ns)
}

/// [`recv_until`] with a relative budget.
pub fn recv_budgeted(
    clock: &impl Clock,
    client: &impl Client,
    budget: Duration,
) -> Result<Vec<u8>> {
    let deadline_ns = deadline_after(clock, budget)?;
    recv_until(clock, client, deadline_ns)
}

/// Resolves a service route using routing v1+nonce with a deterministic deadline and mismatch cap.
///
/// The ask travels on the fleet control channel init installs in every child
/// (`nexus_service_topology::CTRL_SLOTS`) — callers no longer restate its slots (TASK-0324 P4f-6).
/// This helper is intended for os-lite bring-up services that still perform direct control-channel
/// routing calls and need bounded behavior under queue contention.
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
pub fn route_with_nonce_budgeted(
    name: &[u8],
    budget: Duration,
    mismatch_budget: NonceMismatchBudget,
) -> RouteRetryOutcome {
    use core::sync::atomic::{AtomicU32, Ordering};

    if name.is_empty() || name.len() > nexus_abi::routing::MAX_SERVICE_NAME_LEN {
        return RouteRetryOutcome::Rejected { status: nexus_abi::routing::STATUS_MALFORMED };
    }

    static ROUTE_NONCE: AtomicU32 = AtomicU32::new(1);
    let nonce = ROUTE_NONCE.fetch_add(1, Ordering::Relaxed);

    let mut req = [0u8; 5 + nexus_abi::routing::MAX_SERVICE_NAME_LEN + 4];
    let base_len = match nexus_abi::routing::encode_route_get(name, &mut req[..5 + name.len()]) {
        Some(v) => v,
        None => {
            return RouteRetryOutcome::Rejected { status: nexus_abi::routing::STATUS_MALFORMED }
        }
    };
    req[base_len..base_len + 4].copy_from_slice(&nonce.to_le_bytes());
    let req_len = base_len + 4;
    let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, req_len as u32);

    let clock = OsClock;
    let deadline_ns = match deadline_after(&clock, budget) {
        Ok(v) => v,
        Err(e) => return RouteRetryOutcome::Ipc(e),
    };

    let ctrl = nexus_service_topology::CTRL_SLOTS;
    if let Err(e) = raw::send_budgeted(ctrl.send, &hdr, &req[..req_len], deadline_ns) {
        return if e == IpcError::Timeout {
            RouteRetryOutcome::Timeout
        } else {
            RouteRetryOutcome::Ipc(e)
        };
    }

    let mut mismatches: u32 = 0;
    loop {
        // Every iteration WAITS in the kernel for the next control frame (or the deadline);
        // a frame that is not our answer is dropped and the wait resumes.
        let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 32];
        let n = match raw::recv_budgeted(ctrl.recv, &mut rh, &mut buf, deadline_ns) {
            Ok(v) => core::cmp::min(v, buf.len()),
            // No answer inside the budget. Since RFC-0093 §1 the ask may be PARKED in
            // init (a restarting target); the caller retries on its own cadence with a
            // fresh nonce, which replaces the parked ask.
            Err(IpcError::Timeout) => return RouteRetryOutcome::Timeout,
            Err(e) => return RouteRetryOutcome::Ipc(e),
        };

        if n != 17 {
            continue;
        }
        let Some((status, send_slot, recv_slot)) = nexus_abi::routing::decode_route_rsp(&buf[..13])
        else {
            continue;
        };
        let got_nonce = u32::from_le_bytes([buf[13], buf[14], buf[15], buf[16]]);
        if got_nonce != nonce {
            mismatches = mismatches.saturating_add(1);
            if mismatches > mismatch_budget.raw() {
                return RouteRetryOutcome::NonceMismatchBudgetExceeded;
            }
            continue;
        }

        if status == nexus_abi::routing::STATUS_OK {
            return RouteRetryOutcome::Success { send_slot, recv_slot };
        }
        if status == nexus_abi::routing::STATUS_STALE {
            // RFC-0093 §1 (TASK-0324 P3): STALE is TERMINAL here. A dead-but-supervised
            // target is not answered at all any more — init PARKS the ask and answers it
            // once the supervisor re-provisions the route. The re-ask loop that used to
            // live here is exactly the client-side polling routing v2 deletes; a STALE that
            // still arrives means init could not park (overflow, loud on its side), and the
            // caller decides — never a hammering loop.
            return RouteRetryOutcome::TargetStale;
        }
        return RouteRetryOutcome::Rejected { status };
    }
}

/// Low-level helpers for kernel IPC v1 syscalls (slot + `MsgHeader`): blocking with an
/// ABSOLUTE deadline the kernel checks against the same clock as [`OsClock`]. OS-lite only,
/// no allocations. A zero deadline is refused — the kernel reads it as "wait forever".
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
pub mod raw {
    use crate::{IpcError, Result};

    fn map(err: nexus_abi::IpcError) -> IpcError {
        match err {
            nexus_abi::IpcError::TimedOut => IpcError::Timeout,
            nexus_abi::IpcError::QueueFull | nexus_abi::IpcError::QueueEmpty => {
                IpcError::WouldBlock
            }
            e => IpcError::Kernel(e),
        }
    }

    /// Blocks until `bytes` is queued on `send_slot` or `deadline_ns` (absolute) passes.
    pub fn send_budgeted(
        send_slot: u32,
        hdr: &nexus_abi::MsgHeader,
        bytes: &[u8],
        deadline_ns: u64,
    ) -> Result<()> {
        if deadline_ns == 0 {
            return Err(IpcError::Unsupported);
        }
        nexus_abi::ipc_send_v1(send_slot, hdr, bytes, 0, deadline_ns).map(|_| ()).map_err(map)
    }

    /// Blocks until a frame arrives on `recv_slot` or `deadline_ns` (absolute) passes.
    /// Returns the number of bytes written to `out` (truncating longer frames).
    pub fn recv_budgeted(
        recv_slot: u32,
        hdr_out: &mut nexus_abi::MsgHeader,
        out: &mut [u8],
        deadline_ns: u64,
    ) -> Result<usize> {
        if deadline_ns == 0 {
            return Err(IpcError::Unsupported);
        }
        nexus_abi::ipc_recv_v1(recv_slot, hdr_out, out, nexus_abi::IPC_SYS_TRUNCATE, deadline_ns)
            .map(|n| n as usize)
            .map_err(map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::cell::Cell;

    /// A clock that advances by `step_ns` on every reading — time passes only when the
    /// helper looks, so every test is deterministic.
    #[derive(Default)]
    struct TestClock {
        now: Cell<u64>,
        step_ns: u64,
    }

    impl Clock for TestClock {
        fn now_ns(&self) -> Option<u64> {
            let now = self.now.get();
            self.now.set(now.saturating_add(self.step_ns));
            Some(now)
        }
    }

    /// A blocking transport that answers with a scripted sequence of outcomes and records
    /// the wait it was handed each time.
    #[derive(Default)]
    struct TestClient {
        script: Cell<usize>,
        outcomes: Vec<Result<Vec<u8>>>,
        waits: std::cell::RefCell<Vec<Wait>>,
    }

    impl TestClient {
        fn scripted(outcomes: Vec<Result<Vec<u8>>>) -> Self {
            Self { outcomes, ..Default::default() }
        }
        fn next(&self, wait: Wait) -> Result<Vec<u8>> {
            self.waits.borrow_mut().push(wait);
            let i = self.script.get();
            self.script.set(i + 1);
            self.outcomes.get(i).cloned().unwrap_or(Err(IpcError::Timeout))
        }
        fn calls(&self) -> usize {
            self.script.get()
        }
    }

    impl Client for TestClient {
        fn send(&self, _frame: &[u8], wait: Wait) -> Result<()> {
            self.next(wait).map(|_| ())
        }

        fn recv(&self, wait: Wait) -> Result<Vec<u8>> {
            self.next(wait)
        }
    }

    #[test]
    fn recv_until_hands_the_remaining_budget_to_the_transport() {
        let clock = TestClock { now: Cell::new(2_000_000), step_ns: 0 };
        let client = TestClient::scripted(vec![Ok(vec![7])]);
        let got = recv_until(&clock, &client, 5_000_000).unwrap();
        assert_eq!(got, vec![7]);
        assert_eq!(client.waits.borrow()[0], Wait::Timeout(Duration::from_nanos(3_000_000)));
    }

    #[test]
    fn recv_until_re_enters_after_an_early_transport_timeout() {
        // The transport may wake early; the CLOCK decides when the wait is over.
        let clock = TestClock { now: Cell::new(0), step_ns: 1_000 };
        let client = TestClient::scripted(vec![
            Err(IpcError::Timeout),
            Err(IpcError::Timeout),
            Ok(vec![1, 2, 3]),
        ]);
        let got = recv_until(&clock, &client, 1_000_000).unwrap();
        assert_eq!(got, vec![1, 2, 3]);
        assert_eq!(client.calls(), 3);
    }

    #[test]
    fn test_reject_recv_past_deadline() {
        // Deadline already passed: Timeout WITHOUT touching the transport (a zero budget
        // would read as "wait forever" in the kernel).
        let clock = TestClock { now: Cell::new(5), step_ns: 0 };
        let client = TestClient::scripted(vec![Ok(vec![9])]);
        assert_eq!(recv_until(&clock, &client, 5).unwrap_err(), IpcError::Timeout);
        assert_eq!(client.calls(), 0);
    }

    #[test]
    fn test_reject_wouldblock_transport_is_not_waited_on() {
        // A transport that did not wait is an error, never a reason to spin.
        let clock = TestClock { now: Cell::new(0), step_ns: 0 };
        let client = TestClient::scripted(vec![Err(IpcError::WouldBlock), Ok(vec![1])]);
        assert_eq!(recv_until(&clock, &client, 1_000).unwrap_err(), IpcError::WouldBlock);
        assert_eq!(client.calls(), 1);
        let sender = TestClient::scripted(vec![Err(IpcError::WouldBlock), Ok(vec![])]);
        assert_eq!(send_until(&clock, &sender, b"x", 1_000).unwrap_err(), IpcError::WouldBlock);
        assert_eq!(sender.calls(), 1);
    }

    #[test]
    fn send_until_waits_and_times_out_on_the_clock() {
        let clock = TestClock { now: Cell::new(0), step_ns: 500 };
        let client = TestClient::scripted(vec![Err(IpcError::Timeout), Ok(vec![])]);
        send_until(&clock, &client, b"hi", 1_000).unwrap();
        assert_eq!(client.calls(), 2);
        // Two clock readings so far (500 each): the next wait starts AT the deadline.
        let late = TestClient::scripted(vec![Ok(vec![])]);
        assert_eq!(send_until(&clock, &late, b"hi", 1_000).unwrap_err(), IpcError::Timeout);
        assert_eq!(late.calls(), 0);
    }

    #[test]
    fn recv_matching_until_skips_foreign_frames() {
        let clock = TestClock { now: Cell::new(0), step_ns: 1 };
        let client = TestClient::scripted(vec![Ok(vec![0xAA]), Ok(vec![0xBB]), Ok(vec![0x42])]);
        let got = recv_matching_until(&clock, &client, 1_000, |f| (f[0] == 0x42).then_some(f[0]));
        assert_eq!(got.unwrap(), 0x42);
        assert_eq!(client.calls(), 3);
    }

    #[test]
    fn test_reject_foreign_frame_is_not_a_reply() {
        // Only foreign frames arrive: the wait ends on the deadline, never on a frame that
        // fails the correlation rule.
        let clock = TestClock { now: Cell::new(0), step_ns: 400 };
        let foreign = Ok(vec![0xAA]);
        let client = TestClient::scripted(vec![foreign; 8]);
        let got = recv_matching_until(&clock, &client, 1_000, |f| (f[0] == 0x42).then_some(()));
        assert_eq!(got.unwrap_err(), IpcError::Timeout);
        // Readings at 0 / 400 / 800 admit three waits; the fourth is past the deadline.
        assert_eq!(client.calls(), 3, "must stop on the deadline, not on script exhaustion");
    }

    #[test]
    fn budgeted_forms_derive_the_deadline_from_the_clock() {
        let clock = TestClock { now: Cell::new(10_000_000), step_ns: 0 };
        let client = TestClient::scripted(vec![Ok(vec![]), Ok(vec![5])]);
        send_budgeted(&clock, &client, b"hi", Duration::from_millis(5)).unwrap();
        let rsp = recv_budgeted(&clock, &client, Duration::from_millis(5)).unwrap();
        assert_eq!(rsp, vec![5]);
        let waits = client.waits.borrow();
        assert_eq!(waits[0], Wait::Timeout(Duration::from_millis(5)));
        assert_eq!(waits[1], Wait::Timeout(Duration::from_millis(5)));
    }
}
