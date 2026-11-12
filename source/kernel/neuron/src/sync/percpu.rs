// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Per-CPU ownership wrapper — compile-time discipline for CPU-local kernel state
//! (TASK-0283). One slot per logical CPU of the machine (`HartTable`, RFC-0118): the boot
//! hart's is the static's own, the others are sized once at boot for the harts the device tree
//! names.
//! OWNERS: @kernel-team
//! STATUS: In Progress
//! API_STABILITY: Unstable
//! TEST_COVERAGE: host unit tests (slot isolation, reentrancy tripwire, a CPU without a slot)
//! PUBLIC API: PerCpu::new(), PerCpu::size(), PerCpu::with_current()
//! DEPENDS_ON: sync::hart_table, sync::spin_irq (IRQ masking), smp::cpu_current_id (OS target)
//! INVARIANTS: A slot is only ever accessed by the CPU it belongs to; access
//!             runs with interrupts masked (no trap-path reentrancy); nested
//!             access to the same wrapper on one CPU panics instead of aliasing.
//! ADR: docs/architecture/concurrency.md

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, Ordering};

use super::hart_table::{HartTable, SizeError};
use super::spin_irq;

/// One CPU's slot: the value and the tripwire against nested access on that CPU.
struct Slot<T> {
    value: UnsafeCell<T>,
    /// Nested `with_current` on the same CPU would alias `&mut T` and must fail loudly.
    entered: AtomicBool,
}

impl<T> Slot<T> {
    const fn new(value: T) -> Self {
        Self { value: UnsafeCell::new(value), entered: AtomicBool::new(false) }
    }
}

// SAFETY: A slot is only ever accessed from its owning CPU (enforced by `with_current`), with
// interrupts masked for the duration and a reentrancy tripwire against same-CPU aliasing.
// `T: Send` suffices because a slot never changes hands; no `&T` is ever shared across CPUs.
unsafe impl<T: Send> Sync for Slot<T> {}

/// CPU-local storage: one slot per logical CPU, each owned exclusively by its CPU.
///
/// Unlike a lock, there is no cross-CPU access path at all — `with_current`
/// only ever hands out the executing CPU's slot. Cross-CPU-lockable state
/// (run queues for wake/steal) belongs in a `HartTable` of `SpinIrqLock`s instead.
pub struct PerCpu<T> {
    slots: HartTable<Slot<T>>,
}

impl<T> PerCpu<T> {
    /// The boot hart's slot; the others come with [`Self::size`].
    pub const fn new(boot: T) -> Self {
        Self { slots: HartTable::new(Slot::new(boot)) }
    }

    /// Sizes the storage for a machine of `harts` CPUs (once, before a secondary starts).
    pub fn size(&self, harts: usize, mut make: impl FnMut() -> T) -> Result<(), SizeError> {
        self.slots.size(harts, || Slot::new(make()))
    }

    /// Runs `f` with exclusive access to the executing CPU's slot.
    ///
    /// Interrupts are masked for the duration so a trap on this CPU cannot
    /// re-enter the slot mid-mutation.
    pub fn with_current<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        let Some(slot) = self.slots.get(current_cpu_index()) else {
            panic!("PerCpu: no slot for this CPU");
        };

        let _irq = spin_irq::IrqOffGuard::new();

        if slot.entered.swap(true, Ordering::Acquire) {
            panic!("PerCpu: reentrant access on same CPU");
        }
        // SAFETY: the slot is the executing CPU's (its only owner), IRQs are off (no same-CPU
        // trap reentrancy), and the `entered` tripwire rejects nested closures — so this
        // &mut is unique.
        let result = f(unsafe { &mut *slot.value.get() });
        slot.entered.store(false, Ordering::Release);
        result
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

    fn machine_of(harts: usize) -> PerCpu<usize> {
        let per_cpu = PerCpu::new(0);
        per_cpu.size(harts, || 0).unwrap();
        per_cpu
    }

    #[test]
    fn slots_are_isolated_per_cpu() {
        let per_cpu = machine_of(4);

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
    fn the_boot_hart_has_its_slot_before_the_machine_is_sized() {
        static EARLY: PerCpu<usize> = PerCpu::new(5);
        test_support::set_current_index(0);
        assert_eq!(EARLY.with_current(|v| *v), 5);
    }

    #[test]
    #[should_panic(expected = "reentrant access")]
    fn reentrant_access_panics() {
        let per_cpu = machine_of(2);
        test_support::set_current_index(0);
        per_cpu.with_current(|_| {
            per_cpu.with_current(|v| *v);
        });
    }

    #[test]
    #[should_panic(expected = "no slot for this CPU")]
    fn test_reject_a_cpu_the_machine_does_not_have() {
        let per_cpu = machine_of(2);
        test_support::set_current_index(3);
        per_cpu.with_current(|v| *v);
    }
}
