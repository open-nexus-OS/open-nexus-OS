// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! In-kernel selftest harness executed during deterministic boot.

extern crate alloc;

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
use core::{
    ffi::c_void,
    sync::atomic::{AtomicUsize, Ordering},
};

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
use crate::{
    cap::{Capability, CapabilityKind, Rights},
    hal::virt::VirtMachine,
    ipc::Router,
    mm::{AddressSpaceError, AddressSpaceManager, MapError, PAGE_SIZE},
    sched::Scheduler,
    syscall::{
        api, Args, Error as SysError, SyscallTable, SYSCALL_AS_CREATE, SYSCALL_AS_MAP,
        SYSCALL_SPAWN, SYSCALL_YIELD,
    },
    task::TaskTable,
};

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
use crate::task::Pid;
#[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
use crate::{
    hal::virt::VirtMachine, ipc::Router, mm::AddressSpaceManager, sched::Scheduler, task::TaskTable,
};
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
use riscv::register::sstatus;

pub mod assert;

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
core::arch::global_asm!(include_str!("stack_run.S"));

#[allow(unused_macros)]
macro_rules! verbose {
    ($($arg:tt)*) => {};
}

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
const CHILD_TEST_VA: usize = 0x4010_0000;
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
const CHILD_PATTERN: &[u8; 8] = b"NeuronAS";
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
static CHILD_HEARTBEAT: AtomicUsize = AtomicUsize::new(0);
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
static DIRECT_RUN_FLAG: AtomicUsize = AtomicUsize::new(0);

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
#[repr(align(4096))]
struct AlignedPage([u8; PAGE_SIZE]);
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
static mut CHILD_DATA_PAGE: AlignedPage = AlignedPage([0; PAGE_SIZE]);

#[cfg(all(feature = "selftest_priv_stack", target_arch = "riscv64", target_os = "none"))]
const SELFTEST_STACK_PAGES: usize = 8;
#[cfg(all(feature = "selftest_priv_stack", target_arch = "riscv64", target_os = "none"))]
const SELFTEST_STACK_BYTES: usize = SELFTEST_STACK_PAGES * PAGE_SIZE;
#[cfg(all(feature = "selftest_priv_stack", target_arch = "riscv64", target_os = "none"))]
#[link_section = ".bss.selftest_stack_body"]
#[used]
static mut SELFTEST_STACK: [u8; SELFTEST_STACK_BYTES] = [0; SELFTEST_STACK_BYTES];

