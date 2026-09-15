// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Per-boot accounting of the IPC message path — what the
//! `KSELFTEST: ipc stats (…)` line reads (TASK-0054C P1, RFC-0096 Phase 1).
//! The first IPC numbers in the repo: how many messages were sent, how many
//! kernel heap allocations and payload copies they cost, how many receiver
//! wakes went through the runqueue (and how many of those needed a cross-hart
//! IPI). Written on the hot path with relaxed atomics (accounting, never
//! synchronization); reset by the selftest once bring-up completes so the
//! boot-end line judges the steady-state window (the `budgets.rs` two-window
//! pattern); drained once by `sched_telemetry` OP 4.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Unstable — diagnostic surface, the RFC-0096 budgets
//!                (`core/trap/budgets.rs`) are the contract, these are its inputs
//! TEST_COVERAGE: QEMU marker `KSELFTEST: ipc stats (` (headless/smp1/visible ladders)
//! ADR: docs/adr/0064-request-reply-one-trap-per-side-direct-handoff.md
//!
//! Zero-copy is the direction (RFC-0096): a control message should cost the
//! kernel no heap and exactly the two register-sized copies a cross-address-
//! space transfer needs, and bulk never travels through the kernel at all
//! (VMO). Today every non-empty message costs two allocations and three
//! payload copies (user → heap, heap → heap clone, heap → user) — this module
//! is how that claim became a number instead of a sentence.

use core::sync::atomic::{AtomicU64, Ordering};

/// Messages accepted by the router (`ipc_send_v1` success path).
static SENDS: AtomicU64 = AtomicU64::new(0);
/// Kernel heap allocations made for message payloads (the `Vec` the user
/// bytes land in, plus the clone `Message::new` takes per send attempt).
static PAYLOAD_ALLOCS: AtomicU64 = AtomicU64::new(0);
/// Payload copies performed by the kernel (any direction), and their bytes.
static PAYLOAD_COPIES: AtomicU64 = AtomicU64::new(0);
static PAYLOAD_COPY_BYTES: AtomicU64 = AtomicU64::new(0);
/// Receivers woken by a send (each one is a runqueue hop today — no direct
/// handoff exists before TASK-0054C P4, so every wake is a handoff miss).
static RECV_WAKES: AtomicU64 = AtomicU64::new(0);
/// Of those, wakes whose target lives on another hart (a resched IPI).
static WAKE_IPIS: AtomicU64 = AtomicU64::new(0);

/// One message accepted by the router.
#[inline]
pub fn record_send() {
    SENDS.fetch_add(1, Ordering::Relaxed);
}

/// One payload allocation of `bytes` (a zero-length payload allocates nothing
/// and is not counted).
#[inline]
pub fn record_payload_alloc(bytes: usize) {
    if bytes != 0 {
        PAYLOAD_ALLOCS.fetch_add(1, Ordering::Relaxed);
    }
}

/// One payload copy of `bytes` (zero-length copies are not a copy).
#[inline]
pub fn record_payload_copy(bytes: usize) {
    if bytes != 0 {
        PAYLOAD_COPIES.fetch_add(1, Ordering::Relaxed);
        PAYLOAD_COPY_BYTES.fetch_add(bytes as u64, Ordering::Relaxed);
    }
}

/// A receiver blocked on the target endpoint was woken by this send;
/// `cross_hart` = its home hart is not the sending hart (an IPI follows).
#[inline]
pub fn record_recv_wake(cross_hart: bool) {
    RECV_WAKES.fetch_add(1, Ordering::Relaxed);
    if cross_hart {
        WAKE_IPIS.fetch_add(1, Ordering::Relaxed);
    }
}

/// Snapshot for the boot-end line.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct IpcStats {
    pub sends: u64,
    pub payload_allocs: u64,
    pub payload_copies: u64,
    pub payload_copy_bytes: u64,
    pub recv_wakes: u64,
    pub wake_ipis: u64,
}

/// Read every counter (relaxed; a snapshot, not a barrier).
pub fn report() -> IpcStats {
    IpcStats {
        sends: SENDS.load(Ordering::Relaxed),
        payload_allocs: PAYLOAD_ALLOCS.load(Ordering::Relaxed),
        payload_copies: PAYLOAD_COPIES.load(Ordering::Relaxed),
        payload_copy_bytes: PAYLOAD_COPY_BYTES.load(Ordering::Relaxed),
        recv_wakes: RECV_WAKES.load(Ordering::Relaxed),
        wake_ipis: WAKE_IPIS.load(Ordering::Relaxed),
    }
}

/// Two-window measurement: called by the selftest once bring-up completes
/// (`sched_telemetry` OP 5, next to `budgets::reset()`), so the boot-end line
/// covers the steady-state ladder and not the 24-service exec burst.
pub fn reset() {
    for c in
        [&SENDS, &PAYLOAD_ALLOCS, &PAYLOAD_COPIES, &PAYLOAD_COPY_BYTES, &RECV_WAKES, &WAKE_IPIS]
    {
        c.store(0, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_record_and_reset() {
        reset();
        record_send();
        record_payload_alloc(12);
        record_payload_alloc(0);
        record_payload_copy(12);
        record_payload_copy(0);
        record_recv_wake(false);
        record_recv_wake(true);
        let s = report();
        assert_eq!(s.sends, 1);
        assert_eq!(s.payload_allocs, 1);
        assert_eq!(s.payload_copies, 1);
        assert_eq!(s.payload_copy_bytes, 12);
        assert_eq!(s.recv_wakes, 2);
        assert_eq!(s.wake_ipis, 1);
        reset();
        assert_eq!(report(), IpcStats::default());
    }
}
