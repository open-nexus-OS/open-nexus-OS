// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: which address space each hart's `satp` names, and the one place a hart's `satp`
//! changes. A hart runs on the space of the task current on it, or — with no task — on the
//! kernel's own space (`trap::pending::park_hart`, `handle_ecall`). A space a hart still runs
//! on is never freed: its page-table frames would return to the pool under that hart, and the
//! next translation the hart misses walks whatever the frames hold by then — a kernel page
//! fault at best, a stray write at worst (the 4-hart panics and board freezes of 2026-11-11:
//! a task killed by a fault left its hart on its space, the reap on another hart freed it).
//! `AddressSpaceManager::destroy` asks this table and refuses (`HeldByHart`).
//! Split out of `address_space.rs` with the switch itself (structure gate); the table is pure
//! (raw handles) and declared at the crate root, so the host tests run it.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `tests` below (host); `KSELFTEST: address space held by hart ok` at every
//!   boot (the manager refuses, then frees); a refused free in a running system prints
//!   `KSELFTEST: address space free FAIL`
//! INVARIANTS: a hart writes only its own entry, under the kernel lock that also covers
//!   `destroy`; an entry names a live space or none
//! ADR: docs/adr/0082-a-process-is-its-address-space-the-parent-orders-it-whole-freeze-thaw-kill-and-a-frozen-receiver-refuses-the-park.md

use core::cell::Cell;

/// The space each logical CPU runs on, as the raw handle (never 0 for a space; 0: the boot
/// table, before the hart's first switch) — one entry per CPU of the machine (RFC-0118), built
/// once. Interior mutability because a switch does not change the set of spaces
/// (`AddressSpaceManager::activate` takes `&self`); the manager is `!Sync` under the kernel
/// lock.
pub struct HartSpaces(alloc::boxed::Box<[Cell<u32>]>);

impl HartSpaces {
    /// A machine of `harts` CPUs, none on a space yet.
    pub fn new(harts: usize) -> Self {
        Self((0..harts).map(|_| Cell::new(0)).collect())
    }

    /// Hart `cpu`'s `satp` now names the space `raw`.
    pub fn enter(&self, cpu: usize, raw: u32) {
        if let Some(slot) = self.0.get(cpu) {
            slot.set(raw);
        }
    }

    /// Whether hart `cpu`'s `satp` names the space `raw`.
    #[must_use]
    pub fn is_on(&self, cpu: usize, raw: u32) -> bool {
        self.0.get(cpu).is_some_and(|slot| slot.get() == raw)
    }

    /// A hart whose `satp` names the space `raw`, if any.
    #[must_use]
    pub fn hart_on(&self, raw: u32) -> Option<usize> {
        self.0.iter().position(|slot| slot.get() == raw)
    }
}

/// Writes this hart's `satp` (through the island: everything after it runs in the new space).
#[cfg(all(target_arch = "riscv64", target_os = "none", not(feature = "selftest_no_satp")))]
#[inline(never)]
pub(crate) fn switch_satp(satp_val: usize) {
    use crate::mm::PAGE_SIZE;

    const LOG_LIMIT: usize = 16;
    static LOG_COUNT: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
    let log_now = LOG_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed) < LOG_LIMIT
        && crate::log::would_log(crate::log::Level::Trace, "vm");

    // SAFE logging using raw UART - no heap allocation, logs in separate scopes
    if log_now {
        use core::fmt::Write;
        let mut u = crate::uart::raw_writer();
        let _ = u.write_str("AS: activating satp=0x");
        crate::trap::uart_write_hex(&mut u, satp_val);
        let _ = u.write_str("\n");
    }

    // Preflight: verify PC is executable
    if log_now {
        use core::fmt::Write;
        let mut u = crate::uart::raw_writer();
        let pc = crate::arch::riscv::read_pc() & !(PAGE_SIZE - 1);
        let _ = u.write_str("AS: pc check va=0x");
        crate::trap::uart_write_hex(&mut u, pc);
        let _ = u.write_str("\n");
    }

    // Preflight: verify SP is accessible
    if log_now {
        use core::fmt::Write;
        let mut u = crate::uart::raw_writer();
        let sp = crate::arch::riscv::read_sp() & !(PAGE_SIZE - 1);
        let _ = u.write_str("AS: sp check va=0x");
        crate::trap::uart_write_hex(&mut u, sp);
        let _ = u.write_str("\n");
    }

    ensure_rx_guard();

    // Call satp switch - any code after this runs in new AS!
    unsafe {
        extern "C" {
            fn satp_switch_island(val: usize);
        }
        satp_switch_island(satp_val);
    }
    // Function returns - back to activate() which is now in new AS
}

#[cfg(all(target_arch = "riscv64", target_os = "none", not(feature = "selftest_no_satp")))]
fn ensure_rx_guard() {
    const LOG_LIMIT: usize = 16;
    static LOG_COUNT: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
    let log_now = LOG_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed) < LOG_LIMIT
        && crate::log::would_log(crate::log::Level::Trace, "vm");

    let pc = crate::arch::riscv::read_pc();
    if log_now {
        use core::fmt::Write;
        let mut u = crate::uart::raw_writer();
        let _ = u.write_str("AS: ensure_rx_guard pc=0x");
        crate::trap::uart_write_hex(&mut u, pc);
        let _ = u.write_str("\n");
    }
    let mut zero = true;
    let mut offset = 0usize;
    while offset < 8 {
        let byte = unsafe { core::ptr::read_volatile((pc + offset) as *const u8) };
        if byte != 0 {
            zero = false;
        }
        offset += 1;
    }
    if zero {
        panic!("rx guard: zero fetch");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KERNEL: u32 = 1;
    const TASK: u32 = 7;
    const HARTS: usize = 4;

    #[test]
    fn a_hart_is_on_the_space_it_entered_last() {
        let harts = HartSpaces::new(HARTS);
        assert_eq!(harts.hart_on(TASK), None);
        harts.enter(1, TASK);
        harts.enter(3, KERNEL);
        assert_eq!((harts.hart_on(TASK), harts.hart_on(KERNEL)), (Some(1), Some(3)));
        assert!(harts.is_on(1, TASK) && !harts.is_on(3, TASK));
        harts.enter(1, KERNEL);
        assert_eq!((harts.hart_on(TASK), harts.hart_on(KERNEL)), (None, Some(1)));
    }

    #[test]
    fn test_reject_a_free_while_any_hart_still_runs_on_the_space() {
        let harts = HartSpaces::new(HARTS);
        // Two harts ran threads of the process; both tasks are dead, neither hart has left.
        harts.enter(0, TASK);
        harts.enter(2, TASK);
        assert!(harts.hart_on(TASK).is_some());
        // One hart leaving is not enough: the other still walks the page table.
        harts.enter(0, KERNEL);
        assert_eq!(harts.hart_on(TASK), Some(2));
        harts.enter(2, KERNEL);
        assert_eq!(harts.hart_on(TASK), None);
    }

    #[test]
    fn test_reject_a_hart_the_table_does_not_have() {
        let harts = HartSpaces::new(HARTS);
        harts.enter(HARTS, TASK);
        assert_eq!(harts.hart_on(TASK), None);
        assert!(!harts.is_on(HARTS, TASK));
    }
}
