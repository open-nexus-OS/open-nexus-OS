// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the kernel's lock with a class (TASK-0330 P2, RFC-0102). A `RankedLock` is a spin
//! lock that masks the hart's interrupts for the hold, like `SpinIrqLock`, and three things
//! more: it asks the rank rule before every take (debug builds and the `lock_rank_check`
//! feature — the check is the lock's, no caller can skip it), it books its class's waits and
//! holds (`class_stats`), and a hart that spins for it keeps answering shootdown requests, so a
//! holder may wait for acknowledgements.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: host (`tests::*`: mutual exclusion over threads, the guard's release, the
//!   refusals reaching a panic); the order rule itself in `lock_rank`
//! PUBLIC API: RankedLock::new(), set_key(), lock(), try_lock(), RankedGuard, holds_none()
//! INVARIANTS: interrupts are off before the take and until the guard is gone; the hart's held
//!   ranks are touched with interrupts off only; a refusal panics before the hart waits.
//! ADR: docs/adr/0102-a-thread-blocks-in-two-steps-and-a-waker-never-waits-ipc-leaves-the-kernel-lock.md

use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicUsize, Ordering};

use super::class_stats;
use super::lock_rank::Class;
use super::spin_irq::IrqOffGuard;

/// A spin lock of one class. Two locks of a class that pairs are ordered by their key — the
/// lock's address unless [`RankedLock::set_key`] gave it one.
pub struct RankedLock<T> {
    inner: spin::Mutex<T>,
    class: Class,
    /// The ordering key plus one; 0: the lock's address orders it.
    key: AtomicUsize,
}

impl<T> RankedLock<T> {
    /// A free lock of `class` around `value`.
    pub const fn new(class: Class, value: T) -> Self {
        Self { inner: spin::Mutex::new(value), class, key: AtomicUsize::new(0) }
    }

    /// Orders this lock within its class by `key` instead of by its address: a run queue by
    /// its CPU index, a process by its pid. Set once, before the lock is shared.
    pub fn set_key(&self, key: usize) {
        self.key.store(key.wrapping_add(1), Ordering::Release);
    }

    #[cfg(any(debug_assertions, feature = "lock_rank_check"))]
    fn key(&self) -> usize {
        match self.key.load(Ordering::Acquire) {
            0 => self as *const Self as usize,
            key => key - 1,
        }
    }

    /// Takes the lock, waiting for it with interrupts off and shootdown requests answered.
    pub fn lock(&self) -> RankedGuard<'_, T> {
        let irq = IrqOffGuard::new();
        #[cfg(any(debug_assertions, feature = "lock_rank_check"))]
        ranks::ask(self.class, self.key(), true);
        let mut spin: Option<Spin> = None;
        let guard = loop {
            if let Some(guard) = self.inner.try_lock() {
                break guard;
            }
            if spin.is_none() {
                spin = Some(Spin::begin(self as *const Self as usize));
            }
            Spin::turn();
        };
        let taken_at = now();
        let waited = spin.map(|spin| spin.end(taken_at));
        self.taken(guard, irq, taken_at, waited)
    }

    /// Takes the lock if it is free right now.
    pub fn try_lock(&self) -> Option<RankedGuard<'_, T>> {
        let irq = IrqOffGuard::new();
        #[cfg(any(debug_assertions, feature = "lock_rank_check"))]
        ranks::ask(self.class, self.key(), false);
        let guard = self.inner.try_lock()?;
        Some(self.taken(guard, irq, now(), None))
    }

    fn taken<'a>(
        &'a self,
        guard: spin::MutexGuard<'a, T>,
        irq: IrqOffGuard,
        taken_at: u64,
        waited: Option<u64>,
    ) -> RankedGuard<'a, T> {
        class_stats::took(self.class, waited);
        #[cfg(any(debug_assertions, feature = "lock_rank_check"))]
        ranks::took(self.class, self.key());
        RankedGuard { lock: self, guard: Some(guard), taken_at, _irq: irq }
    }
}

