// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: which address space each hart's `satp` names, and the one place a hart's `satp`
//! changes. A hart runs on the space of the task current on it, or — with no task — on the
//! kernel's own space (`trap::pending::park_hart`, `handle_ecall`; off the kernel lock
//! `enter_kernel_space`, in the hold that gives the task up). A space a hart still runs
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
//! INVARIANTS: a hart writes only its own entry; `destroy` reads them under the kernel lock and
//!   a space's last task is withdrawn from every hart before its reap; a hart that gives a task
//!   up is off its space before another hart can find the task free; an entry names a live
//!   space or none; a hart that may hold a translation of a space is in that space's set
//!   (TASK-0330 P3: what a shootdown for one space reaches)
//! ADR: docs/adr/0082-a-process-is-its-address-space-the-parent-orders-it-whole-freeze-thaw-kill-and-a-frozen-receiver-refuses-the-park.md

use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, AtomicUsize, Ordering};

/// The space each logical CPU runs on, as the raw handle (never 0 for a space; 0: the boot
/// table, before the hart's first switch) — one entry per CPU of the machine (RFC-0118), built
/// once. Atomic entries: a hart enters a space at its dispatch, which takes no kernel lock
/// (TASK-0363 P2 S3); `destroy` reads them under the lock, and a hart's own entry is written only
/// by that hart.
///
/// With it, for each space, the CPUs that may hold translations of it (TASK-0330 P3) — a bit
/// per CPU, by the space's raw handle: a shootdown for one space reaches these and no other.
/// A hart sets its own bit before it can load a translation of the space (before its `satp`
/// names it, before it copies through its tables) and clears it after a full flush that finds
/// it on another space; nobody else writes a hart's bit.
pub struct HartSpaces {
    on: alloc::boxed::Box<[AtomicU32]>,
    ran: alloc::boxed::Box<[AtomicU64]>,
    /// The kernel's own space — where a hart with no task runs — as its raw handle (0: not
    /// named yet), its `satp` and its cache bit.
    kernel: (AtomicU32, AtomicUsize, AtomicBool),
}

impl HartSpaces {
    /// A machine of `harts` CPUs, none on a space yet, for up to `spaces` address spaces.
    pub fn new(harts: usize, spaces: usize) -> Self {
        Self {
            on: (0..harts).map(|_| AtomicU32::new(0)).collect(),
            ran: (0..spaces).map(|_| AtomicU64::new(0)).collect(),
            kernel: (AtomicU32::new(0), AtomicUsize::new(0), AtomicBool::new(false)),
        }
    }

    /// Names the kernel's own space, once, at boot. It is the machine's and no task's: a
    /// task's record names what that task runs on and follows every attach — PID 0's
    /// included (a boot selftest that attached a probe space to PID 0 and detached it left
    /// the record with no space, and every hart that gave a task up off the kernel lock
    /// stayed on that task's space: 2026-11-25).
    pub fn name_kernel(&self, raw: u32, satp: usize, cache: bool) {
        self.kernel.1.store(satp, Ordering::Relaxed);
        self.kernel.2.store(cache, Ordering::Relaxed);
        self.kernel.0.store(raw, Ordering::Release);
    }

    /// The kernel's own space as `(raw handle, satp, cache)`; `None` before it is named.
    #[must_use]
    pub fn kernel(&self) -> Option<(u32, usize, bool)> {
        let raw = self.kernel.0.load(Ordering::Acquire);
        (raw != 0).then(|| {
            (raw, self.kernel.1.load(Ordering::Relaxed), self.kernel.2.load(Ordering::Relaxed))
        })
    }

    fn ran_word(&self, raw: u32) -> Option<&AtomicU64> {
        self.ran.get(raw.checked_sub(1)? as usize)
    }

    /// Hart `cpu`'s `satp` now names the space `raw`.
    pub fn enter(&self, cpu: usize, raw: u32) {
        if let Some(slot) = self.on.get(cpu) {
            slot.store(raw, Ordering::Release);
        }
    }

