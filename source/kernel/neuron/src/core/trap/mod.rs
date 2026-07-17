// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//! CONTEXT: Trap handling: external ASM prologue/epilogue + safe Rust core, HPM CSR emulation, SBI timer handling
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU selftests + boot markers (trap/panic paths exercised in smoke runs)
//! PUBLIC API: install_runtime(), register_trap_domain(), TrapDomainId
//! DEPENDS_ON: sched::Scheduler, task::TaskTable, ipc::Router, mm::AddressSpaceManager, SyscallTable
//! INVARIANTS: Trap ABI/prologue stable; ECALL dispatch IDs stable; minimal UART in trap context
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

#![allow(clippy::identity_op)]

extern crate alloc;

use core::fmt::{self, Write};
use core::ptr::NonNull;
use spin::Mutex;

use crate::{hal::Timer, ipc, mm::AddressSpaceManager, sched::Scheduler};
use crate::{
    mm::{AddressSpaceError, MapError},
    syscall::{api, Args, Error as SysError, SyscallTable},
    task,
};

#[cfg(test)]
use alloc::string::String;

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
#[allow(unused_imports)]
use sbi_rt as sbi;

// ——— include low-level vector from assembly (OS target only) ———
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
core::arch::global_asm!(
    include_str!("../../arch/riscv/trap.S"),
    TF_SIZE    = const core::mem::size_of::<TrapFrame>(),
    OFF_SEPC   = const core::mem::offset_of!(TrapFrame, sepc),
    OFF_SSTATUS= const core::mem::offset_of!(TrapFrame, sstatus),
    OFF_SCAUSE = const core::mem::offset_of!(TrapFrame, scause),
    OFF_STVAL  = const core::mem::offset_of!(TrapFrame, stval),
    // HartLocal ABI (sscratch points at the executing hart's block):
    HL_TRAP_TOP   = const core::mem::offset_of!(crate::smp::HartLocal, trap_stack_top),
    HL_SCRATCH_T1 = const core::mem::offset_of!(crate::smp::HartLocal, scratch_t1),
    HL_SCRATCH_SP = const core::mem::offset_of!(crate::smp::HartLocal, scratch_sp),
);

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
extern "C" {
    fn __trap_vector();
}

#[cfg_attr(not(all(target_arch = "riscv64", target_os = "none")), allow(dead_code))]
#[inline]
pub fn uart_write_hex(u: &mut crate::uart::RawUart, value: usize) {
    let nibbles = core::mem::size_of::<usize>() * 2;
    let lut = b"0123456789abcdef";
    let mut i = nibbles;
    while i > 0 {
        i -= 1;
        let shift = i * 4;
        let nib = ((value >> shift) & 0xF) as u8;
        let ch = lut[nib as usize] as char;
        let buf = [ch as u8];
        let s = unsafe { core::str::from_utf8_unchecked(&buf) };
        let _ = u.write_str(s);
    }
}

#[cfg(feature = "debug_uart")]
macro_rules! uart_dbg_block {
    ($body:block) => {
        $body
    };
}

#[cfg(not(feature = "debug_uart"))]
macro_rules! uart_dbg_block {
    ($body:block) => {};
}

#[cfg(feature = "debug_uart")]
const ECALL_LOG_LIMIT: usize = 512;
#[cfg(feature = "debug_uart")]
static ECALL_LOG_COUNT: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

#[cfg(feature = "debug_uart")]
fn ecall_log<F>(f: F)
where
    F: FnOnce(&mut crate::uart::RawUart),
{
    use core::sync::atomic::Ordering;

    if ECALL_LOG_COUNT.load(Ordering::Relaxed) >= ECALL_LOG_LIMIT {
        return;
    }
    let prev = ECALL_LOG_COUNT.fetch_add(1, Ordering::Relaxed);
    if prev >= ECALL_LOG_LIMIT {
        return;
    }
    let mut u = crate::uart::raw_writer();
    f(&mut u);
}

// ——— trap frame ———

/// Saved register state for an S-mode trap.
/// Must match `arch/riscv/trap.S` save/restore layout.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TrapFrame {
    /// x0..x31 (x0 is always 0; we never write it).
    pub x: [usize; 32],
    pub sepc: usize,
    pub sstatus: usize,
    pub scause: usize,
    pub stval: usize,
}

const _: [(); core::mem::size_of::<usize>() * 32] = [(); core::mem::offset_of!(TrapFrame, sepc)];
impl TrapFrame {
    /// Const zero frame (for hart-local resume slots).
    pub(crate) const EMPTY: TrapFrame =
        TrapFrame { x: [0; 32], sepc: 0, sstatus: 0, scause: 0, stval: 0 };

    #[inline]
    fn set_x(&mut self, rd: usize, value: usize) {
        if rd < 32 {
            self.x[rd] = value;
        }
    }
}

// Mechanical split of the former single-file trap.rs (god-file split).
// The glob re-exports keep every item reachable under `crate::trap::*` at its
// original visibility; submodule-private helpers are widened to pub(super) only.
// NOTE: `uart_dbg_block!`/`ecall_log` above must stay ABOVE these `mod` items
// (macro_rules textual scope descends into the submodules).
mod fault;
mod handler;
pub mod budgets;
mod runtime;

pub use fault::*;
// `handle_ecall` (pub, #[allow(dead_code)]) has no external callers yet; keep
// the path `crate::trap::handle_ecall` stable without tripping deny(warnings).
#[allow(unused_imports)]
pub use handler::*;
pub use runtime::*;

// ——— tests (host) ———
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[cfg(any(debug_assertions, feature = "trap_ring"))]
    fn record_and_query_last_trap() {
        let mut frame = TrapFrame::default();
        frame.sepc = 0x1000;
        record(&frame);
        let recorded = last_trap().expect("trap stored");
        assert_eq!(recorded.sepc, 0x1000);
    }
    #[test]
    fn fmt_includes_registers() {
        let mut frame = TrapFrame::default();
        frame.x[10..=17].copy_from_slice(&[1; 8]);
        frame.sepc = 0x2000;
        frame.scause = 9;
        frame.stval = 0x3000;
        let mut out = String::new();
        fmt_trap(&frame, &mut out).unwrap();
        assert!(out.contains("sepc"));
        assert!(out.contains("scause"));
        assert!(out.contains("a0..a7"));
    }

    #[test]
    fn trap_runtime_access_is_boot_hart_only() {
        assert!(trap_runtime_access_allowed_for_cpu(crate::types::CpuId::BOOT));
        assert!(!trap_runtime_access_allowed_for_cpu(crate::types::CpuId::from_raw(1)));
    }
}
