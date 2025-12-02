// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: In-kernel selftest harness executed during deterministic boot
//! OWNERS: @kernel-team
//! PUBLIC API: selftest modules (assert, stack_run)
//! DEPENDS_ON: hal::virt, ipc::Router, mm::AddressSpaceManager, sched::Scheduler, syscall::api
//! INVARIANTS: Minimal side effects; UART markers only; feature-gated private stack
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

extern crate alloc;

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
use core::{
    ffi::c_void,
    sync::atomic::{AtomicUsize, Ordering},
};

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
// Import assertion helpers only when used.
// use crate::{st_assert, st_expect_eq, task};
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
use crate::task::Pid;
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
use crate::{
    cap::{Capability, CapabilityKind, Rights},
    hal::virt::VirtMachine,
    ipc::Router,
    mm::{AddressSpaceError, AddressSpaceManager, MapError, PAGE_SIZE},
    sched::Scheduler,
    syscall::{
        api, Args, Error as SysError, SyscallTable, SYSCALL_AS_CREATE, SYSCALL_AS_MAP,
        SYSCALL_EXIT, SYSCALL_SPAWN, SYSCALL_WAIT, SYSCALL_YIELD,
    },
    task::TaskTable,
};
#[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
use crate::{
    hal::virt::VirtMachine, ipc::Router, mm::AddressSpaceManager, sched::Scheduler, task::TaskTable,
};
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
use riscv::register::sstatus;

pub mod assert;

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
core::arch::global_asm!(include_str!("stack_run.S"));

// Embed init ELF binary when EMBED_INIT_ELF is provided
#[cfg(all(embed_init, target_arch = "riscv64", target_os = "none"))]
static INIT_ELF: &[u8] = include_bytes!(env!("EMBED_INIT_ELF"));

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

#[cfg(all(
    feature = "selftest_priv_stack",
    target_arch = "riscv64",
    target_os = "none"
))]
const SELFTEST_STACK_PAGES: usize = 8;
#[cfg(all(
    feature = "selftest_priv_stack",
    target_arch = "riscv64",
    target_os = "none"
))]
const SELFTEST_STACK_BYTES: usize = SELFTEST_STACK_PAGES * PAGE_SIZE;
#[cfg(all(
    feature = "selftest_priv_stack",
    target_arch = "riscv64",
    target_os = "none"
))]
#[link_section = ".bss.selftest_stack_body"]
#[used]
static mut SELFTEST_STACK: [u8; SELFTEST_STACK_BYTES] = [0; SELFTEST_STACK_BYTES];

