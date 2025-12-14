// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//! CONTEXT: Trap handling: external ASM prologue/epilogue + safe Rust core, HPM CSR emulation, SBI timer handling
//! OWNERS: @kernel-team
//! PUBLIC API: install_runtime(), register_trap_domain(), TrapDomainId
//! DEPENDS_ON: sched::Scheduler, task::TaskTable, ipc::Router, mm::AddressSpaceManager, SyscallTable
//! INVARIANTS: Trap ABI/prologue stable; ECALL dispatch IDs stable; minimal UART in trap context
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

#![allow(clippy::identity_op)]

extern crate alloc;

use alloc::vec::Vec;
use core::fmt::{self, Write};
use core::ptr::NonNull;
use spin::Mutex;

use crate::{ipc, mm::AddressSpaceManager, sched::Scheduler};
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

#[cfg(any(debug_assertions, feature = "trap_ring"))]
static LAST_TRAP: Mutex<Option<TrapFrame>> = Mutex::new(None);
#[cfg(any(debug_assertions, feature = "trap_ring"))]
#[allow(dead_code)]
static TRAP_DIAG_COUNT: Mutex<usize> = Mutex::new(0);

#[cfg(any(debug_assertions, feature = "trap_ring"))]
const TRAP_RING_LEN: usize = 64;
#[cfg(any(debug_assertions, feature = "trap_ring"))]
static TRAP_RING: Mutex<[Option<TrapFrame>; TRAP_RING_LEN]> = Mutex::new([None; TRAP_RING_LEN]);
#[cfg(any(debug_assertions, feature = "trap_ring"))]
static TRAP_RING_IDX: Mutex<usize> = Mutex::new(0);

#[cfg_attr(
    not(all(target_arch = "riscv64", target_os = "none")),
    allow(dead_code)
)]
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

#[cfg_attr(
    not(all(target_arch = "riscv64", target_os = "none")),
    allow(dead_code)
)]
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

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
fn dump_user_stack_for_task(task: &task::Task, spaces: &AddressSpaceManager, sp: usize) {
    const STACK_WORDS: usize = 8;
    if sp == 0 {
        return;
    }
    let handle = match task.address_space() {
        Some(h) => h,
        None => return,
    };
    let space = match spaces.get(handle) {
        Ok(space) => space,
        Err(_) => return,
    };
    let page_table = space.page_table();
    const UART_BASE: usize = 0x1000_0000;
    const UART_TX: usize = 0x0;
    const UART_LSR: usize = 0x5;
    const LSR_TX_IDLE: u8 = 1 << 5;
    unsafe {
        let write_byte = |b: u8| {
            while core::ptr::read_volatile((UART_BASE + UART_LSR) as *const u8) & LSR_TX_IDLE == 0 {
            }
            core::ptr::write_volatile((UART_BASE + UART_TX) as *mut u8, b);
        };
        for index in 0..STACK_WORDS {
            for &b in b"[USER-PF] stack +" {
                write_byte(b);
            }
            let offset = index * core::mem::size_of::<usize>();
            write_byte(b'0');
            write_byte(b'x');
            for shift in (0..4).rev() {
                let nibble = ((offset >> (shift * 4)) & 0xf) as u8;
                let ch = if nibble < 10 {
                    b'0' + nibble
                } else {
                    b'a' + (nibble - 10)
                };
                write_byte(ch);
            }
            for &b in b" = " {
                write_byte(b);
            }
            let addr = sp.wrapping_add(offset);
            if let Some(pa) = page_table.translate(addr) {
                let value = core::ptr::read_volatile(pa as *const usize);
                write_byte(b'0');
                write_byte(b'x');
                for shift in (0..16).rev() {
                    let nibble = ((value >> (shift * 4)) & 0xf) as u8;
                    let ch = if nibble < 10 {
                        b'0' + nibble
                    } else {
                        b'a' + (nibble - 10)
                    };
                    write_byte(ch);
                }
            } else {
                for &b in b"<hole>" {
                    write_byte(b);
                }
            }
            write_byte(b'\n');
        }
    }
}

const INTERRUPT_FLAG: usize = usize::MAX - (usize::MAX >> 1);

/// Identifier selecting a trap domain (e.g. syscall table) for a task.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrapDomainId(pub(crate) usize);

impl Default for TrapDomainId {
    fn default() -> Self {
        TrapDomainId(0)
    }
}

#[derive(Clone, Copy)]
struct KernelHandles {
    scheduler: NonNull<Scheduler>,
    tasks: NonNull<task::TaskTable>,
    router: NonNull<ipc::Router>,
    spaces: NonNull<AddressSpaceManager>,
}
unsafe impl Send for KernelHandles {}
unsafe impl Sync for KernelHandles {}

#[derive(Clone, Copy)]
struct TrapDomain {
    syscalls: NonNull<SyscallTable>,
}
unsafe impl Send for TrapDomain {}
unsafe impl Sync for TrapDomain {}

struct TrapRuntime {
    kernel: KernelHandles,
    domains: Vec<TrapDomain>,
    default_domain: TrapDomainId,
}
unsafe impl Send for TrapRuntime {}
unsafe impl Sync for TrapRuntime {}

static TRAP_RUNTIME: Mutex<Option<TrapRuntime>> = Mutex::new(None);

