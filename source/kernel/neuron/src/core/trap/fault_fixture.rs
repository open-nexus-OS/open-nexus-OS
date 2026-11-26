// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the kernel-fault fixture (TASK-0330 P7) — the proof lane's forced kernel page
//! fault. Compiled only with the feature `selftest_kernel_fault`, which no shipped kernel
//! carries; armed only on the FIRST boot of a boot trace (`/chosen/nexus,trace` sequence 1),
//! so the boot after the reset runs to its end and shows what the loader rescued. The fault is
//! real — a load from an address no Sv39 table can map, taken inside the timer interrupt of a
//! running user task — and everything after it is the kernel's ordinary path: the trap vector,
//! `fatal::begin`, the fault line, the panic. Only the trigger is the fixture's.
//! OWNERS: @kernel-team
//! STATUS: Functional (proof fixture)
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU `ci-os-kfault`; the board's forced-panic cycle
//! INVARIANTS: fires at most once a boot; never armed without a trace sequence of 1.

use core::sync::atomic::{AtomicU64, Ordering};

/// When the fault fires, in timer ticks; 0 = not armed (or already fired).
static FIRE_AT: AtomicU64 = AtomicU64::new(0);

/// How long after the arm: the services are up and printing on every hart, so the ring holds a
/// boot's worth of lines and other harts are mid-line when the fault comes.
const DELAY_NS: u64 = 8_000_000_000;

/// Sv39 maps addresses whose bits 63..39 repeat bit 38; this one does not, so no table of any
/// address space translates it — the load page-faults with `stval` = this value.
const WILD: usize = 0x0000_4000_dead_0000;

/// Arms the fixture for the boot whose trace sequence is `seq`: the first only.
pub(crate) fn arm(seq: u64) {
    if seq != 1 {
        log_info!(target: "selftest", "KSELFTEST: kernel fault fixture idle (seq={})", seq);
        return;
    }
    let at = (riscv::register::time::read() as u64)
        .saturating_add(crate::hal::platform::ns_to_ticks(DELAY_NS));
    FIRE_AT.store(at.max(1), Ordering::Release);
    log_info!(
        target: "selftest",
        "KSELFTEST: kernel fault fixture armed (seq={} in_ms={})",
        seq,
        DELAY_NS / 1_000_000
    );
}

/// A timer interrupt took this hart out of a user task: fire once the delay has passed — on a
/// secondary hart when the machine has one, so the hart that dies is not the one that booted.
pub(crate) fn tick() {
    let at = FIRE_AT.load(Ordering::Acquire);
    if at == 0 || (riscv::register::time::read() as u64) < at {
        return;
    }
    if crate::hal::platform::hart_count() > 1 && crate::smp::cpu_current_id().is_boot() {
        return;
    }
    if FIRE_AT.swap(0, Ordering::AcqRel) == 0 {
        return;
    }
    // SAFETY: none is claimed — this load is the fault the fixture exists to raise; the trap
    // it takes ends in the kernel's panic and never returns here.
    let _ = unsafe { core::ptr::read_volatile(WILD as *const u64) };
}
