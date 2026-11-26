// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: a dying kernel's first act (TASK-0330 P7). A kernel fault and a panic both begin
//! here, before one word is printed: the hart claims the console for the machine's last words
//! (every other hart falls silent at its next console byte, so nothing interleaves with them),
//! leaves the address space of the task it ran for the kernel's own (a fault on a page table
//! that was freed under the hart read the panic text itself through garbage mappings — the
//! 2026-11-11 fault left no `KPGF` line and four unreadable bytes for a message), and sends
//! what the serial line still owes. Then the fault line names the hart and where it stood, and
//! the panic text follows it. The words are in the console ring before the hart stops, which is
//! what the next boot's loader rescues from RAM (RFC-0107 Phase 3).
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the claim's rule on the host (`fatal_rule`); QEMU `ci-os-kfault` (`KPGF
//!   hart=` before `PANIC hart=`, the text whole to `PANIC: end`, the next boot rescues the ring)
//! INVARIANTS: takes no lock it waits for without a bound, allocates nothing, and reads no
//!   state another hart may be changing — it runs with the kernel's invariants already broken.
//! ADR: docs/adr/0016-kernel-libs-architecture.md

use core::sync::atomic::{AtomicUsize, Ordering};

use crate::fatal_rule::{claim, silenced, Claim};

/// The hart that speaks the machine's last words, as `cpu + 1`; 0 while the kernel lives
/// (`fatal_rule`).
static DYING: AtomicUsize = AtomicUsize::new(0);

/// The console's leaf asks before every byte: a hart that is not the one dying stops here, so
/// no line of its own lands between the last words. One atomic load while the kernel lives.
#[inline]
pub(crate) fn park_if_silenced() {
    if DYING.load(Ordering::Acquire) != 0
        && silenced(&DYING, crate::smp::cpu_current_id().as_index())
    {
        park();
    }
}

/// The hart that speaks, and the space it stood on when it began.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Dying {
    /// The hart's index.
    pub(crate) cpu: usize,
    /// Its `satp` at the fault, before it left for the kernel's own space.
    pub(crate) satp: usize,
}

/// A fault's or a panic's first act on this hart; returns who speaks for the line that
/// follows. A hart that is not the first to die never returns: it parks, silent.
pub(crate) fn begin() -> Dying {
    // No interrupt may take this hart out of its last words.
    // SAFETY: masking this hart's interrupts is always sound; it never unmasks them again.
    unsafe { riscv::register::sstatus::clear_sie() };
    let cpu = crate::smp::cpu_current_id().as_index();
    let satp = riscv::register::satp::read().bits();
    match claim(&DYING, cpu) {
        Claim::First => {
            enter_kernel_space();
            // TASK-0330 S2a, S2b: what the serial line still owes goes out synchronously now,
            // and everything after it — a dying kernel waits for its own last words.
            crate::hal::platform::console_async_disable_and_drain();
            // TASK-0327B P4 H0d: this hart's gathered partial line goes out first, so the
            // trace ends at the dying hart's last byte before its last words.
            crate::hal::platform::console_flush_line();
        }
        // The console is already synchronous and this hart's; say on.
        Claim::Nested => {}
        Claim::Silenced => park(),
    }
    Dying { cpu, satp }
}

/// This hart stops for good, saying nothing.
pub(crate) fn park() -> ! {
    // SAFETY: as in `begin` — a parked hart takes no interrupt again.
    unsafe { riscv::register::sstatus::clear_sie() };
    loop {
        crate::arch::riscv::wait_for_interrupt();
    }
}

/// Leaves the space of the task this hart ran for the kernel's own, read from the kernel
/// task's scheduling record (a fixed address, no lock): the kernel's text, data and stacks are
/// mapped alike in every space, so the next instruction runs unchanged — only on mappings
/// nobody frees. Before the kernel task has a space (early boot) the hart is on the boot table,
/// which is as good.
#[cfg(not(feature = "selftest_no_satp"))]
fn enter_kernel_space() {
    let Some(view) =
        crate::sched::record::get(crate::types::Pid::KERNEL).and_then(|record| record.space())
    else {
        return;
    };
    // SAFETY: `view.satp` names the kernel's own root table, which lives as long as the kernel
    // and maps everything this hart executes and touches from here on at the same addresses.
    unsafe {
        core::arch::asm!("csrw satp, {0}", "sfence.vma x0, x0", in(reg) view.satp, options(nostack));
    }
}

#[cfg(feature = "selftest_no_satp")]
fn enter_kernel_space() {}

/// The fault line, on the console before the panic text: `<kind> hart=… sepc=… stval=…
/// scause=… ra=… sp=… a7=… a0=… a1=… a2=… sstatus=… satp=… pid=…`. `kind` is the fault's name
/// (`KPGF` a page fault, `KEXC` another exception, `KILL` an illegal instruction).
pub(crate) fn fault_line(kind: &str, dying: &Dying, frame: &super::TrapFrame, stval: usize) {
    let put = crate::hal::platform::console_write_byte;
    let text = |s: &str| s.bytes().for_each(put);
    let hex = |value: usize, digits: usize| {
        for shift in (0..digits).rev() {
            put(b"0123456789abcdef"[(value >> (shift * 4)) & 0xf]);
        }
    };
    text(kind);
    text(" hart=0x");
    hex(dying.cpu, 2);
    for (name, value) in [
        (" sepc=0x", frame.sepc),
        (" stval=0x", stval),
        (" scause=0x", frame.scause),
        (" ra=0x", frame.x[1]),
        (" sp=0x", frame.x[2]),
        (" a7=0x", frame.x[17]),
        (" a0=0x", frame.x[10]),
        (" a1=0x", frame.x[11]),
        (" a2=0x", frame.x[12]),
        (" sstatus=0x", frame.sstatus),
        (" satp=0x", dying.satp),
    ] {
        text(name);
        hex(value, 16);
    }
    #[cfg(feature = "trap_symbols")]
    if let Some((name, off)) = super::fault::nearest_symbol(frame.x[1]) {
        text(" sym=");
        // Up to 64 bytes of the name keep the line bounded.
        name.bytes().take(64).for_each(put);
        text("+0x");
        hex(off, 4);
    }
    // The task this hart ran, from the scheduler's per-hart word (no lock, no table).
    text(" pid=0x");
    hex(crate::sched::runs_here().as_index(), 4);
    put(b'\n');
}
