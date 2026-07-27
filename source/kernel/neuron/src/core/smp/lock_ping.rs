// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//!
//! CONTEXT: A2 lock-ping selftest — proves `SpinIrqLock` really excludes
//!          across live harts. Split out of `core/smp/mod.rs` (TASK-0306) so
//!          the SMP module holds bring-up, the online mask and rescheduling,
//!          and this holds the proof that the lock works.
//! OWNERS: @kernel
//! STATUS: Functional
//! API_STABILITY: Unstable — selftest surface
//! TEST_COVERAGE: QEMU marker ladder (`KSELFTEST: smp lock ping`)
//! ADR: docs/adr/0045-smp-production-grade.md

use super::{
    cpu_is_online, cpu_online_mask, request_resched, AtomicUsize, CpuId, Ordering, MAX_CPUS,
};

// ——— A2 lock-ping selftest: proves SpinIrqLock excludes across real harts ———

static LOCK_PING_COUNTER: crate::sync::spin_irq::SpinIrqLock<usize> =
    crate::sync::spin_irq::SpinIrqLock::new(0);
static LOCK_PING_ROUNDS: AtomicUsize = AtomicUsize::new(0);
static LOCK_PING_ACKS: AtomicUsize = AtomicUsize::new(0);

/// Secondary-hart side: performs the requested lock-ping rounds exactly once.
/// Called from the secondary park loop.
pub fn lock_ping_participate(participated: &mut bool) {
    if *participated {
        return;
    }
    let rounds = LOCK_PING_ROUNDS.load(Ordering::Acquire);
    if rounds == 0 {
        return;
    }
    for _ in 0..rounds {
        let mut counter = LOCK_PING_COUNTER.lock();
        *counter += 1;
    }
    LOCK_PING_ACKS.fetch_add(1, Ordering::AcqRel);
    *participated = true;
}

/// Boot-hart side: runs a bounded two-(or more-)hart lock ping and returns
/// `(final_counter, acked_secondaries)`. Deterministic result proof: with
/// `n` acked participants the counter must be exactly `rounds * (1 + n)` —
/// a broken lock loses increments, a fake ack inflates none.
pub fn selftest_lock_ping(rounds: usize, spin_budget: usize) -> (usize, usize) {
    {
        let mut counter = LOCK_PING_COUNTER.lock();
        *counter = 0;
    }
    LOCK_PING_ACKS.store(0, Ordering::Release);
    LOCK_PING_ROUNDS.store(rounds, Ordering::Release);
    // Parked secondaries WFI; punch them out so they observe the request.
    for idx in 1..MAX_CPUS {
        let target = CpuId::from_raw(idx as u16);
        if cpu_is_online(target) {
            let _ = request_resched(target);
        }
    }

    for _ in 0..rounds {
        let mut counter = LOCK_PING_COUNTER.lock();
        *counter += 1;
    }

    let expected_acks = cpu_online_mask().count_ones().saturating_sub(1) as usize;
    for _ in 0..spin_budget {
        if LOCK_PING_ACKS.load(Ordering::Acquire) >= expected_acks {
            break;
        }
        core::hint::spin_loop();
    }
    LOCK_PING_ROUNDS.store(0, Ordering::Release);

    let total = *LOCK_PING_COUNTER.lock();
    (total, LOCK_PING_ACKS.load(Ordering::Acquire))
}
