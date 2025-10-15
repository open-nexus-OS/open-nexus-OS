// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//! Trap handling: external ASM prologue/epilogue + safe Rust core,
//! HPM CSR emulation, SBI timer handling.

#![allow(clippy::identity_op)]

#[cfg(test)]
extern crate alloc;

use core::fmt::{self, Write};
use spin::Mutex;

use crate::{
    mm::{AddressSpaceError, MapError},
    syscall::{api, Args, Error as SysError, SyscallTable},
    task,
};
use crate::{ipc, mm::AddressSpaceManager, sched::Scheduler};

#[cfg(test)]
use alloc::string::String;

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
#[allow(unused_imports)]
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
static TRAP_DIAG_COUNT: Mutex<usize> = Mutex::new(0);

// ——— minimal trap ring buffer (debug diagnostics) ———
const TRAP_RING_LEN: usize = 64;
static TRAP_RING: Mutex<[Option<TrapFrame>; TRAP_RING_LEN]> = Mutex::new([None; TRAP_RING_LEN]);
static TRAP_RING_IDX: Mutex<usize> = Mutex::new(0);

#[cfg_attr(not(all(target_arch = "riscv64", target_os = "none")), allow(dead_code))]
#[inline]
fn uart_write_hex(u: &mut crate::uart::RawUart, value: usize) {
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

#[cfg_attr(not(all(target_arch = "riscv64", target_os = "none")), allow(dead_code))]
#[inline]
fn uart_print_exc(scause: usize, sepc: usize, stval: usize) {
    let mut u = crate::uart::raw_writer();
    let _ = u.write_str("EXC: scause=0x");
    uart_write_hex(&mut u, scause);
    let _ = u.write_str(" sepc=0x");
    uart_write_hex(&mut u, sepc);
    let _ = u.write_str(" stval=0x");
    uart_write_hex(&mut u, stval);
    let _ = u.write_str("\n");
}

const INTERRUPT_FLAG: usize = usize::MAX - (usize::MAX >> 1);

// ——— minimal syscall environment for trap-time yield handling ———
struct TrapSysEnv {
    scheduler_addr: usize,
    tasks_addr: usize,
    spaces_addr: usize,
}

static TRAP_ENV: Mutex<Option<TrapSysEnv>> = Mutex::new(None);

/// Registers scheduler/task/router/address-space pointers used by the trap-time
/// ecall fastpath (yield only). Unsafe: caller must ensure single-hart use and
/// that the references remain valid for the lifetime of the kernel.
pub unsafe fn register_scheduler_env(
    scheduler: *mut Scheduler,
    tasks: *mut task::TaskTable,
    _router: *mut ipc::Router,
    spaces: *mut AddressSpaceManager,
) {
    *TRAP_ENV.lock() = Some(TrapSysEnv {
        scheduler_addr: scheduler as usize,
        tasks_addr: tasks as usize,
        spaces_addr: spaces as usize,
    });
}

// ——— HPM CSR emulation helpers ———

#[inline]
#[allow(dead_code)]
fn is_csr_op(inst: u32) -> bool {
    // SYSTEM opcode (0b1110011), funct3 in {001,010,011} => CSRRW/CSRRS/CSRRC
    (inst & 0x7f) == 0b111_0011 && matches!((inst >> 12) & 0x7, 0b001 | 0b010 | 0b011)
}
#[inline]
fn is_rdcycle_or_time(inst: u32) -> bool {
    // rdcycle/rdtime encodings are CSRRS with rs1=x0 and CSR=cycle/time
    if (inst & 0x7f) != 0b111_0011 { return false; }
    let funct3 = (inst >> 12) & 0x7;
    if funct3 != 0b010 { return false; }
    let csr = ((inst >> 20) & 0x0fff) as u16;
    csr == 0xC00 /*cycle*/ || csr == 0xC01 /*time*/
}
#[inline]
fn is_rdinstret(inst: u32) -> bool {
    if (inst & 0x7f) != 0b111_0011 { return false; }
    let funct3 = (inst >> 12) & 0x7;
    if funct3 != 0b010 { return false; }
    let csr = ((inst >> 20) & 0x0fff) as u16;
    csr == 0xC02 /*instret*/
}
#[inline]
#[allow(dead_code)]
fn csr_num(inst: u32) -> u16 {
    ((inst >> 20) & 0x0fff) as u16
}
#[inline]
#[allow(dead_code)]
fn rd_index(inst: u32) -> usize {
    ((inst >> 7) & 0x1f) as usize
}

/// Emulate HPM (mhpmcounter{3..31}, mhpmcounterh{3..31}) reads/writes in S-mode by returning 0
/// and advancing sepc by 4. HPM CSRs are M-mode unless M enables access; on typical firmware they are illegal in S.
#[allow(dead_code)]
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

#[allow(dead_code)]
pub fn handle_ecall(frame: &mut TrapFrame, table: &SyscallTable, ctx: &mut api::Context<'_>) {
    // Save current frame into the current task before handling the syscall.
    let old_pid = ctx.tasks.current_pid();
    if let Some(task) = ctx.tasks.task_mut(old_pid) {
        *task.frame_mut() = *frame;
    }
    record(frame);
    // a7 = syscall number; a0..a5 = args
    let number = frame.x[17]; // a7
    let args =
        Args::new([frame.x[10], frame.x[11], frame.x[12], frame.x[13], frame.x[14], frame.x[15]]);
    let ret = match table.dispatch(number, ctx, &args) {
        Ok(ret) => ret,
        Err(err) => encode_error(err),
    };
    // Advance caller PC and store return in its saved frame (a0).
    if let Some(task) = ctx.tasks.task_mut(old_pid) {
        let f = task.frame_mut();
        f.sepc = f.sepc.wrapping_add(4);
        f.x[10] = ret;
        // Minimal debug: show ecall return site and value
        #[allow(unused_variables)]
        {
            use core::fmt::Write as _;
            let mut u = crate::uart::raw_writer();
            let _ = write!(u, "ECALL-R: pid={} sepc=0x{:x} ret=0x{:x}\n", old_pid, f.sepc, ret);
        }
    }
    // Load the next task's frame into the live trap frame.
    let new_pid = ctx.tasks.current_pid();
    if let Some(task) = ctx.tasks.task_mut(new_pid) {
        *frame = *task.frame();
    }
}

const EPERM: usize = 1;
const ENOMEM: usize = 12;
const EINVAL: usize = 22;
const ENOSPC: usize = 28;
const ENOSYS: usize = 38;

#[allow(dead_code)]
fn encode_error(err: SysError) -> usize {
    match err {
        SysError::InvalidSyscall => errno(ENOSYS),
        SysError::Capability(_) => errno(EPERM),
        SysError::Ipc(_) => errno(EINVAL),
        SysError::Spawn(spawn) => spawn_errno(&spawn),
        SysError::Transfer(_) => errno(EPERM),
        SysError::AddressSpace(as_err) => address_space_errno(&as_err),
    }
}

#[allow(dead_code)]
fn spawn_errno(err: &task::SpawnError) -> usize {
    use task::SpawnError::*;
    match err {
        InvalidParent | InvalidEntryPoint | InvalidStackPointer => errno(EINVAL),
        BootstrapNotEndpoint => errno(EPERM),
        Capability(_) => errno(EPERM),
        Ipc(_) => errno(EINVAL),
        AddressSpace(as_err) => address_space_errno(as_err),
        StackExhausted => errno(ENOMEM),
    }
}

#[allow(dead_code)]
fn address_space_errno(err: &AddressSpaceError) -> usize {
    match err {
        AddressSpaceError::InvalidHandle | AddressSpaceError::InvalidArgs => errno(EINVAL),
        AddressSpaceError::AsidExhausted => errno(ENOSPC),
        AddressSpaceError::InUse => errno(EPERM),
        AddressSpaceError::Unsupported => errno(ENOSYS),
        AddressSpaceError::Mapping(MapError::PermissionDenied) => errno(EPERM),
        AddressSpaceError::Mapping(_) => errno(EINVAL),
    }
}

const fn errno(code: usize) -> usize {
    (-(code as isize)) as usize
}

pub fn record(frame: &TrapFrame) {
    *LAST_TRAP.lock() = Some(*frame);
    // Push into ring
    let mut idx = TRAP_RING_IDX.lock();
    let mut ring = TRAP_RING.lock();
    ring[*idx % TRAP_RING_LEN] = Some(*frame);
    *idx = (*idx + 1) % TRAP_RING_LEN;
}
pub fn last_trap() -> Option<TrapFrame> {
    *LAST_TRAP.lock()
}
#[inline]
pub fn is_interrupt(scause: usize) -> bool {
    scause & INTERRUPT_FLAG != 0
}

#[cfg_attr(not(all(target_arch = "riscv64", target_os = "none")), allow(dead_code))]
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

#[cfg_attr(not(all(target_arch = "riscv64", target_os = "none")), allow(dead_code))]
pub fn fmt_trap<W: Write>(frame: &TrapFrame, f: &mut W) -> fmt::Result {
    writeln!(f, " sepc=0x{:016x}", frame.sepc)?;
    writeln!(f, " scause=0x{:016x} ({})", frame.scause, describe_cause(frame.scause))?;
    writeln!(f, " stval=0x{:016x}", frame.stval)?;
    writeln!(f, " a0..a7 = {:016x?}", &frame.x[10..=17])
}

// ——— SBI timer utilities ———

/// Default tick in cycles (10 ms for 10 MHz mtimer on QEMU virt).
#[cfg_attr(not(all(target_arch = "riscv64", target_os = "none")), allow(dead_code))]
pub const DEFAULT_TICK_CYCLES: u64 = 100_000;

/// Arm S-mode timer via SBI for `now + delta_cycles`.
#[inline]
#[allow(dead_code)]
#[cfg(all(target_arch = "riscv64", target_os = "none", feature = "timer_irq"))]
pub fn timer_arm(delta_cycles: u64) {
    let now = riscv::register::time::read() as u64;
    sbi::set_timer(now.wrapping_add(delta_cycles));
}

#[allow(dead_code)]
#[cfg(not(all(target_arch = "riscv64", target_os = "none", feature = "timer_irq")))]
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
#[allow(dead_code)]
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
#[allow(dead_code)]
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
    // Liveness heartbeat on every trap entry
    crate::liveness::bump();
    if is_interrupt(frame.scause) {
        // Supervisor timer: rearm via SBI and return.
        const S_TIMER_INT: usize = 5;
        let code = frame.scause & (usize::MAX >> 1);
        if code == S_TIMER_INT {
            #[cfg(all(target_arch = "riscv64", target_os = "none", feature = "timer_irq"))]
            {
                let next = riscv::register::time::read() as u64 + DEFAULT_TICK_CYCLES;
                sbi::set_timer(next);
            }
        }
        return;
    }

    // Exception path (print limited diagnostics only for exceptions)
    const ILLEGAL_INSTRUCTION: usize = 2;
    const ECALL_SMODE: usize = 9;
    let exc = frame.scause & (usize::MAX >> 1);
    if !is_interrupt(frame.scause) {
        let mut count = TRAP_DIAG_COUNT.lock();
        if *count < 8 {
            use core::fmt::Write as _;
            let mut u = crate::uart::raw_writer();
            let _ = write!(u, "EXC: scause=0x{:x} sepc=0x{:x}\n", frame.scause as u64, frame.sepc as u64);
            #[cfg(feature = "trap_symbols")]
            if let Some((name, base)) = nearest_symbol(frame.sepc) {
                let _ = write!(u, "EXC-S: {:016x} ~ {}+0x{:x}\n", frame.sepc, name, frame.sepc - base);
            }
            #[cfg(all(target_arch = "riscv64", target_os = "none"))]
            {
                let stval_now = riscv::register::stval::read();
                let _ = write!(u, "EXC: stval=0x{:x}\n", stval_now as u64);
            }
            *count += 1;
        }
    }
    if exc == ECALL_SMODE {
        // Minimal in-kernel syscall handling: SYSCALL_YIELD only.
        const SYSCALL_YIELD: usize = crate::syscall::SYSCALL_YIELD;
        let num = frame.x[17];
        if num == SYSCALL_YIELD {
            // SAFETY: registered once during bring-up; single-core.
            if let Some(env) = TRAP_ENV.lock().as_ref() {
                let scheduler = unsafe { &mut *(env.scheduler_addr as *mut Scheduler) };
                let tasks = unsafe { &mut *(env.tasks_addr as *mut task::TaskTable) };
                let spaces = unsafe { &mut *(env.spaces_addr as *mut AddressSpaceManager) };
                // Persist caller frame and advance past ecall
                let old = tasks.current_pid();
                {
                    use core::fmt::Write as _;
                    let mut u = crate::uart::raw_writer();
                    let _ = write!(u, "ECALL-FP: old={} a7={}\n", old, num);
                }
                if let Some(t) = tasks.task_mut(old) {
                    *t.frame_mut() = *frame;
                    let f = t.frame_mut();
                    f.sepc = f.sepc.wrapping_add(4);
                }
                // Ensure the yielding task is re-enqueued even if `current` was not set.
                scheduler.enqueue(old as u32, crate::sched::QosClass::Normal);
                if let Some(next) = scheduler.schedule_next() {
                    tasks.set_current(next as task::Pid);
                    {
                        use core::fmt::Write as _;
                        let mut u = crate::uart::raw_writer();
                        let satp_now = riscv::register::satp::read().bits();
                        let _ = write!(u, "SW: old={} -> next={} satp=0x{:x}\n", old, next, satp_now);
                    }
                    if let Some(t) = tasks.task(next as task::Pid) {
                        if let Some(h) = t.address_space() {
                            let _ = spaces.activate(h);
                            // Fail-fast: SATP must be non-zero after activation
                            #[cfg(all(target_arch = "riscv64", target_os = "none"))]
                            {
                                let satp_now = riscv::register::satp::read().bits();
                                if satp_now == 0 {
                                    panic!("YF: satp not activated (0) for pid={}", next);
                                }
                            }
                        }
                    }
                    if let Some(t) = tasks.task_mut(next as task::Pid) {
                        *frame = *t.frame();
                        // Fail-fast: loaded frame must have a plausible PC/stack
                        if frame.sepc == 0 || frame.x[2] == 0 {
                            use core::fmt::Write as _;
                            let mut u = crate::uart::raw_writer();
                            let _ = write!(u, "YF-E: invalid frame pid={} sepc=0x{:x} sp=0x{:x}\n", next, frame.sepc, frame.x[2]);
                            panic!("YF: invalid frame loaded");
                        }
                        {
                            use core::fmt::Write as _;
                            let mut u = crate::uart::raw_writer();
                            let _ = write!(
                                u,
                                "YF: switch {}->{} sepc=0x{:x} sp=0x{:x}\n",
                                old,
                                next,
                                frame.sepc,
                                frame.x[2]
                            );
                        }
                    }
                } else {
                    use core::fmt::Write as _;
                    let mut u = crate::uart::raw_writer();
                    let _ = write!(u, "SW: schedule_next none (old={})\n", old);
                }
                return;
            }
        }
        // Unregistered or unsupported syscall: encode ENOSYS and advance PC.
        frame.x[10] = errno(ENOSYS);
        frame.sepc = frame.sepc.wrapping_add(4);
        return;
    }
    if exc == ILLEGAL_INSTRUCTION {
        // Decode from stval (avoid touching faulting PC); emulate only whitelisted CSR reads.
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        let inst = riscv::register::stval::read() as u32;
        #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
        let inst: u32 = 0;
        if is_rdcycle_or_time(inst) || is_rdinstret(inst) {
            let rd = ((inst >> 7) & 0x1f) as usize;
            if rd != 0 { frame.set_x(rd, 0); }
            record(frame);
            frame.sepc = frame.sepc.wrapping_add(4);
            return;
        }
        // Emit precise diagnostics: sepc, ra, stval, and instruction bytes at sepc.
        {
            use core::fmt::Write as _;
            let mut u = crate::uart::raw_writer();
            #[cfg(all(target_arch = "riscv64", target_os = "none"))]
            let stval_now = riscv::register::stval::read();
            #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
            let stval_now: usize = 0;
            let _ = write!(u, "ILLEGAL-D: sepc=0x{:x} ra=0x{:x} stval=0x{:x}\n", frame.sepc, frame.x[1], stval_now);
            // Best-effort fetch of instruction bytes at sepc
            let i16 = unsafe { core::ptr::read_volatile(frame.sepc as *const u16) } as u16;
            let i32 = unsafe { core::ptr::read_volatile(frame.sepc as *const u32) } as u32;
            let _ = write!(u, "ILLEGAL-D: inst16=0x{:04x} inst32=0x{:08x}\n", i16, i32);
            // Extra: dump PTE flags for sepc page if available (debug aid)
            #[cfg(all(target_arch = "riscv64", target_os = "none"))]
            {
                // Walk the active SATP to locate the PTE for the faulting page and dump flags.
                fn vpn_indices_sv39(va: usize) -> [usize; 3] {
                    let vpn0 = (va >> 12) & 0x1ff;
                    let vpn1 = (va >> 21) & 0x1ff;
                    let vpn2 = (va >> 30) & 0x1ff;
                    [vpn2, vpn1, vpn0] // hardware order: L2->L1->L0
                }
                let satp_now = riscv::register::satp::read().bits();
                let ppn = satp_now & ((1 << 44) - 1);
                if ppn == 0 {
                    let page_va = frame.sepc & !(crate::mm::PAGE_SIZE - 1);
                    let _ = write!(u, "ILLEGAL-D: satp=0x{:x} page=0x{:x} (ppn=0)\n", satp_now, page_va);
                } else {
                    let mut table = (ppn << 12) as *const usize;
                let indices = vpn_indices_sv39(frame.sepc);
                let mut pte: usize = 0;
                let mut found = true;
                for (level, idx) in indices.iter().enumerate() {
                    let entry_ptr = unsafe { table.add(*idx) };
                    let entry = unsafe { core::ptr::read_volatile(entry_ptr) };
                    if entry & 1 == 0 { found = false; break; }
                    let is_leaf = (entry & ((1<<1)|(1<<2)|(1<<3))) != 0; // any of R/W/X
                    if level == 2 {
                        if !is_leaf { found = false; break; }
                        pte = entry; break;
                    }
                    if is_leaf { found = false; break; }
                    let next_ppn = (entry >> 10) & ((1<<44)-1);
                    table = (next_ppn << 12) as *const usize;
                }
                    if found {
                        let flags = pte & 0x3ff;
                        let _ = write!(u, "ILLEGAL-D: satp=0x{:x} pte=0x{:x} flags=0x{:x}\n", satp_now, pte, flags);
                    } else {
                        let page_va = frame.sepc & !(crate::mm::PAGE_SIZE - 1);
                        let _ = write!(u, "ILLEGAL-D: satp=0x{:x} page=0x{:x} (unmapped or non-leaf)\n", satp_now, page_va);
                    }
                }
            }
        }
        record(frame);
        panic!("ILLEGAL sepc=0x{:x}", frame.sepc);
    } else {
        // Non-Illegal exceptions: emit minimal diagnostics and enforce null-deref sentinel.
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        {
            let stval_now = riscv::register::stval::read();
            uart_print_exc(frame.scause, frame.sepc, stval_now);
            if stval_now < 0x1000 {
                panic!("NULL-DEREF: sepc=0x{:x} stval=0x{:x}", frame.sepc, stval_now);
            }
        }
        record(frame);
        panic!("EXC: scause=0x{:x} sepc=0x{:x}", frame.scause, frame.sepc);
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

#[allow(dead_code)]
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