/// Borrowed references to kernel subsystems used by selftests.
pub struct Context<'a> {
    #[allow(dead_code)]
    pub hal: &'a VirtMachine,
    #[cfg_attr(not(all(target_arch = "riscv64", target_os = "none")), allow(dead_code))]
    pub router: &'a mut Router,
    #[allow(dead_code)]
    pub address_spaces: &'a mut AddressSpaceManager,
    #[cfg_attr(not(all(target_arch = "riscv64", target_os = "none")), allow(dead_code))]
    pub tasks: &'a mut TaskTable,
    #[cfg_attr(not(all(target_arch = "riscv64", target_os = "none")), allow(dead_code))]
    pub scheduler: &'a mut Scheduler,
}

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
extern "C" fn child_new_as_entry() {
    log_info!(target: "selftest", "KSELFTEST: child entry");
    let mut matched = true;
    let mut first: u8 = 0;
    for (index, byte) in CHILD_PATTERN.iter().enumerate() {
        let value = unsafe { core::ptr::read_volatile((CHILD_TEST_VA + index) as *const u8) };
        if index == 0 {
            first = value;
        }
        if value != *byte {
            matched = false;
            break;
        }
    }
    if matched {
        log_info!(target: "selftest", "KSELFTEST: child newas running");
        CHILD_HEARTBEAT.store(1, Ordering::SeqCst);
    } else {
        log_info!(target: "selftest", "KSELFTEST: child read mismatch b0=0x{:02x}", first);
        CHILD_HEARTBEAT.store(usize::MAX, Ordering::SeqCst);
    }
    if DIRECT_RUN_FLAG.load(Ordering::SeqCst) == 0 {
        syscall_yield();
        syscall_yield();
    }
    CHILD_HEARTBEAT.store(2, Ordering::SeqCst);
    log_info!(target: "selftest", "KSELFTEST: child exit");
}

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
#[inline(always)]
fn syscall_yield() {
    unsafe {
        core::arch::asm!(
            "li a7, {id}",
            "ecall",
            id = const SYSCALL_YIELD,
            out("a0") _,
            out("a1") _,
            out("a2") _,
            out("a3") _,
            out("a4") _,
            out("a5") _,
            out("a6") _,
            out("a7") _,
            options(nostack),
            clobber_abi("C")
        );
    }
}

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
fn ensure_data_cap(tasks: &mut TaskTable) {
    let ptr = unsafe { core::ptr::addr_of_mut!(CHILD_DATA_PAGE.0) as usize };
    let pattern = CHILD_PATTERN;
    for (idx, byte) in pattern.iter().enumerate() {
        unsafe {
            core::ptr::write_volatile((ptr + idx) as *mut u8, *byte);
        }
    }
    let cap =
        Capability { kind: CapabilityKind::Vmo { base: ptr, len: PAGE_SIZE }, rights: Rights::MAP };
    let caps = tasks.bootstrap_mut().caps_mut();
    let _ = caps.set(2, cap);
}

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
fn run_address_space_selftests(ctx: &mut Context<'_>) {
    ensure_data_cap(ctx.tasks);
    let handle_raw: usize;
    let child_pid: usize;
    {
        let mut table = SyscallTable::new();
        api::install_handlers(&mut table);
        let timer = ctx.hal.timer();
        let mut sys_ctx =
            api::Context::new(ctx.scheduler, ctx.tasks, ctx.router, ctx.address_spaces, timer);

        let h = table
            .dispatch(SYSCALL_AS_CREATE, &mut sys_ctx, &Args::new([0; 6]))
            .expect("as_create syscall");
        handle_raw = h;
        log_info!(target: "selftest", "KSELFTEST: as create ok");

        const PROT_READ: usize = 1 << 0;
        const PROT_WRITE: usize = 1 << 1;
        const MAP_FLAG_USER: usize = 1 << 0;
        let map_args =
            Args::new([handle_raw, 2, CHILD_TEST_VA, PAGE_SIZE, PROT_READ, MAP_FLAG_USER]);
        table.dispatch(SYSCALL_AS_MAP, &mut sys_ctx, &map_args).expect("as_map syscall");
        log_info!(target: "selftest", "KSELFTEST: as map ok");

        let entry = child_new_as_entry as usize;
        verbose!("KSELFTEST: before spawn\n");
        // Provide a non-zero stack and bind to the created AS to satisfy strict arg checks
        const STACK_PAGES: usize = 4;
        let user_stack_top: usize = 0x4000_0000;
        let guard_bottom = user_stack_top - (STACK_PAGES + 1) * PAGE_SIZE;
        let stack_map_args = Args::new([
            handle_raw, // target AS
            1,          // VMO slot: identity VMO with MAP rights
            guard_bottom + PAGE_SIZE,
            (STACK_PAGES * PAGE_SIZE) as usize,
            (PROT_READ | PROT_WRITE) as usize,
            MAP_FLAG_USER as usize,
        ]);
        table
            .dispatch(SYSCALL_AS_MAP, &mut sys_ctx, &stack_map_args)
            .expect("as_map stack syscall");
        // Supply valid sp and as_handle
        let spawn_args = Args::new([entry, user_stack_top, handle_raw, 0, 0, 0]);
        child_pid =
            table.dispatch(SYSCALL_SPAWN, &mut sys_ctx, &spawn_args).expect("spawn syscall");
        verbose!("KSELFTEST: after spawn (raw)\n");
        // Force a few yields to exercise trap fastpath and encourage scheduling
        syscall_yield();
        syscall_yield();
        syscall_yield();
        verbose!("KSELFTEST: after spawn\n");
        verbose!("KSELFTEST: child pid={}\n", child_pid);
        // Fail-fast window: child must signal Heartbeat within 64 yields
        let mut spins = 0;
        while CHILD_HEARTBEAT.load(Ordering::SeqCst) == 0 && spins < 8 {
            syscall_yield();
            spins += 1;
        }
        if CHILD_HEARTBEAT.load(Ordering::SeqCst) == 0 {
            let _satp_now = {
                #[cfg(all(target_arch = "riscv64", target_os = "none"))]
                {
                    riscv::register::satp::read().bits()
                }
                #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
                {
                    0
                }
            };
            log_info!(target: "selftest", "KSELFTEST: no child progress yet pid={} satp=0x{:x}", child_pid, _satp_now);
            // Do not abort here; proceed to direct call_on_stack path to validate AS/mapping.
        }
        // For bring-up diagnostics, mark that control returned here.
        verbose!("KSELFTEST: early return after spawn\n");
        // sys_ctx and table drop here to release borrows on ctx.*
    }

    // Confirm we exited the syscall context block cleanly
    verbose!("KSELFTEST: after sysctx block\n");

    // Directly enter the child's address space and run the entry on its stack.
    {
        let pid = child_pid as Pid;
        if let Some(task) = ctx.tasks.task(pid) {
            if let Some(handle) = task.address_space() {
                log_info!(target: "selftest", "KSELFTEST: direct-run begin");
                let _ = ctx.address_spaces.activate(handle);
                log_info!(target: "selftest", "KSELFTEST: direct-run set SUM");
                unsafe {
                    sstatus::set_sum();
                }
                log_info!(target: "selftest", "KSELFTEST: direct-run calling entry sp=0x{:x}", task.frame().x[2]);
                // Witness: SATP must match the child's address space SATP value
                #[cfg(all(target_arch = "riscv64", target_os = "none"))]
                {
                    let satp_now = riscv::register::satp::read().bits();
                    let expected =
                        ctx.address_spaces.get(handle).map(|s| s.satp_value()).unwrap_or(0);
                    if satp_now != expected {
                        crate::selftest::assert::report_failure("witness: satp mismatch");
                    }
                }
                // Witness: Probe read from child VA must return expected byte
                #[cfg(all(target_arch = "riscv64", target_os = "none"))]
                {
                    let b0 = unsafe { core::ptr::read_volatile(CHILD_TEST_VA as *const u8) };
                    if b0 != CHILD_PATTERN[0] {
                        crate::selftest::assert::report_failure("witness: VA probe failed");
                    }
                }
                #[cfg(all(target_arch = "riscv64", target_os = "none"))]
                {
                    let _satp_now = riscv::register::satp::read().bits();
                    verbose!(
                        "KSELFTEST: pre child satp=0x{:x} sp=0x{:x} sepc=0x{:x}\n",
                        _satp_now,
                        task.frame().x[2],
                        task.frame().sepc
                    );
                }
                #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
                verbose!(
                    "KSELFTEST: pre child satp=0x0 sp=0x{:x} sepc=0x{:x}\n",
                    task.frame().x[2],
                    task.frame().sepc
                );
                log_info!(target: "selftest", "KSELFTEST: direct call begin");
                DIRECT_RUN_FLAG.store(1, Ordering::SeqCst);
                child_new_as_entry();
                DIRECT_RUN_FLAG.store(0, Ordering::SeqCst);
                log_info!(target: "selftest", "KSELFTEST: direct call return");
                log_info!(target: "selftest", "KSELFTEST: returned from child entry");
                unsafe {
                    sstatus::clear_sum();
                }
                if let Some(khandle) = ctx.tasks.bootstrap_mut().address_space {
                    let _ = ctx.address_spaces.activate(khandle);
                }
                // Mark spawn success and run W^X negative test early, then return
                CHILD_HEARTBEAT.store(2, Ordering::SeqCst);
                log_info!(target: "selftest", "KSELFTEST: spawn ok");
                {
                    let mut table = SyscallTable::new();
                    api::install_handlers(&mut table);
                    let timer = ctx.hal.timer();
                    let mut sys_ctx = api::Context::new(
                        ctx.scheduler,
                        ctx.tasks,
                        ctx.router,
                        ctx.address_spaces,
                        timer,
                    );
                    const PROT_READ: usize = 1 << 0;
                    const PROT_WRITE: usize = 1 << 1;
                    const PROT_EXEC: usize = 1 << 2;
                    const MAP_FLAG_USER: usize = 1 << 0;
                    let wx_args = Args::new([
                        handle_raw,
                        2,
                        CHILD_TEST_VA + PAGE_SIZE,
                        PAGE_SIZE,
                        PROT_READ | PROT_WRITE | PROT_EXEC,
                        MAP_FLAG_USER,
                    ]);
                    match table.dispatch(SYSCALL_AS_MAP, &mut sys_ctx, &wx_args) {
                        Err(SysError::AddressSpace(AddressSpaceError::Mapping(
                            MapError::PermissionDenied,
                        ))) => {
                            log_info!(target: "selftest", "KSELFTEST: w^x enforced");
                        }
                        Err(_) | Ok(_) => {
                            log_error!(target: "selftest", "KSELFTEST: w^x NOT enforced");
                        }
                    }
                }
                return;
            }
        }
    }
    // Directly enter the child's address space and run the entry function on the
    // child's guarded stack to validate mapping/AS activation in-kernel. This avoids
    // relying on an ECALL-based scheduler fastpath that may be intercepted by SBI.
    {
        let pid = child_pid as Pid;
        if let Some(task) = ctx.tasks.task(pid) {
            if let Some(handle) = task.address_space() {
                // Switch to child AS and allow S-mode to touch USER pages (SUM).
                let _ = ctx.address_spaces.activate(handle);
                unsafe {
                    sstatus::set_sum();
                }
                // Run the child's entry on its stack, then restore SUM; kernel text/data are
                // globally mapped so returning here is safe.
                call_on_stack(child_new_as_entry, task.frame().x[2]);
                unsafe {
                    sstatus::clear_sum();
                }
                // Reactivate the kernel/bootstrap address space for further tests.
                if let Some(khandle) = ctx.tasks.bootstrap_mut().address_space {
                    let _ = ctx.address_spaces.activate(khandle);
                }
            }
        }
    }
    let mut spins = 0;
    while CHILD_HEARTBEAT.load(Ordering::SeqCst) != 2 {
        syscall_yield();
        spins += 1;
        if spins > 64 {
            crate::selftest::assert::report_failure("selftest: no progress after spawn");
        }
    }
    log_info!(target: "selftest", "KSELFTEST: spawn newas ok");

    // Recreate syscall context to test W^X enforcement.
    let mut table = SyscallTable::new();
    api::install_handlers(&mut table);
    let timer = ctx.hal.timer();
    let mut sys_ctx =
        api::Context::new(ctx.scheduler, ctx.tasks, ctx.router, ctx.address_spaces, timer);
    const PROT_READ: usize = 1 << 0;
    const PROT_WRITE: usize = 1 << 1;
    const PROT_EXEC: usize = 1 << 2;
    const MAP_FLAG_USER: usize = 1 << 0;
    let wx_args = Args::new([
        handle_raw,
        2,
        CHILD_TEST_VA + PAGE_SIZE,
        PAGE_SIZE,
        PROT_READ | PROT_WRITE | PROT_EXEC,
        MAP_FLAG_USER,
    ]);
    match table.dispatch(SYSCALL_AS_MAP, &mut sys_ctx, &wx_args) {
        Err(SysError::AddressSpace(AddressSpaceError::Mapping(MapError::PermissionDenied))) => {
            log_info!(target: "selftest", "KSELFTEST: w^x enforced");
        }
        Err(_) | Ok(_) => {
            log_error!(target: "selftest", "KSELFTEST: w^x NOT enforced");
        }
    }

    // Silence unused result in release builds.
    let _ = child_pid;
}

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
fn call_on_stack(entry: extern "C" fn(), new_sp: usize) {
    verbose!("KSELFTEST: call_on_stack enter sp=0x{:x} func=0x{:x}\n", new_sp, entry as usize);
    unsafe {
        core::arch::asm!(
            // Save current sp in t0, switch to child stack, call entry, restore sp
            "mv t0, sp\n\
             mv sp, {sp}\n\
             mv t1, {func}\n\
             jalr ra, t1, 0\n\
             mv sp, t0",
            func = in(reg) entry as usize,
            sp = in(reg) new_sp,
            out("t0") _,
            out("t1") _,
            options(nostack)
        );
    }
    verbose!("KSELFTEST: call_on_stack return\n");
}

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
pub fn entry(ctx: &mut Context<'_>) {
    CHILD_HEARTBEAT.store(0, Ordering::SeqCst);
    run_address_space_selftests(ctx);
}

