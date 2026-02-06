// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Deterministic, budgeted retry loops for IPC operations.
//!
//! This module exists to avoid relying on kernel timeout semantics in OS-lite builds.
//! Callers should use non-blocking IPC attempts (returning `IpcError::WouldBlock`) and
//! apply an explicit time budget based on `nsec()` / a host clock.
//!
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal (crate public, but intended for in-tree use)
//! TEST_COVERAGE: Unit tests (host)

use core::time::Duration;

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
extern crate alloc;

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
use alloc::vec::Vec;
#[cfg(not(all(nexus_env = "os", feature = "os-lite")))]
use std::vec::Vec;

use crate::{Client, IpcError, Result, Wait};

const SPIN_CHECK_MASK: usize = 0x7f; // check time every 128 spins

/// Clock source used for budgeted loops.
pub trait Clock {
    /// Returns the current time in nanoseconds, or `None` if not available.
    fn now_ns(&self) -> Option<u64>;
    /// Cooperative yield to allow other work to make progress.
    fn yield_now(&self);
}

/// OS clock backed by `nexus_abi::nsec()` + `nexus_abi::yield_()`.
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
pub struct OsClock;

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
impl Clock for OsClock {
    fn now_ns(&self) -> Option<u64> {
        nexus_abi::nsec().ok()
    }