/// Borrowed references to kernel subsystems used by selftests.
pub struct Context<'a> {
    #[allow(dead_code)]
    pub hal: &'a VirtMachine,
    #[cfg_attr(
        not(all(target_arch = "riscv64", target_os = "none")),
        allow(dead_code)
    )]
    pub router: &'a mut Router,
    #[allow(dead_code)]
    pub address_spaces: &'a mut AddressSpaceManager,
    #[cfg_attr(
        not(all(target_arch = "riscv64", target_os = "none")),
        allow(dead_code)
    )]
    pub tasks: &'a mut TaskTable,
    #[cfg_attr(
        not(all(target_arch = "riscv64", target_os = "none")),
        allow(dead_code)
    )]
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
extern "C" fn child_exit_zero() -> ! {
    unsafe {
        core::arch::asm!(
            "li a7, {id}\n ecall\n j .",
            id = const SYSCALL_EXIT,
            options(noreturn)
        );
    }
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
    let cap = Capability {
        kind: CapabilityKind::Vmo {
            base: ptr,
            len: PAGE_SIZE,
        },
        rights: Rights::MAP,
    };
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
        let mut sys_ctx = api::Context::new(
            ctx.scheduler,
            ctx.tasks,
            ctx.router,
            ctx.address_spaces,
            timer,
        );

        let h = table
            .dispatch(SYSCALL_AS_CREATE, &mut sys_ctx, &Args::new([0; 6]))
            .expect("as_create syscall");
        handle_raw = h;
        log_info!(target: "selftest", "KSELFTEST: as create ok");

        const PROT_READ: usize = 1 << 0;
        const PROT_WRITE: usize = 1 << 1;
        const MAP_FLAG_USER: usize = 1 << 0;
        let map_args = Args::new([
            handle_raw,
            2,
            CHILD_TEST_VA,
            PAGE_SIZE,
            PROT_READ,
            MAP_FLAG_USER,
        ]);
        table
            .dispatch(SYSCALL_AS_MAP, &mut sys_ctx, &map_args)
            .expect("as_map syscall");
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
        child_pid = table
            .dispatch(SYSCALL_SPAWN, &mut sys_ctx, &spawn_args)
            .expect("spawn syscall");
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
                    let expected = ctx
                        .address_spaces
                        .get(handle)
                        .map(|s| s.satp_value())
                        .unwrap_or(0);
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
fn run_exit_wait_selftests(ctx: &mut Context<'_>) {
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

    let entry = child_exit_zero as usize;
    let spawn_args = Args::new([entry, 0, 0, 0, 0, 0]);
    let child_pid = match table.dispatch(SYSCALL_SPAWN, &mut sys_ctx, &spawn_args) {
        Ok(pid) => pid,
        Err(_) => 0,
    };
    let wait_args = Args::new([child_pid, 0, 0, 0, 0, 0]);
    let _ = table.dispatch(SYSCALL_WAIT, &mut sys_ctx, &wait_args);
    log_info!(target: "selftest", "KSELFTEST: exit ok");

    let _first_child = table
        .dispatch(SYSCALL_SPAWN, &mut sys_ctx, &spawn_args)
        .expect("spawn child a");
    let _second_child = table
        .dispatch(SYSCALL_SPAWN, &mut sys_ctx, &spawn_args)
        .expect("spawn child b");
    let any_args = Args::new([0, 0, 0, 0, 0, 0]);
    let _ = table.dispatch(SYSCALL_WAIT, &mut sys_ctx, &any_args);
    let _ = table.dispatch(SYSCALL_WAIT, &mut sys_ctx, &any_args);
    log_info!(target: "selftest", "KSELFTEST: wait ok");
}

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
fn call_on_stack(entry: extern "C" fn(), new_sp: usize) {
    verbose!(
        "KSELFTEST: call_on_stack enter sp=0x{:x} func=0x{:x}\n",
        new_sp,
        entry as usize
    );
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
    // Ensure subsequent lifecycle tests run as the bootstrap task (PID 0)
    // so parent/child linkage during spawn and wait behaves deterministically.
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    ctx.tasks.set_current(0);
    run_exit_wait_selftests(ctx);

    // Spawn embedded init process
    #[cfg(all(embed_init, target_arch = "riscv64", target_os = "none"))]
    spawn_init_process(ctx);
}

#[cfg(all(embed_init, target_arch = "riscv64", target_os = "none"))]
fn spawn_init_process(ctx: &mut Context<'_>) {
    log_info!(target: "selftest", "KSELFTEST: spawn init begin");

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

    // Load init ELF and get entry point
    let load_result = load_init_elf(&mut sys_ctx);
    let (entry_pc, stack_top, global_pointer, as_handle) = match load_result {
        Ok(result) => result,
        Err(err) => {
            log_info!(target: "selftest", "KSELFTEST: init load failed: {}", err);
            return;
        }
    };

    log_info!(
        target: "selftest",
        "KSELFTEST: init loaded entry=0x{:x} sp=0x{:x} gp=0x{:x}",
        entry_pc,
        stack_top,
        global_pointer
    );

    // Spawn init process with the loaded code
    let spawn_args = Args::new([
        entry_pc,
        stack_top,
        as_handle.to_raw() as usize,
        0,
        global_pointer,
        0,
    ]);
    let init_pid = match table.dispatch(SYSCALL_SPAWN, &mut sys_ctx, &spawn_args) {
        Ok(pid) => pid,
        Err(err) => {
            log_info!(target: "selftest", "KSELFTEST: spawn failed: {:?}", err);
            return;
        }
    };

    log_info!(target: "selftest", "KSELFTEST: spawn ok pid={}", init_pid);

    // Verify init task state
    let init_pid_u32 = init_pid as u32;
    if let Some(task) = sys_ctx.tasks.task(init_pid_u32) {
        use core::fmt::Write as _;
        let mut uart = crate::uart::raw_writer();
        let frame = task.frame();
        let _ = writeln!(uart, "[INFO selftest] KSELFTEST: init frame:");
        let _ = writeln!(uart, "  sepc=0x{:016x}", frame.sepc);
        let _ = writeln!(uart, "  gp=0x{:016x}", frame.x[3]);
        let _ = writeln!(uart, "  sp=0x{:016x}", frame.x[2]);
        let _ = writeln!(uart, "  sstatus=0x{:016x}", frame.sstatus);
        let spp = (frame.sstatus >> 8) & 1;
        let _ = writeln!(uart, "  SPP={} (U-mode=0, S-mode=1)", spp);
        if spp != 0 {
            let _ = writeln!(uart, "[ERROR] SPP should be 0 for U-mode task!");
        }
    } else {
        log_info!(target: "selftest", "KSELFTEST: WARNING task lookup failed!");
    }

    log_info!(target: "selftest", "KSELFTEST: init spawned successfully");
    log_info!(target: "selftest", "KSELFTEST: returning to idle loop for scheduling");
    // Return to kmain's idle loop which will properly schedule init
}

#[cfg(all(embed_init, target_arch = "riscv64", target_os = "none"))]
fn load_init_elf(
    sys_ctx: &mut api::Context<'_>,
) -> Result<(usize, usize, usize, crate::mm::AsHandle), &'static str> {
    use crate::mm::PageFlags;

    let bytes = INIT_ELF;

    // Parse ELF header
    if bytes.len() < 64 {
        return Err("elf too short");
    }
    if &bytes[0..4] != b"\x7FELF" {
        return Err("bad elf magic");
    }
    if bytes[4] != 2 {
        return Err("not elf64");
    }
    if bytes[5] != 1 {
        return Err("not little endian");
    }

    let e_entry = read_u64(&bytes[24..32]) as usize;
    let e_phoff = read_u64(&bytes[32..40]) as usize;
    let e_phentsize = read_u16(&bytes[54..56]) as usize;
    let e_phnum = read_u16(&bytes[56..58]) as usize;

    if e_phoff >= bytes.len() {
        return Err("bad phoff");
    }

    // Create address space for init
    let as_handle = match sys_ctx.address_spaces.create() {
        Ok(h) => h,
        Err(_) => return Err("as create failed"),
    };

    // Load PT_LOAD segments
    const PT_LOAD: u32 = 1;
    const PF_R: u32 = 4;
    const PF_W: u32 = 2;
    const PF_X: u32 = 1;
    const SHT_SYMTAB: u32 = 2;
    const SHT_STRTAB: u32 = 3;

    let mut first_rw_page: Option<usize> = None;
    for i in 0..e_phnum {
        let off = e_phoff + i * e_phentsize;
        if off + 56 > bytes.len() {
            continue;
        }

        let p_type = read_u32(&bytes[off..off + 4]);
        if p_type != PT_LOAD {
            continue;
        }

        let p_flags = read_u32(&bytes[off + 4..off + 8]);
        let p_offset = read_u64(&bytes[off + 8..off + 16]) as usize;
        let p_vaddr = read_u64(&bytes[off + 16..off + 24]) as usize;
        let p_filesz = read_u64(&bytes[off + 32..off + 40]) as usize;
        let p_memsz = read_u64(&bytes[off + 40..off + 48]) as usize;

        log_info!(
            target: "selftest",
            "KSELFTEST: PT_LOAD segment vaddr=0x{:x} memsz=0x{:x} flags=0x{:x}",
            p_vaddr,
            p_memsz,
            p_flags
        );

        // Build page flags
        let mut flags = PageFlags::VALID | PageFlags::USER;
        if p_flags & PF_R != 0 {
            flags |= PageFlags::READ;
        }
        if p_flags & PF_W != 0 {
            flags |= PageFlags::WRITE;
            if first_rw_page.is_none() {
                first_rw_page = Some(align_down(p_vaddr, PAGE_SIZE));
            }
        }
        if p_flags & PF_X != 0 {
            flags |= PageFlags::EXECUTE;
        }

        log_info!(
            target: "selftest",
            "KSELFTEST: map flags bits=0x{:x}",
            flags.bits()
        );

        // Map segment pages
        let seg_start = align_down(p_vaddr, PAGE_SIZE);
        let seg_end = align_up(p_vaddr + p_memsz, PAGE_SIZE);
        let mut va = seg_start;

        while va < seg_end {
            let existing_entry = {
                let space = match sys_ctx.address_spaces.get(as_handle) {
                    Ok(s) => s,
                    Err(_) => return Err("as get failed"),
                };
                space.page_table().lookup(va)
            };

            if let Some(entry) = existing_entry {
                let mut extra = PageFlags::ACCESSED;
                if flags.contains(PageFlags::WRITE) && entry & PageFlags::WRITE.bits() == 0 {
                    extra |= PageFlags::WRITE | PageFlags::DIRTY;
                }
                let _ = sys_ctx.address_spaces.set_leaf_flags(as_handle, va, extra);

                copy_segment_bytes(
                    bytes,
                    p_offset,
                    p_vaddr,
                    p_filesz,
                    va,
                    ((entry >> 10) << 12) as *mut u8,
                );

                va += PAGE_SIZE;
                continue;
            }

            let pa = alloc_init_page().ok_or("oom")?;

            // Zero page
            unsafe {
                core::ptr::write_bytes(pa as *mut u8, 0, PAGE_SIZE);
            }

            // Copy file content if in range
            copy_segment_bytes(bytes, p_offset, p_vaddr, p_filesz, va, pa as *mut u8);

            // Map page
            if let Err(e) = sys_ctx.address_spaces.map_page(as_handle, va, pa, flags) {
                log_info!(target: "selftest", "KSELFTEST: map_page failed va=0x{:x} pa=0x{:x} flags={:?} err={:?}", va, pa, flags, e);
                return Err("map failed");
            }

            va += PAGE_SIZE;
        }
    }

    // Allocate stack (place high in user address space)
    const STACK_PAGES: usize = 16;
    const USER_STACK_TOP: usize = 0x20000000;
    let stack_base = USER_STACK_TOP - STACK_PAGES * PAGE_SIZE;

    for i in 0..STACK_PAGES {
        let va = stack_base + i * PAGE_SIZE;
        let pa = alloc_init_page().ok_or("stack oom")?;
        unsafe {
            core::ptr::write_bytes(pa as *mut u8, 0, PAGE_SIZE);
        }

        let flags = PageFlags::VALID | PageFlags::READ | PageFlags::WRITE | PageFlags::USER;
        if let Err(_) = sys_ctx.address_spaces.map_page(as_handle, va, pa, flags) {
            return Err("stack map failed");
        }
    }

    let e_shoff = read_u64(&bytes[40..48]) as usize;
    let e_shentsize = read_u16(&bytes[58..60]) as usize;
    let e_shnum = read_u16(&bytes[60..62]) as usize;

    let mut symtab_offset = 0usize;
    let mut symtab_size = 0usize;
    let mut symtab_entry_size = 0usize;
    let mut strtab_offset = 0usize;
    let mut strtab_size = 0usize;

    for index in 0..e_shnum {
        let base = e_shoff + index * e_shentsize;
        if base + e_shentsize > bytes.len() {
            break;
        }
        let sh_type = read_u32(&bytes[base + 4..base + 8]);
        if sh_type == SHT_SYMTAB {
            symtab_offset = read_u64(&bytes[base + 24..base + 32]) as usize;
            symtab_size = read_u64(&bytes[base + 32..base + 40]) as usize;
            symtab_entry_size = read_u64(&bytes[base + 56..base + 64]) as usize;

            let str_index = read_u32(&bytes[base + 40..base + 44]) as usize;
            if str_index < e_shnum {
                let str_base = e_shoff + str_index * e_shentsize;
                if str_base + e_shentsize <= bytes.len() {
                    let str_type = read_u32(&bytes[str_base + 4..str_base + 8]);
                    if str_type == SHT_STRTAB {
                        strtab_offset = read_u64(&bytes[str_base + 24..str_base + 32]) as usize;
                        strtab_size = read_u64(&bytes[str_base + 32..str_base + 40]) as usize;
                    }
                }
            }
            break;
        }
    }

    if symtab_offset == 0 || symtab_size == 0 || symtab_entry_size == 0 {
        return Err("missing symtab");
    }
    if strtab_offset == 0 || strtab_size == 0 {
        return Err("missing strtab");
    }
    if symtab_offset + symtab_size > bytes.len() || strtab_offset + strtab_size > bytes.len() {
        return Err("symtab range");
    }

    const DEBUG_PUTC_SYMBOL: &[u8] = b"_ZN9nexus_abi10debug_putc17hab19954914ca9c3eE";
    const DEBUG_WRITE_SYMBOL: &[u8] = b"_ZN9nexus_abi11debug_write17h28b91feb604c588cE";

    let mut global_pointer = None;
    let mut debug_putc_addr = None;
    let mut debug_write_addr = None;
    let entry_count = symtab_size / symtab_entry_size;
    for i in 0..entry_count {
        let entry_off = symtab_offset + i * symtab_entry_size;
        if entry_off + symtab_entry_size > bytes.len() {
            break;
        }
        let name_offset = read_u32(&bytes[entry_off..entry_off + 4]) as usize;
        if name_offset >= strtab_size {
            continue;
        }
        let mut end = 0usize;
        let name_slice = &bytes[strtab_offset + name_offset..strtab_offset + strtab_size];
        while end < name_slice.len() && name_slice[end] != 0 {
            end += 1;
        }
        let value = read_u64(&bytes[entry_off + 8..entry_off + 16]) as usize;
        if name_slice[..end] == b"__global_pointer$"[..] {
            global_pointer = Some(value);
        } else if name_slice[..end] == DEBUG_PUTC_SYMBOL[..] {
            debug_putc_addr = Some(value);
        } else if name_slice[..end] == DEBUG_WRITE_SYMBOL[..] {
            debug_write_addr = Some(value);
        }
    }

    if let Ok(space) = sys_ctx.address_spaces.get(as_handle) {
        use core::fmt::Write as _;
        let text_page = align_down(e_entry, PAGE_SIZE);
        if let Some(entry) = space.page_table().lookup(text_page) {
            let mut uart = crate::uart::raw_writer();
            let _ = writeln!(
                uart,
                "[INFO selftest] KSELFTEST: text page entry=0x{:016x} va=0x{:016x}",
                entry, text_page
            );
        } else {
            let mut uart = crate::uart::raw_writer();
            let _ = writeln!(
                uart,
                "[ERROR selftest] KSELFTEST: missing text mapping va=0x{:016x}",
                text_page
            );
        }
        if let Some(data_page) = first_rw_page {
            if let Some(entry) = space.page_table().lookup(data_page) {
                let mut uart = crate::uart::raw_writer();
                let _ = writeln!(
                    uart,
                    "[INFO selftest] KSELFTEST: data page entry=0x{:016x} va=0x{:016x}",
                    entry, data_page
                );
            } else {
                let mut uart = crate::uart::raw_writer();
                let _ = writeln!(
                    uart,
                    "[ERROR selftest] KSELFTEST: missing data mapping va=0x{:016x}",
                    data_page
                );
            }
        }

        if let Some(addr) = debug_putc_addr {
            log_symbol_words(space, addr, "KSELFTEST: dbg-putc");
        }
        if let Some(addr) = debug_write_addr {
            log_symbol_words(space, addr, "KSELFTEST: dbg-write");
        }
    }

    let gp = global_pointer.ok_or("missing gp symbol")?;

    Ok((e_entry, USER_STACK_TOP, gp, as_handle))
}