    /// Hart `cpu` is about to read through the tables of the space `raw` — its `satp` will
    /// name it, or it copies through them: from now on the space's shootdowns reach it. The
    /// bit is set with a full barrier, so of a hart that sets it and an unmap that cleared an
    /// entry before reading the set, one sees the other.
    pub fn reads_through(&self, cpu: usize, raw: u32) {
        let (Some(word), true) = (self.ran_word(raw), cpu < 64) else { return };
        if word.load(Ordering::Relaxed) & (1 << cpu) == 0 {
            word.fetch_or(1 << cpu, Ordering::SeqCst);
        }
    }

    /// The CPUs a shootdown for the space `raw` must reach, as a bit set; `None` for a space
    /// the table has no word for (the caller reaches every hart).
    #[must_use]
    pub fn ran(&self, raw: u32) -> Option<u64> {
        self.ran_word(raw).map(|word| word.load(Ordering::SeqCst))
    }

    /// Hart `cpu` flushed every translation it held: unless its `satp` names the space `raw`,
    /// it holds none of it any more and leaves the space's set.
    pub fn flushed(&self, cpu: usize, raw: u32) {
        if let (Some(word), true, false) = (self.ran_word(raw), cpu < 64, self.is_on(cpu, raw)) {
            word.fetch_and(!(1 << cpu), Ordering::SeqCst);
        }
    }

    /// The handle `raw` names a new space: no hart holds a translation of it (its ASID is
    /// fresh or was recycled behind a flush of every hart).
    pub fn fresh(&self, raw: u32) {
        if let Some(word) = self.ran_word(raw) {
            word.store(0, Ordering::SeqCst);
        }
    }

    /// Whether hart `cpu`'s `satp` names the space `raw`.
    #[must_use]
    pub fn is_on(&self, cpu: usize, raw: u32) -> bool {
        self.on.get(cpu).is_some_and(|slot| slot.load(Ordering::Acquire) == raw)
    }

    /// A hart whose `satp` names the space `raw`, if any.
    #[must_use]
    pub fn hart_on(&self, raw: u32) -> Option<usize> {
        self.on.iter().position(|slot| slot.load(Ordering::Acquire) == raw)
    }

    /// Makes this table the machine's (the address-space manager's, once at boot): the dispatch
    /// that takes no kernel lock enters a space through it (`installed`).
    pub fn install(self) -> &'static HartSpaces {
        let table: &'static HartSpaces = alloc::boxed::Box::leak(alloc::boxed::Box::new(self));
        INSTALLED.store(core::ptr::from_ref(table).cast_mut(), Ordering::Release);
        table
    }
}

/// The machine's table, once the address-space manager exists.
static INSTALLED: AtomicPtr<HartSpaces> = AtomicPtr::new(core::ptr::null_mut());

/// The machine's table (`HartSpaces::install`); `None` before the manager exists.
pub fn installed() -> Option<&'static HartSpaces> {
    // SAFETY: a non-null pointer is the leaked, never-freed table `install` published.
    unsafe { INSTALLED.load(Ordering::Acquire).as_ref() }
}

/// This hart gives up the task it runs under no kernel lock (a block or a requeue in a syscall
/// that entered before the lock, TASK-0330 P2): it leaves that task's space for the kernel's
/// own — in the hold that makes the task another hart's to wake, take or reap (its thread's
/// lock, its queue's lock), so no reap finds this hart on the space of a task it has given up.
///
/// The space is the one the machine named at boot (`HartSpaces::name_kernel`). A hart that
/// cannot leave says so, FAIL-shaped and once: staying is silent otherwise, and shows only
/// when a reap finds the hart there.
#[cfg(target_os = "none")]
pub fn enter_kernel_space() {
    match installed().and_then(HartSpaces::kernel) {
        Some((raw, satp, cache)) => {
            enter_space(crate::sched::record::SpaceView { raw, satp, cache })
        }
        None => {
            static SAID: AtomicBool = AtomicBool::new(false);
            if !SAID.swap(true, Ordering::Relaxed) {
                log_error!(
                    target: "mm",
                    "KSELFTEST: hart leave FAIL (cpu{} gives a task up; the kernel's space is not named)",
                    crate::smp::cpu_current_id().as_index()
                );
            }
        }
    }
}

