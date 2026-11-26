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
/// The same sends, by sending hart (a report's width, not a hart limit: harts past it share
/// the last slot).
static SENDS_BY_HART: [AtomicU64; HART_SLOTS] = [const { AtomicU64::new(0) }; HART_SLOTS];
/// Slots of the per-hart report.
pub const HART_SLOTS: usize = 8;
/// Kernel heap allocations made for message payloads. ONE per message since
/// TASK-0054C P3a: the `Vec` the user bytes land in. The second — a clone taken
/// per send attempt "in case the attempt fails" — was redundant, because
/// `Router::send_returning_message` already hands the `Message` back on error.
static PAYLOAD_ALLOCS: AtomicU64 = AtomicU64::new(0);
/// Payload copies performed by the kernel (any direction), and their bytes.
static PAYLOAD_COPIES: AtomicU64 = AtomicU64::new(0);
static PAYLOAD_COPY_BYTES: AtomicU64 = AtomicU64::new(0);
/// Receivers woken by a send (each one is a runqueue hop today — no direct
/// handoff exists before TASK-0054C P4, so every wake is a handoff miss).
static RECV_WAKES: AtomicU64 = AtomicU64::new(0);
/// Of those, wakes whose target lives on another hart (a resched IPI).
static WAKE_IPIS: AtomicU64 = AtomicU64::new(0);
/// Payload-size histogram over accepted user sends, one bucket per candidate
/// inline-tier size (TASK-0054C P3a). RFC-0096 named `IPC_SHORT_MAX = 64` before
/// anything measured the distribution, and an average cannot choose it: many
/// 8-byte frames and a few 4 KiB ones average the same as all-44-byte ones.
/// This says what fraction a given tier would actually cover.
/// Buckets: 0 | 1..=32 | 33..=64 | 65..=128 | 129..=256 | 257..=512 | 513..=1024 | > 1024.
static PAYLOAD_HIST: [AtomicU64; PAYLOAD_BUCKETS] = [const { AtomicU64::new(0) }; PAYLOAD_BUCKETS];
/// Number of histogram buckets.
pub const PAYLOAD_BUCKETS: usize = 8;
/// `ipc_call`s finished in the caller's registers (TASK-0054C P4a): the
/// fastpath, where the caller never re-enters the kernel to collect its
/// reply. Counted because "the fastpath exists" and "the fastpath is taken"
/// are different claims, and only the second one is worth anything.
static CALLS_IN_REGS: AtomicU64 = AtomicU64::new(0);

/// One `ipc_call` completed in registers.
#[inline]
pub fn record_call_in_regs() {
    CALLS_IN_REGS.fetch_add(1, Ordering::Relaxed);
}

/// The bucket a payload of `bytes` falls into (see [`PAYLOAD_HIST`]).
#[inline]
const fn bucket_of(bytes: usize) -> usize {
    match bytes {
        0 => 0,
        1..=32 => 1,
        33..=64 => 2,
        65..=128 => 3,
        129..=256 => 4,
        257..=512 => 5,
        513..=1024 => 6,
        _ => 7,
    }
}

/// Records the size of ONE user payload where the kernel COPIES IT IN — the same
/// point [`record_payload_alloc`] counts, so the histogram total always equals
/// `heap_allocs` exactly. Both exceed [`SENDS`] by the messages that fail to
/// enqueue after copy-in (0 in the standard boot profiles, 11 per boot in the
/// OTA bundle lanes) — that difference is the failed-send count, for free.
#[inline]
pub fn record_payload_size(bytes: usize) {
    PAYLOAD_HIST[bucket_of(bytes)].fetch_add(1, Ordering::Relaxed);
}

/// One message accepted by the router.
#[inline]
pub fn record_send(hart: usize) {
    SENDS.fetch_add(1, Ordering::Relaxed);
    SENDS_BY_HART[hart.min(HART_SLOTS - 1)].fetch_add(1, Ordering::Relaxed);
}

/// The window's sends by the hart that made them — which harts the lock split relieves.
pub fn sends_by_hart() -> [u64; HART_SLOTS] {
    core::array::from_fn(|i| SENDS_BY_HART[i].load(Ordering::Relaxed))
}