    fn yield_now(&self) {
        let _ = nexus_abi::yield_();
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

    fn yield_now(&self) {
        std::thread::yield_now();
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

/// Runs `op` until it succeeds, fails with a non-retryable error, or the deadline expires.
///
/// Retryable condition is `IpcError::WouldBlock` (mapped from queue empty/full in non-blocking
/// syscalls / transports).
pub fn retry_ipc_until<T>(
    clock: &impl Clock,
    deadline_ns: u64,
    mut op: impl FnMut() -> Result<T>,
) -> Result<T> {
    let mut spins: usize = 0;
    loop {
        match op() {
            Ok(v) => return Ok(v),
            Err(IpcError::WouldBlock) => {
                if (spins & SPIN_CHECK_MASK) == 0 {
                    let now = clock.now_ns().ok_or(IpcError::Unsupported)?;
                    if now >= deadline_ns {
                        return Err(IpcError::Timeout);
                    }
                }
                clock.yield_now();
            }
            Err(e) => return Err(e),
        }
        spins = spins.wrapping_add(1);
    }
}

/// Runs `op` until it succeeds, fails with a non-retryable error, or the budget expires.
pub fn retry_ipc_budgeted<T>(
    clock: &impl Clock,
    budget: Duration,
    op: impl FnMut() -> Result<T>,
) -> Result<T> {
    let deadline_ns = deadline_after(clock, budget)?;
    retry_ipc_until(clock, deadline_ns, op)
}

/// Sends `frame` on `client` using non-blocking attempts and an explicit time budget.
pub fn send_budgeted(
    clock: &impl Clock,
    client: &impl Client,
    frame: &[u8],
    budget: Duration,
) -> Result<()> {
    let deadline_ns = deadline_after(clock, budget)?;
    send_until(clock, client, frame, deadline_ns)
}

/// Receives a frame from `client` using non-blocking attempts and an explicit time budget.
pub fn recv_budgeted(
    clock: &impl Clock,
    client: &impl Client,
    budget: Duration,
) -> Result<Vec<u8>> {
    let deadline_ns = deadline_after(clock, budget)?;
    recv_until(clock, client, deadline_ns)
}

/// Sends `frame` on `client` using non-blocking attempts until `deadline_ns`.
pub fn send_until(
    clock: &impl Clock,
    client: &impl Client,
    frame: &[u8],
    deadline_ns: u64,
) -> Result<()> {
    retry_ipc_until(clock, deadline_ns, || client.send(frame, Wait::NonBlocking))
}

/// Receives a frame from `client` using non-blocking attempts until `deadline_ns`.
pub fn recv_until(clock: &impl Clock, client: &impl Client, deadline_ns: u64) -> Result<Vec<u8>> {
    retry_ipc_until(clock, deadline_ns, || client.recv(Wait::NonBlocking))
}

/// Low-level helpers for kernel IPC v1 syscalls (slot + `MsgHeader`).
///
/// These are OS-lite only and avoid allocations.
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
pub mod raw {
    use super::{retry_ipc_until, Clock};
    use crate::{IpcError, Result};

    /// Sends `bytes` via kernel IPC v1 using non-blocking attempts until `deadline_ns`.
    pub fn send_budgeted(
        clock: &impl Clock,
        send_slot: u32,
        hdr: &nexus_abi::MsgHeader,
        bytes: &[u8],
        deadline_ns: u64,
    ) -> Result<()> {
        retry_ipc_until(clock, deadline_ns, || {
            match nexus_abi::ipc_send_v1(
                send_slot,
                hdr,
                bytes,
                nexus_abi::IPC_SYS_NONBLOCK,
                0,
            ) {
                Ok(_) => Ok(()),
                Err(nexus_abi::IpcError::QueueFull) => Err(IpcError::WouldBlock),
                Err(e) => Err(IpcError::Kernel(e)),
            }
        })
    }

    /// Receives bytes via kernel IPC v1 using non-blocking attempts until `deadline_ns`.
    ///
    /// Returns the number of bytes written to `out`.
    pub fn recv_budgeted(
        clock: &impl Clock,
        recv_slot: u32,
        hdr_out: &mut nexus_abi::MsgHeader,
        out: &mut [u8],
        deadline_ns: u64,
    ) -> Result<usize> {
        retry_ipc_until(clock, deadline_ns, || {
            match nexus_abi::ipc_recv_v1(
                recv_slot,
                hdr_out,
                out,
                nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
                0,
            ) {
                Ok(n) => Ok(n as usize),
                Err(nexus_abi::IpcError::QueueEmpty) => Err(IpcError::WouldBlock),
                Err(e) => Err(IpcError::Kernel(e)),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::cell::Cell;

    #[derive(Default)]
    struct TestClock {
        now: Cell<u64>,
        now_calls: Cell<u64>,
        yield_calls: Cell<u64>,
        advance_per_yield_ns: u64,
    }

    impl Clock for TestClock {
        fn now_ns(&self) -> Option<u64> {
            self.now_calls.set(self.now_calls.get().saturating_add(1));
            Some(self.now.get())
        }

        fn yield_now(&self) {
            // Deterministic: advance the synthetic clock without sleeping.
            self.yield_calls.set(self.yield_calls.get().saturating_add(1));
            self.now.set(self.now.get().saturating_add(self.advance_per_yield_ns));
        }
    }

    #[derive(Default)]
    struct TestClient {
        send_calls: Cell<u32>,
        recv_calls: Cell<u32>,
        wouldblock_before_ok: u32,
    }

    impl Client for TestClient {
        fn send(&self, _frame: &[u8], _wait: Wait) -> Result<()> {
            let calls = self.send_calls.get().saturating_add(1);
            self.send_calls.set(calls);
            if calls <= self.wouldblock_before_ok {
                Err(IpcError::WouldBlock)
            } else {
                Ok(())
            }
        }

        fn recv(&self, _wait: Wait) -> Result<Vec<u8>> {
            let calls = self.recv_calls.get().saturating_add(1);
            self.recv_calls.set(calls);
            if calls <= self.wouldblock_before_ok {
                Err(IpcError::WouldBlock)
            } else {
                Ok(vec![1, 2, 3])
            }
        }
    }

    #[test]
    fn retry_succeeds_after_wouldblock() {
        let clock = TestClock { advance_per_yield_ns: 1_000_000, ..Default::default() };
        let mut attempts = 0u32;
        let v = retry_ipc_budgeted(&clock, Duration::from_millis(10), || {
            attempts += 1;
            if attempts < 5 {
                Err(IpcError::WouldBlock)
            } else {
                Ok(42u32)
            }
        })
        .unwrap();
        assert_eq!(v, 42);
        assert!(clock.yield_calls.get() >= 4);
    }

    #[test]
    fn retry_times_out_deterministically() {
        let clock = TestClock { advance_per_yield_ns: 1_000_000, ..Default::default() };
        let err = retry_ipc_budgeted(&clock, Duration::from_millis(3), || -> Result<()> {
            Err(IpcError::WouldBlock)
        })
        .unwrap_err();
        assert_eq!(err, IpcError::Timeout);
        assert!(clock.yield_calls.get() > 0);
    }

    #[test]
    fn deadline_check_is_periodic_not_per_spin() {
        // If the operation succeeds quickly, we should not consult the clock on every spin.
        let clock = TestClock { advance_per_yield_ns: 0, ..Default::default() };
        let deadline = 123;
        let mut attempts = 0usize;
        let _ = retry_ipc_until(&clock, deadline, || {
            attempts += 1;
            if attempts < 300 {
                Err(IpcError::WouldBlock)
            } else {
                Ok(())
            }
        })
        .unwrap();
        // now_ns is called once per 128 spins (plus a small constant). 300 spins -> ~3 calls.
        assert!(clock.now_calls.get() <= 6, "now_ns called too often: {}", clock.now_calls.get());
    }

    #[test]
    fn send_and_recv_budgeted_spin_until_progress() {
        let clock = TestClock { advance_per_yield_ns: 1, ..Default::default() };
        let client = TestClient { wouldblock_before_ok: 4, ..Default::default() };

        send_budgeted(&clock, &client, b"hi", Duration::from_millis(5)).unwrap();
        let rsp = recv_budgeted(&clock, &client, Duration::from_millis(5)).unwrap();
        assert_eq!(rsp, vec![1, 2, 3]);
        assert!(clock.yield_calls.get() >= 8);
    }

    #[test]
    fn send_budgeted_times_out() {
        let clock = TestClock { advance_per_yield_ns: 1_000_000, ..Default::default() };
        let client = TestClient { wouldblock_before_ok: u32::MAX, ..Default::default() };

        let err = send_budgeted(&clock, &client, b"hi", Duration::from_millis(2)).unwrap_err();
        assert_eq!(err, IpcError::Timeout);
        assert!(clock.yield_calls.get() > 0);
    }
}