impl TrapRuntime {
    fn new(
        scheduler: &mut Scheduler,
        tasks: &mut task::TaskTable,
        router: &mut ipc::Router,
        spaces: &mut AddressSpaceManager,
    ) -> Self {
        Self {
            kernel: KernelHandles {
                scheduler: NonNull::from(scheduler),
                tasks: NonNull::from(tasks),
                router: NonNull::from(router),
                spaces: NonNull::from(spaces),
            },
            domains: Vec::new(),
            default_domain: TrapDomainId::default(),
        }
    }

    fn push_domain(&mut self, table: &SyscallTable) -> TrapDomainId {
        let id = TrapDomainId(self.domains.len());
        let ptr = (table as *const SyscallTable) as *mut SyscallTable;
        self.domains.push(TrapDomain {
            syscalls: NonNull::new(ptr).expect("syscall table ptr"),
        });
        id
    }

    fn domain(&self, id: TrapDomainId) -> Option<&TrapDomain> {
        self.domains.get(id.0)
    }
}

/// Installs the runtime trap context using kernel subsystems and default syscall table.
pub fn install_runtime(
    scheduler: &mut Scheduler,
    tasks: &mut task::TaskTable,
    router: &mut ipc::Router,
    spaces: &mut AddressSpaceManager,
    syscalls: &SyscallTable,
) -> TrapDomainId {
    let mut runtime = TrapRuntime::new(scheduler, tasks, router, spaces);
    let default = runtime.push_domain(syscalls);
    runtime.default_domain = default;
    *TRAP_RUNTIME.lock() = Some(runtime);
    default
}

/// Registers an additional trap domain (e.g. alternative syscall table).
#[allow(dead_code)]
pub fn register_trap_domain(syscalls: &SyscallTable) -> TrapDomainId {
    let mut guard = TRAP_RUNTIME.lock();
    let runtime = guard.as_mut().expect("trap runtime not installed");
    runtime.push_domain(syscalls)
}

fn runtime_kernel_handles() -> Option<KernelHandles> {
    let guard = TRAP_RUNTIME.lock();
    guard.as_ref().map(|runtime| runtime.kernel)
}

fn runtime_domain(id: TrapDomainId) -> Option<NonNull<SyscallTable>> {
    let guard = TRAP_RUNTIME.lock();
    guard
        .as_ref()
        .and_then(|runtime| {
            runtime
                .domain(id)
                .or_else(|| runtime.domain(runtime.default_domain))
        })
        .map(|domain| domain.syscalls)
}