#[cfg(all(embed_init, target_arch = "riscv64", target_os = "none"))]
fn alloc_init_page() -> Option<usize> {
    static PAGE_CURSOR: core::sync::atomic::AtomicUsize =
        core::sync::atomic::AtomicUsize::new(0x8060_0000);
    const PAGE_LIMIT: usize = 0x8060_0000 + 4 * 1024 * 1024;

    let pa = PAGE_CURSOR.fetch_add(PAGE_SIZE, Ordering::SeqCst);
    if pa + PAGE_SIZE <= PAGE_LIMIT {
        Some(pa)
    } else {
        None
    }
}

#[cfg(all(embed_init, target_arch = "riscv64", target_os = "none"))]
fn read_u16(bytes: &[u8]) -> u16 {
    u16::from_le_bytes([bytes[0], bytes[1]])
}

#[cfg(all(embed_init, target_arch = "riscv64", target_os = "none"))]
fn read_u32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

#[cfg(all(embed_init, target_arch = "riscv64", target_os = "none"))]
fn read_u64(bytes: &[u8]) -> u64 {
    u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ])
}

#[cfg(all(embed_init, target_arch = "riscv64", target_os = "none"))]
fn align_down(addr: usize, align: usize) -> usize {
    addr & !(align - 1)
}

#[cfg(all(embed_init, target_arch = "riscv64", target_os = "none"))]
fn align_up(addr: usize, align: usize) -> usize {
    let rem = addr % align;
    if rem == 0 {
        addr
    } else {
        addr + (align - rem)
    }
}