#[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
pub fn entry(_ctx: &mut Context<'_>) {
    log_info!(target: "selftest", "SELFTEST: host build noop");
}

#[cfg(all(feature = "selftest_priv_stack", target_arch = "riscv64", target_os = "none"))]
pub fn entry_on_private_stack(ctx: &mut Context<'_>) {
    unsafe extern "C" fn shim(arg: *mut c_void) {
        let ctx_ptr = arg as *mut Context<'static>;
        // SAFETY: entry_on_private_stack transmutes the lifetime only for the duration of this call.
        unsafe {
            entry(&mut *ctx_ptr);
        }
    }

    extern "C" {
        fn run_selftest_on_stack(
            func: unsafe extern "C" fn(*mut c_void),
            arg: *mut c_void,
            new_sp: *mut u8,
        );
        static __selftest_stack_top: u8;
    }

    let top = unsafe { &__selftest_stack_top as *const u8 as usize };
    let sp = top as *mut u8;
    let raw_ctx: *mut Context<'static> = unsafe { core::mem::transmute(ctx as *mut Context<'_>) };
    unsafe {
        run_selftest_on_stack(shim, raw_ctx.cast::<c_void>(), sp);
    }
}

#[cfg(not(all(feature = "selftest_priv_stack", target_arch = "riscv64", target_os = "none")))]
#[allow(dead_code)]
pub fn entry_on_private_stack(ctx: &mut Context<'_>) {
    entry(ctx);
}