fn runtime_default_domain() -> TrapDomainId {
    let guard = TRAP_RUNTIME.lock();
    guard
        .as_ref()
        .map(|runtime| runtime.default_domain)
        .unwrap_or_default()
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
    if (inst & 0x7f) != 0b111_0011 {
        return false;
    }
    let funct3 = (inst >> 12) & 0x7;
    if funct3 != 0b010 {
        return false;
    }
    let csr = ((inst >> 20) & 0x0fff) as u16;
    csr == 0xC00 /*cycle*/ || csr == 0xC01 /*time*/
}
#[inline]
fn is_rdinstret(inst: u32) -> bool {
    if (inst & 0x7f) != 0b111_0011 {
        return false;
    }
    let funct3 = (inst >> 12) & 0x7;
    if funct3 != 0b010 {
        return false;
    }
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

const _: [(); core::mem::size_of::<usize>() * 32] = [(); core::mem::offset_of!(TrapFrame, sepc)];
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
    uart_dbg_block!({
        let mut u = crate::uart::raw_writer();
        let _ = u.write_str("HECALL start old=0x");
        uart_write_hex(&mut u, old_pid as usize);
        let _ = u.write_str("\n");
    });
    if let Some(task) = ctx.tasks.task_mut(old_pid) {
        uart_dbg_block!({
            let mut u = crate::uart::raw_writer();
            let _ = u.write_str("HECALL save frame pid=0x");
            uart_write_hex(&mut u, old_pid as usize);
            let _ = u.write_str("\n");
        });
        *task.frame_mut() = *frame;
    } else {
        uart_dbg_block!({
            let mut u = crate::uart::raw_writer();
            let _ = u.write_str("HECALL missing task pid=0x");
            uart_write_hex(&mut u, old_pid as usize);
            let _ = u.write_str("\n");
        });
    }
    record(frame);
    // a7 = syscall number; a0..a5 = args
    let number = frame.x[17]; // a7
    let args = Args::new([
        frame.x[10],
        frame.x[11],
        frame.x[12],
        frame.x[13],
        frame.x[14],
        frame.x[15],
    ]);
    #[cfg(feature = "debug_uart")]
    if number != SYSCALL_DEBUG_PUTC {
        uart_dbg_block!({
            let mut u = crate::uart::raw_writer();
            let _ = u.write_str("SYSCALL a7=0x");
            uart_write_hex(&mut u, number);
            let _ = u.write_str(" a0=0x");
            uart_write_hex(&mut u, frame.x[10]);
            let _ = u.write_str("\n");
        });
    }
    #[cfg(feature = "debug_uart")]
    if number == SYSCALL_AS_MAP {
        uart_dbg_block!({
            let mut u = crate::uart::raw_writer();
            let _ = u.write_str("SYSCALL as_map handle=0x");
            uart_write_hex(&mut u, frame.x[10]);
            let _ = u.write_str(" vmo=0x");
            uart_write_hex(&mut u, frame.x[11]);
            let _ = u.write_str(" va=0x");
            uart_write_hex(&mut u, frame.x[12]);
            let _ = u.write_str(" len=0x");
            uart_write_hex(&mut u, frame.x[13]);
            let _ = u.write_str(" prot=0x");
            uart_write_hex(&mut u, frame.x[14]);
            let _ = u.write_str(" flags=0x");
            uart_write_hex(&mut u, frame.x[15]);
            let _ = u.write_str("\n");
        });
    }
    uart_dbg_block!({
        let mut u = crate::uart::raw_writer();
        let _ = u.write_str("HECALL dispatch num=0x");
        uart_write_hex(&mut u, number);
        let _ = u.write_str("\n");
    });
    uart_dbg_block!({
        let mut u = crate::uart::raw_writer();
        let _ = u.write_str("HECALL table ptr=0x");
        uart_write_hex(&mut u, table as *const SyscallTable as usize);
        let _ = u.write_str(" handler=0x");
        if let Some(addr) = table.debug_handler_addr(number) {
            uart_write_hex(&mut u, addr);
        } else {
            let _ = u.write_str("none");
        }
        let _ = u.write_str("\n");
    });
    let mut maybe_ret = None;
    match table.dispatch(number, ctx, &args) {
        Ok(ret) => maybe_ret = Some(ret),
        Err(SysError::TaskExit) => {}
        Err(err) => {
            let _errno_val = encode_error(err);
            uart_dbg_block!({
                let mut u = crate::uart::raw_writer();
                let _ = u.write_str("HECALL dispatch err num=0x");
                uart_write_hex(&mut u, number);
                let _ = u.write_str(" sepc=0x");
                uart_write_hex(&mut u, frame.sepc);
                let _ = u.write_str(" err=");
                uart_write_hex(&mut u, _errno_val);
                let _ = u.write_str("\n");
            });
            // Fail-fast: terminate the offending task to avoid ECALL storms.
            ctx.tasks.exit_current(-22);
            return;
        }
    }
    uart_dbg_block!({
        let mut u = crate::uart::raw_writer();
        let _ = u.write_str("HECALL dispatch done maybe=");
        match maybe_ret {
            Some(ret) => uart_write_hex(&mut u, ret),
            None => {
                let _ = u.write_str("none");
            }
        }
        let _ = u.write_str("\n");
    });
    uart_dbg_block!({
        let mut u = crate::uart::raw_writer();
        let _ = u.write_str("handle_ecall before advance sepc=0x");
        uart_write_hex(&mut u, frame.sepc);
        let _ = u.write_str("\n");
    });
    // Advance caller PC and store return in its saved frame (a0).
    if let Some(ret) = maybe_ret {
        if let Some(task) = ctx.tasks.task_mut(old_pid) {
            let f = task.frame_mut();
            uart_dbg_block!({
                ecall_log(|u| {
                    use core::fmt::Write as _;
                    let _ = write!(
                        u,
                        "ECALL pre-advance pid=0x{:x} sepc=0x{:x}\n",
                        old_pid as usize, f.sepc
                    );
                });
            });
            f.sepc = f.sepc.wrapping_add(4);
            f.x[10] = ret;
            uart_dbg_block!({
                let mut u = crate::uart::raw_writer();
                let _ = u.write_str("HECALL ret store pid=0x");
                uart_write_hex(&mut u, old_pid as usize);
                let _ = u.write_str(" sepc=0x");
                uart_write_hex(&mut u, f.sepc);
                let _ = u.write_str(" a0=0x");
                uart_write_hex(&mut u, ret);
                let _ = u.write_str("\n");
            });
            uart_dbg_block!({
                let mut u = crate::uart::raw_writer();
                let _ = u.write_str("task.frame_mut after sepc=0x");
                uart_write_hex(&mut u, f.sepc);
                let _ = u.write_str("\n");
            });
            uart_dbg_block!({
                ecall_log(|u| {
                    use core::fmt::Write as _;
                    let _ = write!(
                        u,
                        "ECALL post-advance pid=0x{:x} sepc=0x{:x}\n",
                        old_pid as usize, f.sepc
                    );
                });
            });
        }
    }
    // Load the next task's frame into the live trap frame.
    let new_pid = ctx.tasks.current_pid();
    uart_dbg_block!({
        let mut u = crate::uart::raw_writer();
        let _ = u.write_str("HECALL load next pid=0x");
        uart_write_hex(&mut u, new_pid as usize);
        let _ = u.write_str("\n");
    });
    if let Some(task) = ctx.tasks.task_mut(new_pid) {
        *frame = *task.frame();
        // If the syscall path switched `current_pid` (e.g. SYSCALL_YIELD), ensure we
        // also switch SATP + SSCRATCH before returning to U-mode. Do NOT do this for
        // non-switching syscalls (debug_putc etc) or we will spam the UART and slow
        // boot to a crawl.
        if new_pid != old_pid {
            #[cfg(not(feature = "selftest_no_satp"))]
            if let Some(handle) = task.address_space() {
                if ctx.address_spaces.activate(handle).is_err() {
                    // Fail-fast: returning with a mismatched SATP is unsafe.
                    ctx.tasks.exit_current(-22);
                    return;
                }
            }
            #[cfg(all(target_arch = "riscv64", target_os = "none"))]
            riscv::register::sscratch::write(frame.x[2]);
        }
        uart_dbg_block!({
            let mut u = crate::uart::raw_writer();
            let _ = u.write_str("HECALL frame updated sepc=0x");
            uart_write_hex(&mut u, frame.sepc);
            let _ = u.write_str("\n");
        });
        uart_dbg_block!({
            ecall_log(|u| {
                use core::fmt::Write as _;
                let _ = write!(
                    u,
                    "ECALL load pid=0x{:x} sepc=0x{:x}\n",
                    new_pid as usize, frame.sepc
                );
            });
        });
    }
}

