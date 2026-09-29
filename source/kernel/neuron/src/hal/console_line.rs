// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The console funnel's line discipline on the target (TASK-0327B P4 H0d): binds the
//! pure, host-tested `crate::console_line` to `platform::console_write_byte`. Each hart gathers
//! its line in its own buffer and sends it at `\n` as ONE unit — ring and UART bytes, in order —
//! through the gate (`LineOwner`), which is held only while pure console bytes go out. Nothing
//! ever waits for a hart that is doing anything else, so no kernel lock can couple with the
//! console: the first cut (a hart OWNING the line from its first byte) did exactly that on
//! QEMU — IPC round trips of 275 µs against a 64 µs budget, statefs writes past their budget —
//! because a waiter could be holding a lock the owner needed. A hart `tp` does not name yet
//! (the single-hart early boot) and a re-entered hart (a trap mid-push on the same hart) write
//! through byte by byte, as before. The panic path flushes its own hart's line first, so a
//! trapped death still ends at its last byte; a silent death costs at most one partial line
//! per hart in the ring (RFC-0107 note).
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `crate::console_line` (host); the SMP lanes' ladder greps; the board lane

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, Ordering};

use crate::console_line::{LineBuf, LineOwner};
use crate::smp::MAX_CPUS;

/// The gate a unit goes out through — one hart's whole line at a time.
static GATE: LineOwner = LineOwner::new();

/// How long a hart waits at the gate, and the age at which a held gate counts as abandoned
/// (a hart that died mid-unit): a full 256-byte unit at the slowest console rate is ~22 ms.
const WAIT_NS: u64 = 30_000_000;

/// A hart's line buffer plus its re-entry latch.
struct HartLine {
    line: UnsafeCell<LineBuf>,
    busy: AtomicBool,
}

// SAFETY: `line` is touched only by its own hart, and only while `busy` is held by that hart's
// outermost push (a trap that re-enters mid-push sees `busy` and writes through, never touching
// the buffer). `busy` itself is an atomic.
unsafe impl Sync for HartLine {}

const fn hart_line() -> HartLine {
    HartLine { line: UnsafeCell::new(LineBuf::new()), busy: AtomicBool::new(false) }
}

/// A const item, so the array below is `MAX_CPUS` fresh buffers whatever that count is.
const EMPTY_LINE: HartLine = hart_line();

static LINES: [HartLine; MAX_CPUS] = [EMPTY_LINE; MAX_CPUS];

/// Emits `byte` as part of this hart's line: gathered, then sent as one unit at `\n`. `emit` is
/// the ring-then-UART path for one byte.
pub fn with_line(byte: u8, emit: impl Fn(u8)) {
    let Some(hart) = crate::smp::cpu_from_hart_local_tp().map(|cpu| cpu.as_index()) else {
        emit(byte);
        return;
    };
    let Some(slot) = LINES.get(hart) else {
        emit(byte);
        return;
    };
    if slot.busy.swap(true, Ordering::Acquire) {
        // Re-entered on the same hart mid-push: write through, never touch the buffer.
        emit(byte);
        return;
    }
    // SAFETY: `busy` is ours until the store below; no other path touches this hart's buffer.
    let line = unsafe { &mut *slot.line.get() };
    if line.push(byte) {
        send_unit(hart, line.take(), &emit);
    }
    slot.busy.store(false, Ordering::Release);
}

/// Sends the current hart's gathered bytes now, whole — the panic path's first act, so the
/// trace ends at the dying hart's last byte.
pub fn flush_current_hart(emit: impl Fn(u8)) {
    let Some(hart) = crate::smp::cpu_from_hart_local_tp().map(|cpu| cpu.as_index()) else {
        return;
    };
    let Some(slot) = LINES.get(hart) else {
        return;
    };
    if slot.busy.swap(true, Ordering::Acquire) {
        // A panic mid-push on this hart: the buffer is in use above us; the bytes it holds
        // are the line being written, and the panic text follows them raw.
        return;
    }
    // SAFETY: as in `with_line`.
    let line = unsafe { &mut *slot.line.get() };
    if !line.is_empty() {
        send_unit(hart, line.take(), &emit);
    }
    slot.busy.store(false, Ordering::Release);
}

/// One unit through the gate: bounded wait, takeover of an abandoned gate, write-through past
/// the bound — never a wait without end, and nothing but console bytes under the gate.
fn send_unit(hart: usize, unit: &[u8], emit: &impl Fn(u8)) {
    let bound = super::platform::ns_to_ticks(WAIT_NS);
    let _ = GATE.begin(hart, bound, crate::arch::riscv::read_time);
    for &b in unit {
        emit(b);
    }
    GATE.end(hart);
}
