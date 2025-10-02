// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Trap handling primitives.

use crate::{
    arch::riscv,
    syscall::{self, api, Args, Error as SysError, SyscallTable},
};

/// Saved register state for an S-mode trap.
#[derive(Default)]
pub struct TrapFrame {
    /// Arguments registers a0-a7.
    pub a: [usize; 8],
    /// Saved program counter.
    pub sepc: usize,
    /// Trap cause as reported by the CPU.
    pub scause: usize,
}

/// Handles an ECALL originating from user mode.
pub fn handle_ecall(frame: &mut TrapFrame, table: &SyscallTable, ctx: &mut api::Context<'_>) {
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

#[cfg(not(test))]
#[no_mangle]
extern "C" fn __trap_vector() -> ! {
    loop {
        riscv::wait_for_interrupt();
    }
}
