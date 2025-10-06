// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Trap handling primitives.

use core::fmt::{self, Write};

use spin::Mutex;

use crate::syscall::{api, Args, Error as SysError, SyscallTable};

static LAST_TRAP: Mutex<Option<TrapFrame>> = Mutex::new(None);
const INTERRUPT_FLAG: usize = usize::MAX - (usize::MAX >> 1);

/// Saved register state for an S-mode trap.
#[derive(Clone, Copy, Default)]
pub struct TrapFrame {
    /// Arguments registers a0-a7.
    pub a: [usize; 8],
    /// Saved program counter.
    pub sepc: usize,
    /// Trap cause as reported by the CPU.
    pub scause: usize,
    /// Trap value register conveying faulting address or instruction bits.
    pub stval: usize,
}

/// Handles an ECALL originating from user mode.
pub fn handle_ecall(frame: &mut TrapFrame, table: &SyscallTable, ctx: &mut api::Context<'_>) {
    record(frame);
    let number = frame.a[7];
    let args = Args::new([frame.a[0], frame.a[1], frame.a[2], frame.a[3], frame.a[4], frame.a[5]]);
    match table.dispatch(number, ctx, &args) {
        Ok(ret) => frame.a[0] = ret,
        Err(err) => frame.a[0] = encode_error(err),
    }
    frame.sepc = frame.sepc.wrapping_add(4);
}

fn encode_error(err: SysError) -> usize {
    match err {
        SysError::InvalidSyscall => usize::MAX,
        SysError::Capability(_) => usize::MAX - 1,
        SysError::Ipc(_) => usize::MAX - 2,
    }
}

/// Records the latest trap frame for diagnostic purposes.
pub fn record(frame: &TrapFrame) {
    *LAST_TRAP.lock() = Some(*frame);
}

/// Returns the most recently recorded trap frame if available.
#[allow(dead_code)]
pub fn last_trap() -> Option<TrapFrame> {
    *LAST_TRAP.lock()
}

/// Determines whether the given `scause` represents an interrupt.
#[inline]
pub fn is_interrupt(scause: usize) -> bool {
    scause & INTERRUPT_FLAG != 0
}

/// Provides a human readable description of the trap cause.
pub fn describe_cause(scause: usize) -> &'static str {
    let code = scause & (usize::MAX >> 1);
    if is_interrupt(scause) {
        match code {
            1 => "SupervisorSoftInt",
            5 => "SupervisorTimerInt",
            9 => "SupervisorExternalInt",
            _ => "Interrupt",
        }
    } else {
        match code {
            0 => "InstructionAddressMisaligned",
            1 => "InstructionAccessFault",
            2 => "IllegalInstruction",
            3 => "Breakpoint",
            4 => "LoadAddressMisaligned",
            5 => "LoadAccessFault",
            6 => "StoreAMOAddressMisaligned",
            7 => "StoreAMOAccessFault",
            8 => "EnvironmentCallFromUMode",
            9 => "EnvironmentCallFromSMode",
            12 => "InstructionPageFault",
            13 => "LoadPageFault",
            15 => "StoreAMOPageFault",
            _ => "Exception",
        }
    }
}

/// Formats the trap registers for diagnostics.
#[allow(dead_code)]
pub fn fmt_trap<W: Write>(frame: &TrapFrame, f: &mut W) -> fmt::Result {
    writeln!(f, " sepc=0x{:016x}", frame.sepc)?;
    writeln!(f, " scause=0x{:016x} ({})", frame.scause, describe_cause(frame.scause))?;
    writeln!(f, " stval=0x{:016x}", frame.stval)?;
    writeln!(f, " a0=0x{:016x} a1=0x{:016x} a2=0x{:016x} a3=0x{:016x}", frame.a[0], frame.a[1], frame.a[2], frame.a[3])?;
    writeln!(f, " a4=0x{:016x} a5=0x{:016x} a6=0x{:016x} a7=0x{:016x}", frame.a[4], frame.a[5], frame.a[6], frame.a[7])
}

#[cfg(not(test))]
#[no_mangle]
extern "C" fn __trap_vector() -> ! {
    loop {
        crate::arch::riscv::wait_for_interrupt();
    }
}
