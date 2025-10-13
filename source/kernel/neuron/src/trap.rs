// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//! Trap handling: external ASM prologue/epilogue + safe Rust core,
//! HPM CSR emulation, SBI timer handling.

#![allow(clippy::identity_op)]

#[cfg(test)]
extern crate alloc;

use core::fmt::{self, Write};
use spin::Mutex;

use crate::syscall::{api, Args, Error as SysError, SyscallTable};

#[cfg(test)]
use alloc::string::String;

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
use sbi_rt as sbi;

// ——— include low-level vector from assembly (OS target only) ———
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
core::arch::global_asm!(
    include_str!("arch/riscv/trap.S"),
    TF_SIZE    = const core::mem::size_of::<TrapFrame>(),
    OFF_SEPC   = const 32*8,
    OFF_SSTATUS= const 33*8,
    OFF_SCAUSE = const 34*8,
    OFF_STVAL  = const 35*8,
);

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
extern "C" {
    fn __trap_vector();
}

// ——— diagnostics ———

static LAST_TRAP: Mutex<Option<TrapFrame>> = Mutex::new(None);

const INTERRUPT_FLAG: usize = usize::MAX - (usize::MAX >> 1);

// ——— HPM CSR emulation helpers ———

#[inline]
fn is_csr_op(inst: u32) -> bool {
    // SYSTEM opcode (0b1110011), funct3 in {001,010,011} => CSRRW/CSRRS/CSRRC
    (inst & 0x7f) == 0b111_0011 && matches!((inst >> 12) & 0x7, 0b001 | 0b010 | 0b011)
}
#[inline]
fn csr_num(inst: u32) -> u16 {
    ((inst >> 20) & 0x0fff) as u16
}
#[inline]
fn rd_index(inst: u32) -> usize {
    ((inst >> 7) & 0x1f) as usize
}

