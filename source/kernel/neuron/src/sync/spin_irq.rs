// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: IRQ-safe spinlock — the only lock type allowed in trap-reachable paths (TASK-0277)
//! OWNERS: @kernel-team
//! STATUS: In Progress
//! API_STABILITY: Unstable
//! TEST_COVERAGE: host unit tests (mutual exclusion, guard drop, relock detection)
//! PUBLIC API: SpinIrqLock::new(), SpinIrqLock::lock(), SpinIrqGuard
//! DEPENDS_ON: spin::Mutex, sstatus.SIE CSR (OS target only)
//! INVARIANTS: SIE is cleared BEFORE acquisition and restored to its prior
//!             state on guard drop; same-hart re-lock panics in debug builds
//!             instead of deadlocking silently.
//! ADR: docs/rfcs/RFC-0022-kernel-smp-v1b-scheduler-hardening-contract.md

use core::ops::{Deref, DerefMut};

#[cfg(debug_assertions)]
use core::sync::atomic::{AtomicUsize, Ordering};

/// Atomically clears `sstatus.SIE` and reports whether it was set before.
///
/// Interrupts must be off *before* the spin acquisition: a trap taken while
/// holding the lock on the same hart would re-enter and deadlock.
#[inline(always)]
fn irq_save_disable() -> bool {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SIE_BIT: usize = 1 << 1;
        let prev: usize;
        // SAFETY: csrrci atomically clears SIE and returns the prior sstatus;
        // masking supervisor interrupts has no memory-safety implications.
        unsafe {
            core::arch::asm!(
                "csrrci {prev}, sstatus, 2",
                prev = out(reg) prev,
                options(nomem, nostack, preserves_flags)
            );
        }
        prev & SIE_BIT != 0
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        false
    }
}

/// Re-enables `sstatus.SIE`. Only called when `irq_save_disable` reported it set.
#[inline(always)]
fn irq_restore(was_enabled: bool) {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        if was_enabled {
            // SAFETY: setting SIE re-enables supervisor interrupts; the caller
            // observed SIE set before the paired irq_save_disable.
            unsafe {
                core::arch::asm!(
                    "csrrsi zero, sstatus, 2",
                    options(nomem, nostack, preserves_flags)
                );
            }
        }
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = was_enabled;
    }
}

/// RAII scope with supervisor interrupts masked: SIE is cleared on
/// construction and restored to its prior state on drop.
///
/// Building block for `PerCpu` and other CPU-local critical sections that
/// need trap-reentrancy protection without a lock.
pub struct IrqOffGuard {
    was_enabled: bool,
}

impl IrqOffGuard {
    #[inline]
    pub fn new() -> Self {
        Self { was_enabled: irq_save_disable() }
    }
}

impl Default for IrqOffGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for IrqOffGuard {
    fn drop(&mut self) {
        irq_restore(self.was_enabled);
    }
}

/// Spinlock that masks supervisor interrupts for the full hold duration.
///
/// Trap-reachable kernel state must only be protected by this type: plain
/// spinlocks deadlock when a trap on the holding hart re-enters the lock.
pub struct SpinIrqLock<T> {
    inner: spin::Mutex<T>,
    /// Debug relock tripwire: CPU index + 1 of the current holder, 0 = free.
    #[cfg(debug_assertions)]
    holder: AtomicUsize,
}

impl<T> SpinIrqLock<T> {
    pub const fn new(value: T) -> Self {
        Self {
            inner: spin::Mutex::new(value),
            #[cfg(debug_assertions)]
            holder: AtomicUsize::new(0),
        }
    }