/// Switches this hart to the space `view` names: its `satp`, the machine's table of which hart is
/// on which space, and user-mode cache maintenance exactly when the space holds it (ADR-0079).
/// The one switch: the manager's `activate` and the dispatch that takes no lock (TASK-0363 P2
/// S3) both take it.
#[cfg(target_os = "none")]
#[track_caller]
pub fn enter_space(view: crate::sched::record::SpaceView) {
    crate::smp::trail::entered_space();
    // TASK-0330 P3: the space's shootdowns reach this hart from before its `satp` names it —
    // and no shootdown is answered between the three steps (the answer would read which space
    // the hart is on, half-way).
    let _whole = crate::sync::spin_irq::IrqOffGuard::new();
    let (harts, cpu) = (installed(), crate::smp::cpu_current_id().as_index());
    if let Some(harts) = harts {
        harts.reads_through(cpu, view.raw);
    }
    // CRITICAL: a separate function, so every log and temporary drops BEFORE the switch.
    #[cfg(all(target_arch = "riscv64", not(feature = "selftest_no_satp")))]
    switch_satp(view.satp);
    if let Some(harts) = harts {
        harts.enter(cpu, view.raw);
    }
    #[cfg(target_arch = "riscv64")]
    crate::hal::platform::apply_user_cache_maintenance(view.cache);
    #[cfg(all(target_arch = "riscv64", feature = "selftest_no_satp"))]
    {
        // Skip SATP switch for bring-up; emit post-switch marker directly
        crate::mm::address_space::fence_i();
        log_debug!(target: "vm", "post-satp OK");
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

    const SPACES: usize = 16;

    #[test]
    fn the_kernels_space_is_the_machines_and_no_hart_or_task_changes_it() {
        let harts = HartSpaces::new(HARTS, SPACES);
        assert_eq!(harts.kernel(), None, "a hart cannot leave for a space nobody named");
        harts.name_kernel(KERNEL, 0x8000_1234, false);
        // Harts enter and leave spaces; what a hart with no task runs on stays.
        harts.enter(2, TASK);
        harts.reads_through(2, TASK);
        harts.enter(2, KERNEL);
        assert_eq!(harts.kernel(), Some((KERNEL, 0x8000_1234, false)));
    }

    #[test]
    fn a_shootdown_for_a_space_reaches_the_harts_that_read_through_it() {
        let harts = HartSpaces::new(HARTS, SPACES);
        assert_eq!(harts.ran(TASK), Some(0), "nobody ran it yet");
        harts.reads_through(1, TASK);
        harts.enter(1, TASK);
        harts.reads_through(3, TASK);
        harts.reads_through(3, TASK);
        assert_eq!(harts.ran(TASK), Some(0b1010));
        assert_eq!(harts.ran(KERNEL), Some(0), "one space's harts are not another's");
        // Hart 3 copied through the tables without being on the space: a full flush empties
        // it of them and it leaves. Hart 1 is on the space: its flush refills from it.
        harts.flushed(3, TASK);
        harts.flushed(1, TASK);
        assert_eq!(harts.ran(TASK), Some(0b0010));
        // Off the space, the next flush lets it leave too.
        harts.enter(1, KERNEL);
        harts.flushed(1, TASK);
        assert_eq!(harts.ran(TASK), Some(0));
    }

    #[test]
    fn a_new_space_under_an_old_handle_starts_with_no_hart() {
        let harts = HartSpaces::new(HARTS, SPACES);
        harts.reads_through(2, TASK);
        harts.fresh(TASK);
        assert_eq!(harts.ran(TASK), Some(0));
    }

    #[test]
    fn test_reject_a_space_or_a_hart_the_set_does_not_have() {
        let harts = HartSpaces::new(HARTS, SPACES);
        // No word: the caller reaches every hart instead.
        assert_eq!(harts.ran(0), None);
        assert_eq!(harts.ran(SPACES as u32 + 1), None);
        harts.reads_through(64, TASK);
        harts.reads_through(1, 0);
        harts.flushed(64, TASK);
        harts.fresh(0);
        assert_eq!(harts.ran(TASK), Some(0));
    }

    #[test]
    fn a_hart_is_on_the_space_it_entered_last() {
        let harts = HartSpaces::new(HARTS, SPACES);
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
        let harts = HartSpaces::new(HARTS, SPACES);
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
        let harts = HartSpaces::new(HARTS, SPACES);
        harts.enter(HARTS, TASK);
        assert_eq!(harts.hart_on(TASK), None);
        assert!(!harts.is_on(HARTS, TASK));
    }
}