/// The hold of a [`RankedLock`]; the lock is free and the interrupts are as before when it is
/// gone.
pub struct RankedGuard<'a, T> {
    lock: &'a RankedLock<T>,
    guard: Option<spin::MutexGuard<'a, T>>,
    taken_at: u64,
    // Declared last: the interrupts come back after the lock is free.
    _irq: IrqOffGuard,
}

impl<T> Deref for RankedGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.guard.as_ref().expect("guard live until drop")
    }
}

impl<T> DerefMut for RankedGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.guard.as_mut().expect("guard live until drop")
    }
}

impl<T> Drop for RankedGuard<'_, T> {
    fn drop(&mut self) {
        let held = now().saturating_sub(self.taken_at);
        self.guard.take();
        class_stats::released(self.lock.class, held);
        #[cfg(any(debug_assertions, feature = "lock_rank_check"))]
        ranks::released(self.lock.class, self.lock.key());
    }
}

/// Whether the executing hart holds no ranked lock — what a switch and a return to user mode
/// require (RFC-0102). Always true where the check is not built.
#[must_use]
pub fn holds_none() -> bool {
    #[cfg(any(debug_assertions, feature = "lock_rank_check"))]
    {
        let _irq = IrqOffGuard::new();
        ranks::none()
    }
    #[cfg(not(any(debug_assertions, feature = "lock_rank_check")))]
    {
        true
    }
}

#[inline]
fn now() -> u64 {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        riscv::register::time::read() as u64
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        0
    }
}

/// A hart's wait for a held lock.
struct Spin {
    since: u64,
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    tag: usize,
}

impl Spin {
    #[cold]
    fn begin(_lock_at: usize) -> Self {
        Self {
            since: now(),
            // The stall line names what this hart spins for.
            #[cfg(all(target_arch = "riscv64", target_os = "none"))]
            tag: crate::smp::tlb::swap_activity(crate::smp::tlb::ACT_SPIN_WAIT, _lock_at),
        }
    }

    /// One turn of the wait. The holder may be waiting for this hart's shootdown
    /// acknowledgement: a hart with its interrupts off answers here.
    #[inline]
    fn turn() {
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        let _ = crate::smp::tlb::poll_mailbox(crate::smp::cpu_current_id());
        core::hint::spin_loop();
    }

    /// The wait is over at `taken_at`; answers how long it was.
    fn end(self, taken_at: u64) -> u64 {
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        crate::smp::tlb::restore_activity(self.tag);
        taken_at.saturating_sub(self.since)
    }
}

/// The rank check: what the executing hart holds, asked before every take.
#[cfg(any(debug_assertions, feature = "lock_rank_check"))]
mod ranks {
    use super::super::lock_rank::{Class, Held, Refusal};

    #[cfg(target_os = "none")]
    fn with<R>(f: impl FnOnce(&mut Held) -> R) -> R {
        // SAFETY: the caller masked this hart's interrupts and the block is the hart's own, so
        // nothing else reads or writes it while `f` runs.
        f(unsafe { &mut *crate::smp::lock_ranks() })
    }

    #[cfg(not(target_os = "none"))]
    fn with<R>(f: impl FnOnce(&mut Held) -> R) -> R {
        std::thread_local! {
            static HELD: core::cell::RefCell<Held> = const { core::cell::RefCell::new(Held::EMPTY) };
        }
        HELD.with(|held| f(&mut held.borrow_mut()))
    }

    pub(super) fn ask(class: Class, key: usize, wait: bool) {
        let verdict =
            with(|held| if wait { held.check(class, key) } else { held.check_try(class, key) });
        if let Err(refusal) = verdict {
            refuse(class, key, refusal);
        }
    }

    pub(super) fn took(class: Class, key: usize) {
        with(|held| held.push(class, key));
    }

    pub(super) fn released(class: Class, key: usize) {
        with(|held| held.pop(class, key));
    }

    pub(super) fn none() -> bool {
        with(|held| held.is_empty())
    }