    /// Acquires the lock and returns a guard that restores the prior
    /// interrupt state on drop. SIE is guaranteed OFF only while the lock is
    /// HELD; between failed acquisition attempts interrupts are re-enabled —
    /// a hart spinning for the BKL must stay responsive to correctness IPIs
    /// (A5 TLB shootdown: the initiator holds the BKL while waiting for
    /// acks; a responder spinning IRQ-off for that same lock would deadlock).
    pub fn lock(&self) -> SpinIrqGuard<'_, T> {
        loop {
            #[cfg(debug_assertions)]
            {
                let me = current_cpu_index_for_debug() + 1;
                if self.holder.load(Ordering::Acquire) == me {
                    // A same-hart re-lock would spin forever.
                    panic!("SpinIrqLock: same-hart re-lock");
                }
            }

            let was_enabled = irq_save_disable();
            if let Some(guard) = self.inner.try_lock() {
                #[cfg(debug_assertions)]
                self.holder.store(current_cpu_index_for_debug() + 1, Ordering::Release);
                return SpinIrqGuard { lock: self, guard: Some(guard), was_enabled };
            }
            // Not acquired: reopen the interrupt window before retrying.
            irq_restore(was_enabled);
            core::hint::spin_loop();
        }
    }
}

#[cfg(debug_assertions)]
#[inline]
fn current_cpu_index_for_debug() -> usize {
    #[cfg(target_os = "none")]
    {
        crate::smp::cpu_current_id().as_index()
    }
    #[cfg(not(target_os = "none"))]
    {
        // Host stand-in: a unique per-thread index, so cross-thread contention
        // is never mistaken for a same-hart re-lock.
        use std::sync::atomic::{AtomicUsize as StdAtomicUsize, Ordering as StdOrdering};
        static NEXT_THREAD_INDEX: StdAtomicUsize = StdAtomicUsize::new(0);
        std::thread_local! {
            static THREAD_INDEX: usize = NEXT_THREAD_INDEX.fetch_add(1, StdOrdering::Relaxed);
        }
        THREAD_INDEX.with(|v| *v)
    }
}

pub struct SpinIrqGuard<'a, T> {
    lock: &'a SpinIrqLock<T>,
    // Option so Drop can release the inner guard BEFORE restoring SIE.
    guard: Option<spin::MutexGuard<'a, T>>,
    was_enabled: bool,
}

impl<'a, T> Deref for SpinIrqGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        self.guard.as_ref().expect("guard live until drop")
    }
}

impl<'a, T> DerefMut for SpinIrqGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.guard.as_mut().expect("guard live until drop")
    }
}

impl<'a, T> Drop for SpinIrqGuard<'a, T> {
    fn drop(&mut self) {
        #[cfg(debug_assertions)]
        self.lock.holder.store(0, Ordering::Release);
        #[cfg(not(debug_assertions))]
        let _ = &self.lock;

        // Release the lock while interrupts are still off, THEN restore SIE:
        // a trap arriving in between must never observe the lock held.
        self.guard.take();
        irq_restore(self.was_enabled);
    }
}

#[cfg(all(test, not(target_os = "none")))]
mod tests {
    use super::*;

    #[test]
    fn lock_grants_mutable_access_and_releases() {
        let lock = SpinIrqLock::new(7usize);
        {
            let mut g = lock.lock();
            *g += 1;
        }
        assert_eq!(*lock.lock(), 8);
    }

    #[test]
    fn mutual_exclusion_across_threads() {
        use std::sync::Arc;

        let lock = Arc::new(SpinIrqLock::new(0usize));
        let mut handles = Vec::new();
        for _ in 0..4 {
            let lock = Arc::clone(&lock);
            handles.push(std::thread::spawn(move || {
                for _ in 0..1000 {
                    let mut g = lock.lock();
                    let v = *g;
                    // Widen the race window: a broken lock loses increments.
                    std::hint::black_box(v);
                    *g = v + 1;
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(*lock.lock(), 4000);
    }

    #[test]
    #[should_panic(expected = "same-hart re-lock")]
    fn same_hart_relock_panics_in_debug() {
        let lock = SpinIrqLock::new(0usize);
        let _g = lock.lock();
        let _g2 = lock.lock();
    }
}