/// Emulate HPM (mhpmcounter{3..31}, mhpmcounterh{3..31}) reads/writes in S-mode by returning 0
/// and advancing sepc by 4. HPM CSRs are M-mode unless M enables access; on typical firmware they are illegal in S.
fn emulate_hpm_csr(frame: &mut TrapFrame, inst: u32) -> bool {
    if !is_csr_op(inst) {
        return false;
    }
    let csr = csr_num(inst);
    let is_hpm = (0x0B03..=0x0B1F).contains(&csr) || (0x0B83..=0x0B9F).contains(&csr);
    if !is_hpm {
        return false;
    }

    let rd = rd_index(inst);
    if rd != 0 {
        frame.set_x(rd, 0);
    } // read-as-zero
    frame.sepc = frame.sepc.wrapping_add(4);
    true
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
impl TrapFrame {
    #[inline]
    fn set_x(&mut self, rd: usize, value: usize) {
        if rd < 32 {
            self.x[rd] = value;
        }
    }
}

// ——— syscall path (unchanged API) ———

pub fn handle_ecall(frame: &mut TrapFrame, table: &SyscallTable, ctx: &mut api::Context<'_>) {
    record(frame);
    // a7 = syscall number; a0..a5 = args
    let number = frame.x[17]; // a7
    let args =
        Args::new([frame.x[10], frame.x[11], frame.x[12], frame.x[13], frame.x[14], frame.x[15]]);
    frame.x[10] = match table.dispatch(number, ctx, &args) {
        Ok(ret) => ret, // a0 = return
        Err(err) => encode_error(err),
    };
    frame.sepc = frame.sepc.wrapping_add(4);
}

fn encode_error(err: SysError) -> usize {
    match err {
        SysError::InvalidSyscall => usize::MAX,
        SysError::Capability(_) => usize::MAX - 1,
        SysError::Ipc(_) => usize::MAX - 2,
        SysError::Spawn(_) => usize::MAX - 3,
        SysError::Transfer(_) => usize::MAX - 4,
    }
}

pub fn record(frame: &TrapFrame) {
    *LAST_TRAP.lock() = Some(*frame);
}
pub fn last_trap() -> Option<TrapFrame> {
    *LAST_TRAP.lock()
}
#[inline]
pub fn is_interrupt(scause: usize) -> bool {
    scause & INTERRUPT_FLAG != 0
}

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

pub fn fmt_trap<W: Write>(frame: &TrapFrame, f: &mut W) -> fmt::Result {
    writeln!(f, " sepc=0x{:016x}", frame.sepc)?;
    writeln!(f, " scause=0x{:016x} ({})", frame.scause, describe_cause(frame.scause))?;
    writeln!(f, " stval=0x{:016x}", frame.stval)?;
    writeln!(f, " a0..a7 = {:016x?}", &frame.x[10..=17])
}

// ——— SBI timer utilities ———

/// Default tick in cycles (10 ms for 10 MHz mtimer on QEMU virt).
pub const DEFAULT_TICK_CYCLES: u64 = 100_000;

/// Arm S-mode timer via SBI for `now + delta_cycles`.
#[inline]
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
pub fn timer_arm(delta_cycles: u64) {
    let now = riscv::register::time::read() as u64;
    sbi::set_timer(now.wrapping_add(delta_cycles));
}

#[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
pub fn timer_arm(_delta_cycles: u64) {}

/// Install trap vector; call once during early boot (before enabling SIE).
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
pub unsafe fn install_trap_vector() {
    // SAFETY: must be called early and exactly once per hart; SSCRATCH becomes well-defined.
    unsafe {
        riscv::register::sscratch::write(0);
        riscv::register::stvec::write(
            __trap_vector as usize,
            riscv::register::mtvec::TrapMode::Direct,
        );
    }
}

#[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
pub unsafe fn install_trap_vector() {}

/// Enable supervisor timer interrupts after arming the first timer.
/// Gated behind `timer_irq` feature to avoid dead_code in default builds.
#[cfg(all(target_arch = "riscv64", target_os = "none", feature = "timer_irq"))]
pub unsafe fn enable_timer_interrupts() {
    use riscv::register::{sie, sstatus};
    // SAFETY: requires trap vector installed and first timer armed.
    unsafe {
        sie::set_stimer();
        sstatus::set_sie();
    }
}

// No non-OS stub; avoid dead_code in host builds

/// Disable supervisor timer interrupts.
/// Gated behind `timer_irq` feature to avoid dead_code in default builds.
#[cfg_attr(not(test), inline)]
#[cfg(all(target_arch = "riscv64", target_os = "none", feature = "timer_irq"))]
pub unsafe fn disable_timer_interrupts() {
    use riscv::register::{sie, sstatus};
    // SAFETY: caller must ensure trap vector is installed and interrupts are masked appropriately elsewhere when needed.
    unsafe {
        sstatus::clear_sie();
        sie::clear_stimer();
    }
}

// Intentionally no non-OS stub to avoid dead_code in host builds

// ——— Rust trap handler called from assembly ———

#[no_mangle]
extern "C" fn __trap_rust(frame: &mut TrapFrame) {
    if is_interrupt(frame.scause) {
        // Supervisor timer: rearm via SBI and return.
        const S_TIMER_INT: usize = 5;
        let code = frame.scause & (usize::MAX >> 1);
        if code == S_TIMER_INT {
            #[cfg(all(target_arch = "riscv64", target_os = "none"))]
            {
                let next = riscv::register::time::read() as u64 + DEFAULT_TICK_CYCLES;
                sbi::set_timer(next);
            }
        }
        return;
    }

    // Exception path
    const ILLEGAL_INSTRUCTION: usize = 2;
    let exc = frame.scause & (usize::MAX >> 1);
    if exc == ILLEGAL_INSTRUCTION {
        // Fetch the faulting instruction; CSR ops are 32-bit.
        let inst = unsafe { core::ptr::read_volatile(frame.sepc as *const u32) };
        // Also read the lower 16 bits in case the fault was on a compressed instruction
        let inst16 = unsafe { core::ptr::read_volatile(frame.sepc as *const u16) } as u32;
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        let stval_now = riscv::register::stval::read();
        #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
        let stval_now: usize = 0;
        if emulate_hpm_csr(frame, inst) {
            return;
        }
        // Fallthrough to diagnostics with valid inst
        {
            use core::fmt::Write as _;
            let mut u = crate::uart::raw_writer();
            // Check if sepc appears to be within executable text
            #[cfg(all(target_arch = "riscv64", target_os = "none"))]
            extern "C" {
                static __text_start: u8;
                static __text_end: u8;
            }
            #[cfg(all(target_arch = "riscv64", target_os = "none"))]
            let exec_ok = {
                let start = unsafe { &__text_start as *const u8 as usize };
                let end = unsafe { &__text_end as *const u8 as usize };
                frame.sepc >= start && frame.sepc < end
            };
            #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
            let exec_ok = true;
            let _ = write!(
                u,
                "EXC: scause=0x{:x} sepc=0x{:x} inst=0x{:08x} inst16=0x{:04x} stval=0x{:x} exec={}\n",
                frame.scause, frame.sepc, inst, inst16, stval_now, if exec_ok { "Y" } else { "N" }
            );
            if let Some((name, base)) = nearest_symbol(frame.sepc) {
                let _ =
                    write!(u, "EXC-S: {:016x} ~ {}+0x{:x}\n", frame.sepc, name, frame.sepc - base);
            }
            // Attempt to dump 16 bytes around sepc for debugging. Best-effort.
            let base = frame.sepc & !0xf;
            let mut bytes = [0u8; 16];
            let mut i = 0;
            while i < 16 {
                // SAFETY: best-effort diagnostic read; may fault on some addresses.
                let b = unsafe { core::ptr::read_volatile((base + i) as *const u8) };
                bytes[i] = b;
                i += 1;
            }
            let _ = write!(u, "EXC: bytes[@0x{:x}] =", base);
            for b in &bytes {
                let _ = write!(u, " {:02x}", *b);
            }
            let _ = write!(u, "\n");
        }
    } else {
        // For non-IllegalInstruction, avoid reading instruction (could fault again)
        {
            use core::fmt::Write as _;
            let mut u = crate::uart::raw_writer();
            #[cfg(all(target_arch = "riscv64", target_os = "none"))]
            let stval_now = riscv::register::stval::read();
            #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
            let stval_now: usize = 0;
            let _ = write!(
                u,
                "EXC: scause=0x{:x} sepc=0x{:x} stval=0x{:x}\n",
                frame.scause, frame.sepc, stval_now
            );
            let sp = frame.x[2];
            let _ = write!(u, "EXC2: sp=0x{:x}\n", sp);
        }
    }
    // Park the hart for diagnostics (do not reboot; LAST_TRAP can be read).
    record(frame);
    loop {
        riscv::asm::wfi();
    }
}

// ——— tests (host) ———
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
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
}

#[cfg(all(target_arch = "riscv64", target_os = "none", feature = "trap_symbols"))]
#[allow(dead_code)]
mod trap_symbols {
    include!(concat!(env!("OUT_DIR"), "/trap_symbols.rs"));
}
#[cfg(not(all(target_arch = "riscv64", target_os = "none", feature = "trap_symbols")))]
mod trap_symbols {
    #[allow(dead_code)]
    pub static TRAP_SYMBOLS: &[(usize, &str)] = &[];
}

fn nearest_symbol(_addr: usize) -> Option<(&'static str, usize)> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        // Binary search in sorted table if present
        let table = &trap_symbols::TRAP_SYMBOLS;
        if table.is_empty() {
            return None;
        }
        let mut lo = 0usize;
        let mut hi = table.len();
        while lo < hi {
            let mid = (lo + hi) / 2;
            if table[mid].0 <= _addr {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if lo == 0 {
            return None;
        }
        let (base, name) = table[lo - 1];
        Some((name, base))
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        None
    }
}