const EPERM: usize = 1;
const ENOMEM: usize = 12;
const EINVAL: usize = 22;
const ENOSPC: usize = 28;
const ENOSYS: usize = 38;
const ESRCH: usize = 3;
const ECHILD: usize = 10;

#[allow(dead_code)]
fn encode_error(err: SysError) -> usize {
    match err {
        SysError::InvalidSyscall => errno(ENOSYS),
        SysError::Capability(_) => errno(EPERM),
        SysError::Ipc(_) => errno(EINVAL),
        SysError::Spawn(spawn) => spawn_errno(&spawn),
        SysError::Transfer(_) => errno(EPERM),
        SysError::AddressSpace(as_err) => address_space_errno(&as_err),
        SysError::Wait(wait) => wait_errno(&wait),
        SysError::TaskExit => errno(EINVAL),
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

#[allow(dead_code)]
fn wait_errno(err: &task::WaitError) -> usize {
    use task::WaitError::*;
    match err {
        NoChildren => errno(ECHILD),
        NoSuchPid => errno(ESRCH),
        InvalidTarget => errno(EINVAL),
        WouldBlock => errno(EINVAL),
    }
}

const fn errno(code: usize) -> usize {
    (-(code as isize)) as usize
}

#[cfg(any(debug_assertions, feature = "trap_ring"))]
pub fn record(frame: &TrapFrame) {
    *LAST_TRAP.lock() = Some(*frame);
    let mut idx = TRAP_RING_IDX.lock();
    let mut ring = TRAP_RING.lock();
    ring[*idx % TRAP_RING_LEN] = Some(*frame);
    *idx = (*idx + 1) % TRAP_RING_LEN;
}

#[cfg(not(any(debug_assertions, feature = "trap_ring")))]
pub fn record(_frame: &TrapFrame) {}

#[cfg(any(debug_assertions, feature = "trap_ring"))]
pub fn last_trap() -> Option<TrapFrame> {
    *LAST_TRAP.lock()
}

#[cfg(not(any(debug_assertions, feature = "trap_ring")))]
pub fn last_trap() -> Option<TrapFrame> {
    None
}

#[cfg(any(debug_assertions, feature = "trap_ring"))]
pub fn visit_trap_ring(mut f: impl FnMut(usize, &TrapFrame)) {
    let len = TRAP_RING_LEN;
    let start = *TRAP_RING_IDX.lock();
    let ring = TRAP_RING.lock();
    for offset in 0..len {
        let slot = (start + offset) % len;
        if let Some(frame) = &ring[slot] {
            f(slot, frame);
        }
    }
}
#[inline]
pub fn is_interrupt(scause: usize) -> bool {
    scause & INTERRUPT_FLAG != 0
}

#[cfg_attr(
    not(all(target_arch = "riscv64", target_os = "none")),
    allow(dead_code)
)]
#[allow(dead_code)]
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

#[cfg_attr(
    not(all(target_arch = "riscv64", target_os = "none")),
    allow(dead_code)
)]
#[allow(dead_code)]
pub fn fmt_trap<W: Write>(frame: &TrapFrame, f: &mut W) -> fmt::Result {
    writeln!(f, " sepc=0x{:016x}", frame.sepc)?;
    writeln!(
        f,
        " scause=0x{:016x} ({})",
        frame.scause,
        describe_cause(frame.scause)
    )?;
    writeln!(f, " stval=0x{:016x}", frame.stval)?;
    writeln!(f, " a0..a7 = {:016x?}", &frame.x[10..=17])
}

// ——— SBI timer utilities ———