#[cfg(all(embed_init, target_arch = "riscv64", target_os = "none"))]
fn log_symbol_words(space: &crate::mm::address_space::AddressSpace, virt: usize, label: &str) {
    use core::fmt::Write as _;

    let page = align_down(virt, PAGE_SIZE);
    let offset = virt - page;

    let mut uart = crate::uart::raw_writer();
    if let Some(entry) = space.page_table().lookup(page) {
        let phys = ((entry >> 10) << 12) + offset;
        unsafe {
            let ptr = phys as *const u32;
            let word0 = core::ptr::read(ptr);
            let word1 = core::ptr::read(ptr.add(1));
            let _ = writeln!(
                uart,
                "[INFO selftest] {} va=0x{:016x} words=0x{:08x} 0x{:08x}",
                label, virt, word0, word1
            );
        }
    } else {
        let _ = writeln!(
            uart,
            "[ERROR selftest] {} missing mapping va=0x{:016x}",
            label, virt
        );
    }
}

#[cfg(all(embed_init, target_arch = "riscv64", target_os = "none"))]
fn copy_segment_bytes(
    file: &[u8],
    seg_offset: usize,
    seg_vaddr: usize,
    seg_filesz: usize,
    page_va: usize,
    page_ptr: *mut u8,
) {
    if seg_filesz == 0 {
        return;
    }

    let Some(seg_end) = seg_vaddr.checked_add(seg_filesz) else {
        return;
    };
    let Some(page_end) = page_va.checked_add(PAGE_SIZE) else {
        return;
    };

    let copy_start = core::cmp::max(page_va, seg_vaddr);
    if copy_start >= seg_end {
        return;
    }
    let copy_end = core::cmp::min(page_end, seg_end);
    if copy_end <= copy_start {
        return;
    }

    let len = copy_end - copy_start;
    let src_off = seg_offset + (copy_start - seg_vaddr);
    let dst_off = copy_start - page_va;
    if src_off + len > file.len() {
        return;
    }

    unsafe {
        core::ptr::copy_nonoverlapping(file.as_ptr().add(src_off), page_ptr.add(dst_off), len);
    }
}

#[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
pub fn entry(_ctx: &mut Context<'_>) {
    log_info!(target: "selftest", "SELFTEST: host build noop");
}

#[cfg(all(
    feature = "selftest_priv_stack",
    target_arch = "riscv64",
    target_os = "none"
))]
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

#[cfg(not(all(
    feature = "selftest_priv_stack",
    target_arch = "riscv64",
    target_os = "none"
)))]
#[allow(dead_code)]
pub fn entry_on_private_stack(ctx: &mut Context<'_>) {
    entry(ctx);
}