    /// A kernel bug: the names go to the console, then the hart dies (the panic's text is a
    /// literal — the handler prints no formatted message).
    #[cold]
    fn refuse(taking: Class, key: usize, refusal: Refusal) -> ! {
        let (under, held_key) = match refusal {
            Refusal::Inversion { held, .. } => (held, 0),
            Refusal::Order { held, .. } => (taking, held),
            Refusal::TooDeep => (Class::Leaf, 0),
        };
        named(taking, key, under, held_key);
        match refusal {
            Refusal::Inversion { .. } => panic!("lock rank inversion"),
            Refusal::Order { .. } => panic!("lock order within a class"),
            Refusal::TooDeep => panic!("lock nest too deep"),
        }
    }

    #[cfg(target_os = "none")]
    fn named(taking: Class, key: usize, under: Class, held_key: usize) {
        log_error!(
            target: "lock",
            "KPANIC: lock {} 0x{:x} taken under {} 0x{:x}",
            taking.name(),
            key,
            under.name(),
            held_key
        );
    }

    #[cfg(not(target_os = "none"))]
    fn named(_taking: Class, _key: usize, _under: Class, _held_key: usize) {}
}

#[cfg(all(test, not(target_os = "none")))]
mod tests {
    use super::*;

    #[test]
    fn a_hold_excludes_and_the_guard_releases() {
        let lock = RankedLock::new(Class::Ipc, 7usize);
        {
            let mut guard = lock.lock();
            *guard += 1;
            assert!(lock.inner.try_lock().is_none(), "held");
            assert!(!holds_none());
        }
        assert!(holds_none());
        assert_eq!(*lock.lock(), 8);
        assert_eq!(*lock.try_lock().expect("free"), 8);
    }

    #[test]
    fn harts_count_together_without_a_lost_update() {
        use std::sync::Arc;
        let lock = Arc::new(RankedLock::new(Class::Thread, 0u64));
        let workers: std::vec::Vec<_> = (0..4)
            .map(|_| {
                let lock = Arc::clone(&lock);
                std::thread::spawn(move || {
                    for _ in 0..10_000 {
                        *lock.lock() += 1;
                    }
                })
            })
            .collect();
        workers.into_iter().for_each(|worker| worker.join().expect("worker"));
        assert_eq!(*lock.lock(), 40_000);
    }

    #[test]
    fn the_wake_path_nests_and_a_key_orders_two_of_a_class() {
        let endpoint = RankedLock::new(Class::Ipc, ());
        let thread = RankedLock::new(Class::Thread, ());
        let (low, high) =
            (RankedLock::new(Class::RunQueue, ()), RankedLock::new(Class::RunQueue, ()));
        low.set_key(1);
        high.set_key(6);
        let _e = endpoint.lock();
        let _t = thread.lock();
        let _l = low.lock();
        let _h = high.lock();
    }

    #[test]
    #[should_panic(expected = "lock rank inversion")]
    fn test_reject_an_endpoint_taken_under_a_thread() {
        let endpoint = RankedLock::new(Class::Ipc, ());
        let thread = RankedLock::new(Class::Thread, ());
        let _t = thread.lock();
        let _e = endpoint.lock();
    }

    #[test]
    #[should_panic(expected = "lock order within a class")]
    fn test_reject_a_lower_run_queue_waited_for_under_a_higher() {
        let (low, high) =
            (RankedLock::new(Class::RunQueue, ()), RankedLock::new(Class::RunQueue, ()));
        low.set_key(1);
        high.set_key(6);
        let _h = high.lock();
        let _l = low.lock();
    }

    #[test]
    fn a_thief_tries_a_lower_run_queue_and_never_waits() {
        let (low, high) =
            (RankedLock::new(Class::RunQueue, ()), RankedLock::new(Class::RunQueue, ()));
        low.set_key(1);
        high.set_key(6);
        let _h = high.lock();
        assert!(low.try_lock().is_some());
    }
}