/// Completed `ipc_call` round trips by the hart the call was made on (TASK-0330 P2: the IPC
/// call budget on every hart, from the calls the system makes — no probe pinned anywhere):
/// from the call's start to its answer, in the caller's frame or its own collect. The server's
/// work is part of it, so a hart's fastest is its cheapest exchange — the path's cost when
/// one of its calls asked for nothing.
static CALL_FASTEST_NS: [AtomicU64; HART_SLOTS] = [const { AtomicU64::new(u64::MAX) }; HART_SLOTS];
static CALL_SUM_NS: [AtomicU64; HART_SLOTS] = [const { AtomicU64::new(0) }; HART_SLOTS];
static CALL_COUNT: [AtomicU64; HART_SLOTS] = [const { AtomicU64::new(0) }; HART_SLOTS];

/// One call's round trip of `ns`, made on `hart`.
#[inline]
pub fn record_call_round_trip(hart: usize, ns: u64) {
    let i = hart.min(HART_SLOTS - 1);
    CALL_FASTEST_NS[i].fetch_min(ns, Ordering::Relaxed);
    CALL_SUM_NS[i].fetch_add(ns, Ordering::Relaxed);
    CALL_COUNT[i].fetch_add(1, Ordering::Relaxed);
}

/// The window's call round trips by hart: the fastest and the mean in microseconds, and how
/// many (a hart without calls reads 0, 0, 0).
pub fn call_round_trips_us() -> ([u64; HART_SLOTS], [u64; HART_SLOTS], [u64; HART_SLOTS]) {
    let n: [u64; HART_SLOTS] = core::array::from_fn(|i| CALL_COUNT[i].load(Ordering::Relaxed));
    let fastest = core::array::from_fn(|i| match n[i] {
        0 => 0,
        _ => CALL_FASTEST_NS[i].load(Ordering::Relaxed) / 1_000,
    });
    let mean = core::array::from_fn(|i| {
        CALL_SUM_NS[i].load(Ordering::Relaxed).checked_div(n[i]).unwrap_or(0) / 1_000
    });
    (fastest, mean, n)
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
    /// Payload-size histogram (see [`PAYLOAD_BUCKETS`]).
    pub payload_hist: [u64; PAYLOAD_BUCKETS],
    /// `ipc_call`s finished in the caller's registers (TASK-0054C P4a).
    pub calls_in_regs: u64,
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
        payload_hist: core::array::from_fn(|i| PAYLOAD_HIST[i].load(Ordering::Relaxed)),
        calls_in_regs: CALLS_IN_REGS.load(Ordering::Relaxed),
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
    for b in PAYLOAD_HIST.iter().chain(&SENDS_BY_HART).chain(&CALL_SUM_NS).chain(&CALL_COUNT) {
        b.store(0, Ordering::Relaxed);
    }
    for f in &CALL_FASTEST_NS {
        f.store(u64::MAX, Ordering::Relaxed);
    }
    CALLS_IN_REGS.store(0, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_record_and_reset() {
        reset();
        record_send(2);
        assert_eq!(sends_by_hart()[2], 1);
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
        assert_eq!(sends_by_hart(), [0; HART_SLOTS]);

        // A call's round trip lands on the hart it was made on: the fastest, the mean and the
        // count; a hart without calls reads zero, one past the report's width shares the last
        // slot. (In this test: the counters are global, and the test harness runs in parallel.)
        record_call_round_trip(1, 61_000);
        record_call_round_trip(1, 69_000);
        record_call_round_trip(99, 40_000);
        let (fastest, mean, n) = call_round_trips_us();
        assert_eq!((fastest[1], mean[1], n[1]), (61, 65, 2));
        assert_eq!((fastest[0], mean[0], n[0]), (0, 0, 0));
        assert_eq!((fastest[HART_SLOTS - 1], n[HART_SLOTS - 1]), (40, 1));
        reset();
        assert_eq!(call_round_trips_us(), ([0; HART_SLOTS], [0; HART_SLOTS], [0; HART_SLOTS]));
    }
}