/// Default tick in cycles (10 ms for 10 MHz mtimer on QEMU virt).
#[cfg_attr(
    not(all(target_arch = "riscv64", target_os = "none")),
    allow(dead_code)
)]
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
    // CRITICAL: Trap entry marker with cause (safe UART, no heap)
    uart_dbg_block!({
        let mut u = crate::uart::raw_writer();
        let _ = u.write_str("TRAP[");
        uart_write_hex(&mut u, frame.scause);
        let _ = u.write_str("] sepc=0x");
        uart_write_hex(&mut u, frame.sepc);
        let _ = u.write_str("\n");
        core::mem::drop(u);
    });

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
    const ECALL_UMODE: usize = 8;
    const ECALL_SMODE: usize = 9;
    const LOAD_PAGE_FAULT: usize = 13;
    const STORE_PAGE_FAULT: usize = 15;
    const INST_PAGE_FAULT: usize = 12;
    let exc = frame.scause & (usize::MAX >> 1);
    // Quiet exception banner during bring-up to avoid fmt/alloc paths
    if exc == ECALL_UMODE || exc == ECALL_SMODE {
        // Debug: Log FIRST ecall only using safe UART (no heap allocation)
        static ECALL_COUNT: core::sync::atomic::AtomicUsize =
            core::sync::atomic::AtomicUsize::new(0);
        // Guard against true ECALL storms (same sepc repeating), but do not penalize
        // normal syscall-heavy workloads (e.g. init printing boot markers).
        static LAST_ECALL_SEPC: core::sync::atomic::AtomicUsize =
            core::sync::atomic::AtomicUsize::new(0);
        static SAME_ECALL_SEPC_COUNT: core::sync::atomic::AtomicUsize =
            core::sync::atomic::AtomicUsize::new(0);
        let count = ECALL_COUNT.fetch_add(1, core::sync::atomic::Ordering::SeqCst);
        if count == 0 {
            // CRITICAL: Minimal logging in separate scope, explicit drop before proceeding
            uart_dbg_block!({
                let mut u = crate::uart::raw_writer();
                let _ = u.write_str("ECALL #0 sepc=0x");
                uart_write_hex(&mut u, frame.sepc);
                let _ = u.write_str("\n");
                core::mem::drop(u);
            });
        }

        // Prevent endless ECALL storms: abort only if we observe a large number of
        // ECALLs from the exact same sepc (no forward progress).
        let last = LAST_ECALL_SEPC.load(core::sync::atomic::Ordering::Relaxed);
        let same = if last == frame.sepc {
            SAME_ECALL_SEPC_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed) + 1
        } else {
            LAST_ECALL_SEPC.store(frame.sepc, core::sync::atomic::Ordering::Relaxed);
            SAME_ECALL_SEPC_COUNT.store(0, core::sync::atomic::Ordering::Relaxed);
            0
        };
        if same > 10_000 {
            uart_dbg_block!({
                let mut u = crate::uart::raw_writer();
                let _ = u.write_str("ECALL-STORM abort sepc=0x");
                uart_write_hex(&mut u, frame.sepc);
                let _ = u.write_str(" ra=0x");
                uart_write_hex(&mut u, frame.x[1]);
                let _ = u.write_str("\n");
            });
            frame.x[10] = errno(EINVAL);
            if let Some(mut handles) = runtime_kernel_handles() {
                unsafe {
                    let tasks = handles.tasks.as_mut();
                    tasks.exit_current(-22);
                }
            }
            return;
        }

        // Debug: Log syscall number with safe UART
        let kernel_handles = match runtime_kernel_handles() {
            Some(handles) => handles,
            None => {
                let mut u = crate::uart::raw_writer();
                let _ = u.write_str("WARN: trap runtime not installed\n");
                frame.x[10] = errno(ENOSYS);
                frame.sepc = frame.sepc.wrapping_add(4);
                return;
            }
        };

        // User-mode sanity: verify sepc is mapped in current AS and looks executable.
        // This is diagnostic-only and protects against jumping into rodata.
        const SSTATUS_SPP: usize = 1 << 8;
        let from_user = (frame.sstatus & SSTATUS_SPP) == 0;
        if from_user {
            let tasks = unsafe { kernel_handles.tasks.as_ref() };
            let spaces = unsafe { kernel_handles.spaces.as_ref() };
            let pid = tasks.current_pid();
            if let Some(task) = tasks.task(pid) {
                if let Some(as_handle) = task.address_space() {
                    if let Ok(space) = spaces.get(as_handle) {
                        let pt = space.page_table();
                        let maybe_sepc = pt.translate(frame.sepc);
                        if let Some(_pa) = maybe_sepc {
                            uart_dbg_block!({
                                let mut u = crate::uart::raw_writer();
                                let _ = u.write_str("ECALL-BOUNDS sepc ok pa=0x");
                                uart_write_hex(&mut u, _pa);
                                let _ = u.write_str("\n");
                            });
                        } else {
                            uart_dbg_block!({
                                let mut u = crate::uart::raw_writer();
                                let _ = u.write_str("ECALL-BOUNDS unmapped sepc=0x");
                                uart_write_hex(&mut u, frame.sepc);
                                let _ = u.write_str(" ra=0x");
                                uart_write_hex(&mut u, frame.x[1]);
                                let _ = u.write_str(" sp=0x");
                                uart_write_hex(&mut u, frame.x[2]);
                                let _ = u.write_str("\n");
                            });
                            frame.x[10] = errno(EINVAL);
                            if let Some(mut handles) = runtime_kernel_handles() {
                                unsafe {
                                    let tasks = handles.tasks.as_mut();
                                    tasks.exit_current(-22);
                                }
                            }
                            return;
                        }
                    }
                }
            }
        }

        static LOGGED_ENV_PTRS: core::sync::atomic::AtomicBool =
            core::sync::atomic::AtomicBool::new(false);
        if !LOGGED_ENV_PTRS.swap(true, core::sync::atomic::Ordering::SeqCst) {
            uart_dbg_block!({
                let mut u = crate::uart::raw_writer();
                let _ = u.write_str("TRAPENV sched=0x");
                uart_write_hex(&mut u, kernel_handles.scheduler.as_ptr() as usize);
                let _ = u.write_str(" tasks=0x");
                uart_write_hex(&mut u, kernel_handles.tasks.as_ptr() as usize);
                let _ = u.write_str(" router=0x");
                uart_write_hex(&mut u, kernel_handles.router.as_ptr() as usize);
                let _ = u.write_str(" spaces=0x");
                uart_write_hex(&mut u, kernel_handles.spaces.as_ptr() as usize);
                let _ = u.write_str("\n");
            });
        }

        let mut sched_ptr = kernel_handles.scheduler;
        let scheduler = unsafe { sched_ptr.as_mut() };
        let mut tasks_ptr = kernel_handles.tasks;
        let tasks = unsafe { tasks_ptr.as_mut() };
        let mut router_ptr = kernel_handles.router;
        let router = unsafe { router_ptr.as_mut() };
        let mut spaces_ptr = kernel_handles.spaces;
        let spaces = unsafe { spaces_ptr.as_mut() };

        let current_pid = tasks.current_pid();
        let domain_id = tasks
            .task(current_pid)
            .map(|task| task.trap_domain())
            .unwrap_or_else(|| runtime_default_domain());
        let syscalls_ptr = runtime_domain(domain_id)
            .or_else(|| runtime_domain(runtime_default_domain()))
            .expect("trap domain not available");
        let table = unsafe { syscalls_ptr.as_ref() };

        struct NullTimer;
        impl crate::hal::Timer for NullTimer {
            fn now(&self) -> u64 {
                0
            }
            fn set_wakeup(&self, _deadline: u64) {}
        }
        let timer = NullTimer;
        #[allow(unused_variables)]
        let old_pid = tasks.current_pid();
        let mut ctx = api::Context::new(scheduler, tasks, router, spaces, &timer);
        handle_ecall(frame, table, &mut ctx);

        let current_pid = ctx.tasks.current_pid();
        uart_dbg_block!({
            let mut u = crate::uart::raw_writer();
            let _ = u.write_str("CTX PID old=0x");
            uart_write_hex(&mut u, old_pid as usize);
            let _ = u.write_str(" new=0x");
            uart_write_hex(&mut u, current_pid as usize);
            let _ = u.write_str("\n");
        });
        if let Some(task) = ctx.tasks.task(current_pid) {
            let tf = task.frame();
            uart_dbg_block!({
                let mut u = crate::uart::raw_writer();
                let _ = u.write_str("TASK FRAME sepc=0x");
                uart_write_hex(&mut u, tf.sepc);
                let _ = u.write_str("\n");
            });
            frame.x.copy_from_slice(&tf.x);
            frame.sepc = tf.sepc;
            frame.sstatus = tf.sstatus;
            frame.scause = tf.scause;
            frame.stval = tf.stval;
        } else {
            uart_dbg_block!({
                let mut u = crate::uart::raw_writer();
                let _ = u.write_str("TASK FRAME missing for pid=0x");
                uart_write_hex(&mut u, current_pid as usize);
                let _ = u.write_str("\n");
            });
        }
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
            if rd != 0 {
                frame.set_x(rd, 0);
            }
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
            let _ = write!(
                u,
                "ILLEGAL-D: sepc=0x{:x} ra=0x{:x} stval=0x{:x}\n",
                frame.sepc, frame.x[1], stval_now
            );
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
                    let _ = write!(
                        u,
                        "ILLEGAL-D: satp=0x{:x} page=0x{:x} (ppn=0)\n",
                        satp_now, page_va
                    );
                } else {
                    let mut table = (ppn << 12) as *const usize;
                    let indices = vpn_indices_sv39(frame.sepc);
                    let mut pte: usize = 0;
                    let mut found = true;
                    for (level, idx) in indices.iter().enumerate() {
                        let entry_ptr = unsafe { table.add(*idx) };
                        let entry = unsafe { core::ptr::read_volatile(entry_ptr) };
                        if entry & 1 == 0 {
                            found = false;
                            break;
                        }
                        let is_leaf = (entry & ((1 << 1) | (1 << 2) | (1 << 3))) != 0; // any of R/W/X
                        if level == 2 {
                            if !is_leaf {
                                found = false;
                                break;
                            }
                            pte = entry;
                            break;
                        }
                        if is_leaf {
                            found = false;
                            break;
                        }
                        let next_ppn = (entry >> 10) & ((1 << 44) - 1);
                        table = (next_ppn << 12) as *const usize;
                    }
                    if found {
                        let flags = pte & 0x3ff;
                        let _ = write!(
                            u,
                            "ILLEGAL-D: satp=0x{:x} pte=0x{:x} flags=0x{:x}\n",
                            satp_now, pte, flags
                        );
                    } else {
                        let page_va = frame.sepc & !(crate::mm::PAGE_SIZE - 1);
                        let _ = write!(
                            u,
                            "ILLEGAL-D: satp=0x{:x} page=0x{:x} (unmapped or non-leaf)\n",
                            satp_now, page_va
                        );
                    }
                }
            }
        }
        record(frame);
        // Avoid formatted panic to prevent allocator/formatting faults during bring-up
        panic!("ILLEGAL");
    }

    // Handle page faults (common for user processes)
    if exc == LOAD_PAGE_FAULT || exc == STORE_PAGE_FAULT || exc == INST_PAGE_FAULT {
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        {
            let stval_now = riscv::register::stval::read();
            const SSTATUS_SPP: usize = 1 << 8;
            let from_user = (frame.sstatus & SSTATUS_SPP) == 0;

            if from_user {
                // User page fault - ROBUST logging via direct MMIO (no heap, no fmt)
                // This cannot crash because it uses no dynamic allocation
                const UART_BASE: usize = 0x10000000;
                const UART_TX: usize = 0x0;
                const UART_LSR: usize = 0x5;
                const LSR_TX_IDLE: u8 = 1 << 5;

                unsafe {
                    // Helper to write one byte
                    let write_byte = |b: u8| {
                        while core::ptr::read_volatile((UART_BASE + UART_LSR) as *const u8)
                            & LSR_TX_IDLE
                            == 0
                        {}
                        core::ptr::write_volatile((UART_BASE + UART_TX) as *mut u8, b);
                    };

                    // Write "[USER-PF] type @ sepc=0x... stval=0x...\n"
                    for &b in b"[USER-PF] " {
                        write_byte(b);
                    }

                    // Fault type
                    let fault_name: &[u8] = match exc {
                        LOAD_PAGE_FAULT => b"LOAD",
                        STORE_PAGE_FAULT => b"STORE",
                        INST_PAGE_FAULT => b"INST",
                        _ => b"???",
                    };
                    for &b in fault_name {
                        write_byte(b);
                    }

                    for &b in b" @ sepc=0x" {
                        write_byte(b);
                    }
                    for shift in (0..16).rev() {
                        let nibble = ((frame.sepc >> (shift * 4)) & 0xf) as u8;
                        let ch = if nibble < 10 {
                            b'0' + nibble
                        } else {
                            b'a' + (nibble - 10)
                        };
                        write_byte(ch);
                    }

                    for &b in b" stval=0x" {
                        write_byte(b);
                    }
                    for shift in (0..16).rev() {
                        let nibble = ((stval_now >> (shift * 4)) & 0xf) as u8;
                        let ch = if nibble < 10 {
                            b'0' + nibble
                        } else {
                            b'a' + (nibble - 10)
                        };
                        write_byte(ch);
                    }

                    write_byte(b'\n');

                    for &b in b"[USER-PF] regs ra=0x" {
                        write_byte(b);
                    }
                    for shift in (0..16).rev() {
                        let nibble = ((frame.x[1] >> (shift * 4)) & 0xf) as u8;
                        let ch = if nibble < 10 {
                            b'0' + nibble
                        } else {
                            b'a' + (nibble - 10)
                        };
                        write_byte(ch);
                    }
                    for &b in b" sp=0x" {
                        write_byte(b);
                    }
                    for shift in (0..16).rev() {
                        let nibble = ((frame.x[2] >> (shift * 4)) & 0xf) as u8;
                        let ch = if nibble < 10 {
                            b'0' + nibble
                        } else {
                            b'a' + (nibble - 10)
                        };
                        write_byte(ch);
                    }
                    write_byte(b'\n');

                    for &b in b"[USER-PF] regs gp=0x" {
                        write_byte(b);
                    }
                    for shift in (0..16).rev() {
                        let nibble = ((frame.x[3] >> (shift * 4)) & 0xf) as u8;
                        let ch = if nibble < 10 {
                            b'0' + nibble
                        } else {
                            b'a' + (nibble - 10)
                        };
                        write_byte(ch);
                    }
                    write_byte(b'\n');

                    for &b in b"[USER-PF] regs a0=0x" {
                        write_byte(b);
                    }
                    for shift in (0..16).rev() {
                        let nibble = ((frame.x[10] >> (shift * 4)) & 0xf) as u8;
                        let ch = if nibble < 10 {
                            b'0' + nibble
                        } else {
                            b'a' + (nibble - 10)
                        };
                        write_byte(ch);
                    }
                    write_byte(b'\n');

                    for &b in b"[USER-PF] regs a1=0x" {
                        write_byte(b);
                    }
                    for shift in (0..16).rev() {
                        let nibble = ((frame.x[11] >> (shift * 4)) & 0xf) as u8;
                        let ch = if nibble < 10 {
                            b'0' + nibble
                        } else {
                            b'a' + (nibble - 10)
                        };
                        write_byte(ch);
                    }
                    write_byte(b'\n');

                    for &b in b"[USER-PF] regs a2=0x" {
                        write_byte(b);
                    }
                    for shift in (0..16).rev() {
                        let nibble = ((frame.x[12] >> (shift * 4)) & 0xf) as u8;
                        let ch = if nibble < 10 {
                            b'0' + nibble
                        } else {
                            b'a' + (nibble - 10)
                        };
                        write_byte(ch);
                    }
                    write_byte(b'\n');

                    for &b in b"[USER-PF] regs a3=0x" {
                        write_byte(b);
                    }
                    for shift in (0..16).rev() {
                        let nibble = ((frame.x[13] >> (shift * 4)) & 0xf) as u8;
                        let ch = if nibble < 10 {
                            b'0' + nibble
                        } else {
                            b'a' + (nibble - 10)
                        };
                        write_byte(ch);
                    }
                    write_byte(b'\n');

                    // Additional diagnostics to catch stray branch targets.
                    let regs_to_dump = [
                        (&b"t0"[..], 5usize),
                        (&b"t1"[..], 6usize),
                        (&b"t2"[..], 7usize),
                        (&b"s0"[..], 8usize),
                        (&b"s1"[..], 9usize),
                        (&b"s2"[..], 18usize),
                        (&b"s3"[..], 19usize),
                        (&b"s4"[..], 20usize),
                        (&b"s5"[..], 21usize),
                        (&b"s6"[..], 22usize),
                        (&b"s7"[..], 23usize),
                        (&b"s8"[..], 24usize),
                        (&b"s9"[..], 25usize),
                        (&b"s10"[..], 26usize),
                        (&b"s11"[..], 27usize),
                        (&b"t3"[..], 28usize),
                        (&b"t4"[..], 29usize),
                        (&b"t5"[..], 30usize),
                        (&b"t6"[..], 31usize),
                    ];
                    for &(label, reg_idx) in regs_to_dump.iter() {
                        for &b in b"[USER-PF] regs " {
                            write_byte(b);
                        }
                        for &b in label.iter() {
                            write_byte(b);
                        }
                        for &b in b"=0x" {
                            write_byte(b);
                        }
                        let value = frame.x[reg_idx];
                        for shift in (0..16).rev() {
                            let nibble = ((value >> (shift * 4)) & 0xf) as u8;
                            let ch = if nibble < 10 {
                                b'0' + nibble
                            } else {
                                b'a' + (nibble - 10)
                            };
                            write_byte(ch);
                        }
                        write_byte(b'\n');
                    }
                }

                // Snapshot current task's saved frame for additional diagnostics
                if let Some(handles) = runtime_kernel_handles() {
                    unsafe {
                        let tasks = handles.tasks.as_ref();
                        let spaces = handles.spaces.as_ref();
                        let current_pid = tasks.current_pid();
                        if let Some(task) = tasks.task(current_pid) {
                            dump_user_stack_for_task(task, spaces, frame.x[2]);
                            let tf = task.frame();
                            let write_field = |label: &[u8], value: usize| {
                                let write_byte = |b: u8| {
                                    while core::ptr::read_volatile(
                                        (UART_BASE + UART_LSR) as *const u8,
                                    ) & LSR_TX_IDLE
                                        == 0
                                    {}
                                    core::ptr::write_volatile((UART_BASE + UART_TX) as *mut u8, b);
                                };
                                for &b in b"[USER-PF] task " {
                                    write_byte(b);
                                }
                                for &b in label {
                                    write_byte(b);
                                }
                                for &b in b"=0x" {
                                    write_byte(b);
                                }
                                for shift in (0..16).rev() {
                                    let nibble = ((value >> (shift * 4)) & 0xf) as u8;
                                    let ch = if nibble < 10 {
                                        b'0' + nibble
                                    } else {
                                        b'a' + (nibble - 10)
                                    };
                                    write_byte(ch);
                                }
                                write_byte(b'\n');
                            };
                            write_field(b"sepc", tf.sepc);
                            write_field(b"sp", tf.x[2]);
                        }
                    }
                }

                // TODO: Properly kill task and return to scheduler
                // For now, just hang this task by looping on same instruction
                return;
            }

            // Kernel page fault - emit minimal diagnostics via raw MMIO then panic
            {
                const UART_BASE: usize = 0x10000000;
                const UART_TX: usize = 0x0;
                const UART_LSR: usize = 0x5;
                const LSR_TX_IDLE: u8 = 1 << 5;
                unsafe {
                    let write_byte = |b: u8| {
                        while core::ptr::read_volatile((UART_BASE + UART_LSR) as *const u8)
                            & LSR_TX_IDLE
                            == 0
                        {}
                        core::ptr::write_volatile((UART_BASE + UART_TX) as *mut u8, b);
                    };
                    let write_hex = |val: usize, digits: usize| {
                        for shift in (0..digits).rev() {
                            let nibble = ((val >> (shift * 4)) & 0xf) as u8;
                            let ch = if nibble < 10 {
                                b'0' + nibble
                            } else {
                                b'a' + (nibble - 10)
                            };
                            write_byte(ch);
                        }
                    };
                    for &b in b"KPGF sepc=0x" {
                        write_byte(b);
                    }
                    write_hex(frame.sepc, 16);
                    for &b in b" stval=0x" {
                        write_byte(b);
                    }
                    write_hex(stval_now, 16);
                    for &b in b" scause=0x" {
                        write_byte(b);
                    }
                    write_hex(frame.scause, 16);
                    for &b in b" ra=0x" {
                        write_byte(b);
                    }
                    write_hex(frame.x[1], 16);
                    for &b in b" sp=0x" {
                        write_byte(b);
                    }
                    write_hex(frame.x[2], 16);
                    for &b in b" a7=0x" {
                        write_byte(b);
                    }
                    write_hex(frame.x[17], 16);
                    for &b in b" a0=0x" {
                        write_byte(b);
                    }
                    write_hex(frame.x[10], 16);
                    for &b in b" a1=0x" {
                        write_byte(b);
                    }
                    write_hex(frame.x[11], 16);
                    for &b in b" a2=0x" {
                        write_byte(b);
                    }
                    write_hex(frame.x[12], 16);
                    for &b in b" sstatus=0x" {
                        write_byte(b);
                    }
                    write_hex(frame.sstatus, 16);
                    for &b in b" satp=0x" {
                        write_byte(b);
                    }
                    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
                    write_hex(riscv::register::satp::read().bits(), 16);
                    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
                    write_hex(0, 16);
                    write_byte(b'\n');
                }
            }
            record(frame);
            panic!("KPGF");
        }
    }

    // Other exceptions
    {
        // Non-Illegal exceptions: emit minimal diagnostics
        #[cfg(all(target_arch = "riscv64", target_os = "none"))]
        {
            let stval_now = riscv::register::stval::read();
            uart_print_exc(frame.scause, frame.sepc, stval_now);

            // Check if fault is from user mode (SPP=0) or kernel mode (SPP=1)
            const SSTATUS_SPP: usize = 1 << 8;
            let from_user = (frame.sstatus & SSTATUS_SPP) == 0;

            if from_user {
                // User task fault - log and halt
                // CRITICAL: Use safe UART (no allocation/formatting)
                use core::fmt::Write as _;
                let mut u = crate::uart::raw_writer();
                let _ = u.write_str("[USER-FAULT] scause=0x");
                uart_write_hex(&mut u, frame.scause);
                let _ = u.write_str(" sepc=0x");
                uart_write_hex(&mut u, frame.sepc);
                let _ = u.write_str(" stval=0x");
                uart_write_hex(&mut u, stval_now);
                let _ = u.write_str("\n");
                // Hang user task
                frame.sepc = frame.sepc;
                return;
            }

            // Kernel fault - this is a bug
            if stval_now < 0x1000 {
                panic!("KNULL");
            }
        }
        record(frame);
        panic!("KEXC");
    }
}

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
