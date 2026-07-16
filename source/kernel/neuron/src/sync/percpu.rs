// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Per-CPU ownership wrapper — compile-time discipline for CPU-local kernel state (TASK-0283)
//! OWNERS: @kernel-team
//! STATUS: In Progress
//! API_STABILITY: Unstable
//! TEST_COVERAGE: host unit tests (slot isolation, reentrancy tripwire)
//! PUBLIC API: PerCpu::new(), PerCpu::with_current()
//! DEPENDS_ON: sync::spin_irq (IRQ masking), smp::cpu_current_id (OS target)
//! INVARIANTS: A slot is only ever accessed by the CPU it belongs to; access
//!             runs with interrupts masked (no trap-path reentrancy); nested
//!             access to the same wrapper on one CPU panics instead of aliasing.
//! ADR: docs/architecture/16-rust-concurrency-model.md

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, Ordering};

use super::spin_irq;

/// CPU-local storage: `N` slots, each owned exclusively by one CPU.
///
/// Unlike a lock, there is no cross-CPU access path at all — `with_current`
/// only ever hands out the executing CPU's slot. Cross-CPU-lockable state
/// (run queues for wake/steal) belongs in `SpinIrqLock` arrays instead.
pub struct PerCpu<T, const N: usize> {
    slots: [UnsafeCell<T>; N],
    /// Reentrancy tripwire per slot: nested `with_current` on the same CPU
    /// would alias `&mut T` and must fail loudly.
    entered: [AtomicBool; N],
}

// SAFETY: Each slot is only ever accessed from its owning CPU (enforced by
// `with_current`), with interrupts masked for the duration and a reentrancy
// tripwire against same-CPU aliasing. `T: Send` suffices because a slot never
// changes hands; no `&T` is ever shared across CPUs (no Sync bound needed).
unsafe impl<T: Send, const N: usize> Sync for PerCpu<T, N> {}

impl<T, const N: usize> PerCpu<T, N> {
    pub const fn new(values: [T; N]) -> Self {
        Self { slots: unsafe_cell_array(values), entered: [const { AtomicBool::new(false) }; N] }
    }

    /// Runs `f` with exclusive access to the executing CPU's slot.
    ///
    /// Interrupts are masked for the duration so a trap on this CPU cannot
    /// re-enter the slot mid-mutation.
    pub fn with_current<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        let idx = current_cpu_index();
        assert!(idx < N, "PerCpu: CPU index out of range");

        let _irq = spin_irq::IrqOffGuard::new();

        if self.entered[idx].swap(true, Ordering::Acquire) {
            panic!("PerCpu: reentrant access on same CPU");
        }
        // SAFETY: `idx` is the executing CPU (only owner of this slot), IRQs
        // are off (no same-CPU trap reentrancy), and the `entered` tripwire
        // rejects nested closures — so this &mut is unique.
        let result = f(unsafe { &mut *self.slots[idx].get() });
        self.entered[idx].store(false, Ordering::Release);
        result
    }
}

/// `const`-context helper: rewraps a value array into `UnsafeCell`s.
const fn unsafe_cell_array<T, const N: usize>(values: [T; N]) -> [UnsafeCell<T>; N] {
    // SAFETY: UnsafeCell<T> is repr(transparent) over T, so the array layouts
    // are identical; ownership moves wholesale without duplication.
    unsafe {
        let wrapped = core::ptr::read(&values as *const [T; N] as *const [UnsafeCell<T>; N]);
        core::mem::forget(values);
        wrapped
    }
}

#[inline]
fn current_cpu_index() -> usize {
    #[cfg(target_os = "none")]
    {
        crate::smp::cpu_current_id().as_index()
    }
    #[cfg(not(target_os = "none"))]
    {
        test_support::current_index()
    }
}

/// Host-test hook: lets unit tests steer which "CPU" is executing.
#[cfg(not(target_os = "none"))]
pub(crate) mod test_support {
    use std::cell::Cell;

    std::thread_local! {
        static FAKE_CPU_INDEX: Cell<usize> = const { Cell::new(0) };
    }

    pub(crate) fn current_index() -> usize {
        FAKE_CPU_INDEX.with(|v| v.get())
    }

    pub(crate) fn set_current_index(idx: usize) {
        FAKE_CPU_INDEX.with(|v| v.set(idx));
    }
}

#[cfg(all(test, not(target_os = "none")))]
mod tests {
    use super::*;

    #[test]
    fn slots_are_isolated_per_cpu() {
        let per_cpu: PerCpu<usize, 4> = PerCpu::new([0; 4]);

        test_support::set_current_index(0);
        per_cpu.with_current(|v| *v = 11);

        test_support::set_current_index(1);
        per_cpu.with_current(|v| *v = 22);
        // CPU 1 sees its own slot, untouched by CPU 0's write.
        assert_eq!(per_cpu.with_current(|v| *v), 22);

        test_support::set_current_index(0);
        assert_eq!(per_cpu.with_current(|v| *v), 11);
    }

    #[test]
    #[should_panic(expected = "reentrant access")]
    fn reentrant_access_panics() {
        let per_cpu: PerCpu<usize, 2> = PerCpu::new([0; 2]);
        test_support::set_current_index(0);
        per_cpu.with_current(|_| {
            per_cpu.with_current(|v| *v);
        });
    }

    #[test]
    #[should_panic(expected = "out of range")]
    fn out_of_range_cpu_panics() {
        let per_cpu: PerCpu<usize, 2> = PerCpu::new([0; 2]);
        test_support::set_current_index(3);
        per_cpu.with_current(|v| *v);
    }
}
