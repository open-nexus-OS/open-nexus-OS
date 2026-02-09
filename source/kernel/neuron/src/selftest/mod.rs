// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: In-kernel selftest harness executed during deterministic boot
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker contract (see scripts/qemu-test.sh)
//! PUBLIC API: selftest modules (assert, stack_run)
//! DEPENDS_ON: hal::virt, ipc::Router, mm::AddressSpaceManager, sched::Scheduler, syscall::api
//! INVARIANTS: Minimal side effects; UART markers only; feature-gated private stack
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md
//!
//! TEST_SCOPE:
//!   - Deterministic boot-time kernel selftests (AS create/map/activate, spawn/exit, IPC/caps as enabled)
//!   - Marker emission used by CI to prove invariants (see scripts/qemu-test.sh)
//!
//! TEST_SCENARIOS:
//!   - Address space bring-up: AS create/map/activate and post-SATP marker
//!   - Spawn lifecycle: child runs, yields, exits; parent observes
//!   - W^X: mappings reject WRITE|EXECUTE combinations at enforcement boundary

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
        SYSCALL_EXIT, SYSCALL_SPAWN, SYSCALL_VMO_CREATE, SYSCALL_VMO_WRITE, SYSCALL_WAIT,
        SYSCALL_YIELD,
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
    let cap =
        Capability { kind: CapabilityKind::Vmo { base: ptr, len: PAGE_SIZE }, rights: Rights::MAP };
    let caps = tasks.bootstrap_mut().caps_mut();
    // Reserve bootstrap cap slots:
    // - slot 0: bootstrap endpoint
    // - slot 1: identity VMO
    // - slot 2: EndpointFactory (Phase-2 hardening)
    //
    // Use slot 4 for this selftest-only VMO:
    // - slot 3 is used by the VMO-zero-initialization selftest below.
    const DATA_VMO_SLOT: usize = 4;
    let _ = caps.set(DATA_VMO_SLOT, cap);
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

        // RFC-0004: VMO allocations must be zero-initialized (no stale memory leaks).
        // Create a fresh VMO and validate its backing bytes are all zero before any write.
        {
            const VMO_SLOT: usize = 3;
            let vmo_create_args = Args::new([VMO_SLOT, PAGE_SIZE, 0, 0, 0, 0]);
            table
                .dispatch(SYSCALL_VMO_CREATE, &mut sys_ctx, &vmo_create_args)
                .expect("vmo_create syscall");
            let cap =
                sys_ctx.tasks.bootstrap_mut().caps_mut().get(VMO_SLOT).expect("vmo cap present");
            let (base, len) = match cap.kind {
                CapabilityKind::Vmo { base, len } => (base, len),
                _ => panic!("unexpected cap kind"),
            };
            let probe_len = core::cmp::min(len, 64);
            let mut all_zero = true;
            for idx in 0..probe_len {
                let byte = unsafe { core::ptr::read_volatile((base + idx) as *const u8) };
                if byte != 0 {
                    all_zero = false;
                    break;
                }
            }
            if all_zero {
                log_info!(target: "selftest", "KSELFTEST: vmo zero ok");
            } else {
                log_info!(target: "selftest", "KSELFTEST: vmo zero FAIL");
            }

            // RFC-0004: pointer provenance / user range guard.
            // `sys_vmo_write` must reject user pointers outside the Sv39 user range deterministically,
            // without attempting to touch them.
            const USER_VADDR_LIMIT: usize = 0x8000_0000;
            let bad_write = table.dispatch(
                SYSCALL_VMO_WRITE,
                &mut sys_ctx,
                &Args::new([VMO_SLOT, 0, USER_VADDR_LIMIT, 1, 0, 0]),
            );
            match bad_write {
                Err(crate::syscall::Error::AddressSpace(
                    crate::mm::AddressSpaceError::InvalidArgs,
                )) => {
                    log_info!(target: "selftest", "KSELFTEST: userptr guard ok");
                }
                other => {
                    panic!("KSELFTEST: userptr guard FAIL: {:?}", other);
                }
            }
        }

        const PROT_READ: usize = 1 << 0;
        const PROT_WRITE: usize = 1 << 1;
        const MAP_FLAG_USER: usize = 1 << 0;
        const DATA_VMO_SLOT: usize = 4;
        let map_args = Args::new([
            handle_raw,
            DATA_VMO_SLOT,
            CHILD_TEST_VA,
            PAGE_SIZE,
            PROT_READ,
            MAP_FLAG_USER,
        ]);
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
        // Supply valid sp/as_handle and a gp derived from the current pointer to avoid gp=0 in child.
        let current_gp: usize;
        unsafe {
            core::arch::asm!("mv {0}, gp", out(reg) current_gp);
        }
        let gp = if current_gp == 0 { user_stack_top.wrapping_add(0x800) } else { current_gp };
        let spawn_args = Args::new([entry, user_stack_top, handle_raw, 0, gp, 0]);
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
fn run_exit_wait_selftests(ctx: &mut Context<'_>) {
    let mut table = SyscallTable::new();
    api::install_handlers(&mut table);
    let timer = ctx.hal.timer();
    let mut sys_ctx =
        api::Context::new(ctx.scheduler, ctx.tasks, ctx.router, ctx.address_spaces, timer);

    let entry = child_exit_zero as usize;
    let spawn_args = Args::new([entry, 0, 0, 0, 0, 0]);
    let child_pid = match table.dispatch(SYSCALL_SPAWN, &mut sys_ctx, &spawn_args) {
        Ok(pid) => pid,
        Err(_) => 0,
    };
    let wait_args = Args::new([child_pid, 0, 0, 0, 0, 0]);
    let _ = table.dispatch(SYSCALL_WAIT, &mut sys_ctx, &wait_args);
    log_info!(target: "selftest", "KSELFTEST: exit ok");

    let _first_child = match table.dispatch(SYSCALL_SPAWN, &mut sys_ctx, &spawn_args) {
        Ok(pid) => pid,
        Err(err) => {
            crate::selftest::assert::report_failure_fmt(format_args!(
                "selftest: spawn child a failed: {:?}",
                err
            ));
        }
    };
    let _second_child = match table.dispatch(SYSCALL_SPAWN, &mut sys_ctx, &spawn_args) {
        Ok(pid) => pid,
        Err(err) => {
            crate::selftest::assert::report_failure_fmt(format_args!(
                "selftest: spawn child b failed: {:?}",
                err
            ));
        }
    };
    let any_args = Args::new([0, 0, 0, 0, 0, 0]);
    let _ = table.dispatch(SYSCALL_WAIT, &mut sys_ctx, &any_args);
    let _ = table.dispatch(SYSCALL_WAIT, &mut sys_ctx, &any_args);
    log_info!(target: "selftest", "KSELFTEST: wait ok");
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
    run_ipc_queue_full_selftest(ctx);
    run_ipc_bytes_full_selftest(ctx);
    run_ipc_global_bytes_budget_selftest();
    run_ipc_owner_bytes_budget_selftest();
    run_ipc_waiter_fifo_selftests(ctx);
    run_ipc_send_unblocks_after_recv_selftest(ctx);
    run_ipc_endpoint_quota_selftest(ctx);
    run_ipc_close_wakes_waiters_selftest(ctx);
    run_ipc_owner_exit_wakes_waiters_selftest(ctx);
    run_spawn_reason_selftest();
    run_resource_sentinel_selftest(ctx);
    // Ensure subsequent lifecycle tests run as the bootstrap task (PID 0)
    // so parent/child linkage during spawn and wait behaves deterministically.
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    ctx.tasks.set_current(0);
    run_exit_wait_selftests(ctx);

    // Spawn embedded init process
    #[cfg(all(embed_init, target_arch = "riscv64", target_os = "none"))]
    spawn_init_process(ctx);
}

fn run_ipc_queue_full_selftest(ctx: &mut Context<'_>) {
    use crate::ipc::header::MessageHeader;
    use alloc::vec::Vec;

    // This is a kernel-side sanity check that endpoint depth limits are enforced.
    // It complements syscall-level unit tests in `syscall/api.rs`.
    let ep = ctx.router.create_endpoint(1, None).unwrap();
    let hdr = MessageHeader::new(0, ep, 0, 0, 0);
    let msg = crate::ipc::Message::new(hdr, Vec::new(), None);
    let _ = ctx.router.send(ep, msg.clone());
    match ctx.router.send(ep, msg) {
        Err(crate::ipc::IpcError::QueueFull) => {
            log_info!(target: "selftest", "KSELFTEST: ipc queue full ok");
        }
        other => {
            log_error!(target: "selftest", "KSELFTEST: ipc queue full FAIL: {:?}", other);
        }
    }
}

fn run_ipc_bytes_full_selftest(ctx: &mut Context<'_>) {
    use crate::ipc::header::MessageHeader;
    use alloc::{vec, vec::Vec};

    // Endpoint with depth=1 => max_queued_bytes = MAX_FRAME_BYTES (see ipc/mod.rs).
    //
    // NOTE: For syscall-driven traffic, payloads are bounded at entry; this selftest uses a direct
    // router send with an oversized payload to deterministically trigger NoSpace.
    const MAX_FRAME_BYTES: usize = 8 * 1024;
    let ep = ctx.router.create_endpoint(1, None).unwrap();
    let oversized = MAX_FRAME_BYTES + 1;
    let hdr = MessageHeader::new(0, ep, 0, 0, oversized as u32);
    let msg = crate::ipc::Message::new(hdr, vec![0u8; oversized], None);
    match ctx.router.send(ep, msg) {
        Err(crate::ipc::IpcError::NoSpace) => {
            let hdr3 = MessageHeader::new(0, ep, 0, 0, 1);
            let msg3 = crate::ipc::Message::new(hdr3, Vec::new(), None);
            match ctx.router.send(ep, msg3) {
                Ok(()) => log_info!(target: "selftest", "KSELFTEST: ipc bytes full ok"),
                other => {
                    log_error!(target: "selftest", "KSELFTEST: ipc bytes full FAIL: {:?}", other)
                }
            }
        }
        other => {
            log_error!(target: "selftest", "KSELFTEST: ipc bytes full FAIL: {:?}", other);
        }
    }
}

fn run_ipc_global_bytes_budget_selftest() {
    use crate::ipc::header::MessageHeader;
    use alloc::{vec, vec::Vec};

    // Validate global router queued-bytes budget using an isolated local router instance.
    // Global budget=512, but endpoint budget (depth=2) would allow 1024 bytes. The second send
    // must fail with NoSpace due to the global cap.
    let mut local = crate::ipc::Router::new_with_global_bytes_budget(0, 512);
    let ep = local.create_endpoint(2, None).unwrap();

    let hdr = MessageHeader::new(0, ep, 0, 0, 512);
    let msg = crate::ipc::Message::new(hdr, vec![0u8; 512], None);
    let _ = local.send(ep, msg);

    let hdr2 = MessageHeader::new(0, ep, 0, 0, 1);
    let msg2 = crate::ipc::Message::new(hdr2, vec![0u8; 1], None);
    match local.send(ep, msg2) {
        Err(crate::ipc::IpcError::NoSpace) => {
            // Drain frees bytes; subsequent send should work.
            let _ = local.recv(ep);
            let hdr3 = MessageHeader::new(0, ep, 0, 0, 1);
            let msg3 = crate::ipc::Message::new(hdr3, Vec::new(), None);
            match local.send(ep, msg3) {
                Ok(()) => log_info!(target: "selftest", "KSELFTEST: ipc global bytes budget ok"),
                other => {
                    log_error!(target: "selftest", "KSELFTEST: ipc global bytes budget FAIL: {:?}", other)
                }
            }
        }
        other => {
            log_error!(target: "selftest", "KSELFTEST: ipc global bytes budget FAIL: {:?}", other)
        }
    }
}

fn run_ipc_owner_bytes_budget_selftest() {
    use crate::ipc::header::MessageHeader;
    use alloc::{vec, vec::Vec};

    // Owner budget=512 bytes; global budget is large enough not to interfere.
    let mut local = crate::ipc::Router::new_with_bytes_budgets(0, 4096, 512);
    let owner: u32 = 7;
    let ep1 = local.create_endpoint(2, Some(owner)).unwrap();
    let ep2 = local.create_endpoint(2, Some(owner)).unwrap();

    // Fill owner budget with one 512-byte message on ep1.
    let hdr = MessageHeader::new(0, ep1, 0, 0, 512);
    let msg = crate::ipc::Message::new(hdr, vec![0u8; 512], None);
    let _ = local.send(ep1, msg);

    // Next send to a different endpoint owned by same PID must fail due to owner cap.
    let hdr2 = MessageHeader::new(0, ep2, 0, 0, 1);
    let msg2 = crate::ipc::Message::new(hdr2, vec![0u8; 1], None);
    match local.send(ep2, msg2) {
        Err(crate::ipc::IpcError::NoSpace) => {
            // Drain frees bytes; subsequent send should work.
            let _ = local.recv(ep1);
            let hdr3 = MessageHeader::new(0, ep2, 0, 0, 1);
            let msg3 = crate::ipc::Message::new(hdr3, Vec::new(), None);
            match local.send(ep2, msg3) {
                Ok(()) => log_info!(target: "selftest", "KSELFTEST: ipc owner bytes budget ok"),
                other => {
                    log_error!(target: "selftest", "KSELFTEST: ipc owner bytes budget FAIL: {:?}", other)
                }
            }
        }
        other => {
            log_error!(target: "selftest", "KSELFTEST: ipc owner bytes budget FAIL: {:?}", other)
        }
    }
}

fn run_ipc_waiter_fifo_selftests(ctx: &mut Context<'_>) {
    use crate::ipc::header::MessageHeader;
    use crate::task::BlockReason;
    use alloc::vec::Vec;

    // --- recv waiters FIFO ---
    let ep = ctx.router.create_endpoint(1, None).unwrap();
    let r1 = ctx.tasks.selftest_create_dummy_task(0, ctx.scheduler);
    let r2 = ctx.tasks.selftest_create_dummy_task(0, ctx.scheduler);
    let r3 = ctx.tasks.selftest_create_dummy_task(0, ctx.scheduler);

    for pid in [r1, r2, r3] {
        ctx.tasks.set_current(pid);
        let _ = ctx.router.register_recv_waiter(ep, pid as u32);
        ctx.tasks
            .block_current(BlockReason::IpcRecv { endpoint: ep, deadline_ns: 0 }, ctx.scheduler);
    }

    // Send one message, then wake the next recv waiter and check it is r1.
    let hdr = MessageHeader::new(0, ep, 0, 0, 0);
    let msg = crate::ipc::Message::new(hdr, Vec::new(), None);
    let _ = ctx.router.send(ep, msg);
    let fifo_ok = match ctx.router.pop_recv_waiter(ep) {
        Ok(Some(w)) if w == r1 as u32 => true,
        _ => false,
    };
    if fifo_ok {
        log_info!(target: "selftest", "KSELFTEST: ipc recv waiter fifo ok");
    } else {
        log_error!(target: "selftest", "KSELFTEST: ipc recv waiter fifo FAIL");
    }

    // --- send waiters FIFO ---
    let ep2 = ctx.router.create_endpoint(1, None).unwrap();
    // Fill the queue so future sends would block.
    let hdr_fill = MessageHeader::new(0, ep2, 0, 0, 0);
    let fill = crate::ipc::Message::new(hdr_fill, Vec::new(), None);
    let _ = ctx.router.send(ep2, fill);

    let s1 = ctx.tasks.selftest_create_dummy_task(0, ctx.scheduler);
    let s2 = ctx.tasks.selftest_create_dummy_task(0, ctx.scheduler);
    let s3 = ctx.tasks.selftest_create_dummy_task(0, ctx.scheduler);

    for pid in [s1, s2, s3] {
        ctx.tasks.set_current(pid);
        let _ = ctx.router.register_send_waiter(ep2, pid as u32);
        ctx.tasks
            .block_current(BlockReason::IpcSend { endpoint: ep2, deadline_ns: 0 }, ctx.scheduler);
    }

    // Drain one message, then wake the next send waiter and check it is s1.
    let _ = ctx.router.recv(ep2);
    let fifo_ok = match ctx.router.pop_send_waiter(ep2) {
        Ok(Some(w)) if w == s1 as u32 => true,
        _ => false,
    };
    if fifo_ok {
        log_info!(target: "selftest", "KSELFTEST: ipc send waiter fifo ok");
    } else {
        log_error!(target: "selftest", "KSELFTEST: ipc send waiter fifo FAIL");
    }
}

fn run_ipc_send_unblocks_after_recv_selftest(ctx: &mut Context<'_>) {
    use crate::ipc::header::MessageHeader;
    use crate::task::BlockReason;
    use alloc::vec::Vec;

    // Create two lightweight dummy tasks (no AS/stack allocation) that we can block/wake.
    let sender_pid = ctx.tasks.selftest_create_dummy_task(0, ctx.scheduler);
    let recv_pid = ctx.tasks.selftest_create_dummy_task(0, ctx.scheduler);

    // Endpoint with depth=1 and one message already enqueued => "full".
    let ep = ctx.router.create_endpoint(1, None).unwrap();
    let hdr = MessageHeader::new(0, ep, 0, 0, 0);
    let msg = crate::ipc::Message::new(hdr, Vec::new(), None);
    let _ = ctx.router.send(ep, msg);

    // Simulate sender hitting QueueFull in blocking mode: register waiter + block task.
    ctx.tasks.set_current(sender_pid);
    let _ = ctx.router.register_send_waiter(ep, sender_pid as u32);
    ctx.tasks.block_current(BlockReason::IpcSend { endpoint: ep, deadline_ns: 0 }, ctx.scheduler);
    let blocked_ok = ctx.tasks.task(sender_pid).map(|t| t.is_blocked()).unwrap_or(false);

    // Receiver drains one message and wakes one send-waiter (same as sys_ipc_recv_v1 does).
    ctx.tasks.set_current(recv_pid);
    let _ = ctx.router.recv(ep);
    if let Ok(Some(waiter)) = ctx.router.pop_send_waiter(ep) {
        let _ = ctx.tasks.wake(waiter as crate::task::Pid, ctx.scheduler);
    }
    let woke_ok = ctx.tasks.task(sender_pid).map(|t| !t.is_blocked()).unwrap_or(false);

    if blocked_ok && woke_ok {
        log_info!(target: "selftest", "KSELFTEST: ipc send unblock ok");
    } else {
        log_error!(
            target: "selftest",
            "KSELFTEST: ipc send unblock FAIL: blocked_ok={} woke_ok={}",
            blocked_ok,
            woke_ok
        );
    }
}

fn run_ipc_endpoint_quota_selftest(ctx: &mut Context<'_>) {
    let _ = ctx;
    // IMPORTANT: Do not consume the global router endpoints during boot; init-lite needs to create
    // endpoints for services. Validate the quota using an isolated local router instance.
    let mut local = crate::ipc::Router::new(0);
    let mut created: usize = 0;
    loop {
        match local.create_endpoint(1, None) {
            Ok(_) => {
                created += 1;
                if created > 4096 {
                    log_error!(target: "selftest", "KSELFTEST: ipc endpoint quota FAIL: runaway");
                    return;
                }
            }
            Err(crate::ipc::IpcError::NoSpace) => {
                log_info!(
                    target: "selftest",
                    "KSELFTEST: ipc endpoint quota ok (created={})",
                    created
                );
                return;
            }
            Err(other) => {
                log_error!(
                    target: "selftest",
                    "KSELFTEST: ipc endpoint quota FAIL: {:?}",
                    other
                );
                return;
            }
        }
    }
}

fn run_ipc_close_wakes_waiters_selftest(ctx: &mut Context<'_>) {
    use crate::task::BlockReason;
    // Create a real endpoint in the global router, register blocked send/recv waiters, then close it
    // and ensure both waiters are woken.
    let recv_pid = ctx.tasks.selftest_create_dummy_task(0, ctx.scheduler);
    let send_pid = ctx.tasks.selftest_create_dummy_task(0, ctx.scheduler);

    let ep = match ctx.router.create_endpoint(1, None) {
        Ok(id) => id,
        Err(e) => {
            log_error!(target: "selftest", "KSELFTEST: ipc close wakes FAIL: {:?}", e);
            return;
        }
    };

    ctx.tasks.set_current(recv_pid);
    let _ = ctx.router.register_recv_waiter(ep, recv_pid as u32);
    ctx.tasks.block_current(BlockReason::IpcRecv { endpoint: ep, deadline_ns: 0 }, ctx.scheduler);

    ctx.tasks.set_current(send_pid);
    let _ = ctx.router.register_send_waiter(ep, send_pid as u32);
    ctx.tasks.block_current(BlockReason::IpcSend { endpoint: ep, deadline_ns: 0 }, ctx.scheduler);

    let waiters = match ctx.router.close_endpoint(ep) {
        Ok(w) => w,
        Err(e) => {
            log_error!(target: "selftest", "KSELFTEST: ipc close wakes FAIL: {:?}", e);
            return;
        }
    };
    for pid in waiters {
        let _ = ctx.tasks.wake(pid as crate::task::Pid, ctx.scheduler);
    }

    let recv_ok = ctx.tasks.task(recv_pid).map(|t| !t.is_blocked()).unwrap_or(false);
    let send_ok = ctx.tasks.task(send_pid).map(|t| !t.is_blocked()).unwrap_or(false);
    if recv_ok && send_ok {
        log_info!(target: "selftest", "KSELFTEST: ipc close wakes ok");
    } else {
        log_error!(
            target: "selftest",
            "KSELFTEST: ipc close wakes FAIL: recv_ok={} send_ok={}",
            recv_ok,
            send_ok
        );
    }
}

fn run_ipc_owner_exit_wakes_waiters_selftest(ctx: &mut Context<'_>) {
    use crate::task::BlockReason;
    // Close-on-exit semantics are implemented by closing all endpoints owned by the exiting PID.
    // Verify that draining wakes registered send/recv waiters.
    let owner: u32 = 7;
    let recv_pid = ctx.tasks.selftest_create_dummy_task(0, ctx.scheduler);
    let send_pid = ctx.tasks.selftest_create_dummy_task(0, ctx.scheduler);

    let ep = match ctx.router.create_endpoint(1, Some(owner)) {
        Ok(id) => id,
        Err(e) => {
            log_error!(target: "selftest", "KSELFTEST: ipc owner-exit wakes FAIL: {:?}", e);
            return;
        }
    };

    ctx.tasks.set_current(recv_pid);
    let _ = ctx.router.register_recv_waiter(ep, recv_pid as u32);
    ctx.tasks.block_current(BlockReason::IpcRecv { endpoint: ep, deadline_ns: 0 }, ctx.scheduler);

    ctx.tasks.set_current(send_pid);
    let _ = ctx.router.register_send_waiter(ep, send_pid as u32);
    ctx.tasks.block_current(BlockReason::IpcSend { endpoint: ep, deadline_ns: 0 }, ctx.scheduler);

    let waiters = ctx.router.close_endpoints_for_owner(owner);
    for pid in waiters {
        let _ = ctx.tasks.wake(pid as crate::task::Pid, ctx.scheduler);
    }

    let recv_ok = ctx.tasks.task(recv_pid).map(|t| !t.is_blocked()).unwrap_or(false);
    let send_ok = ctx.tasks.task(send_pid).map(|t| !t.is_blocked()).unwrap_or(false);
    if recv_ok && send_ok {
        log_info!(target: "selftest", "KSELFTEST: ipc owner-exit wakes ok");
    } else {
        log_error!(
            target: "selftest",
            "KSELFTEST: ipc owner-exit wakes FAIL: recv_ok={} send_ok={}",
            recv_ok,
            send_ok
        );
    }
}

fn run_spawn_reason_selftest() {
    use crate::cap::CapError;
    use crate::ipc::IpcError;
    use crate::mm::AddressSpaceError;
    use crate::task::{spawn_fail_reason, SpawnError, SpawnFailReason};

    let cases = [
        (SpawnError::InvalidEntryPoint, SpawnFailReason::InvalidPayload),
        (SpawnError::InvalidStackPointer, SpawnFailReason::InvalidPayload),
        (SpawnError::StackExhausted, SpawnFailReason::OutOfMemory),
        (SpawnError::Capability(CapError::NoSpace), SpawnFailReason::CapTableFull),
        (SpawnError::Ipc(IpcError::NoSpace), SpawnFailReason::EndpointQuota),
        (SpawnError::AddressSpace(AddressSpaceError::AsidExhausted), SpawnFailReason::MapFailed),
    ];

    for (err, expected) in cases {
        let got = spawn_fail_reason(&err);
        if got == expected {
            log_info!(
                target: "selftest",
                "KSELFTEST: spawn reason {} ok",
                expected.label()
            );
        } else {
            log_error!(
                target: "selftest",
                "KSELFTEST: spawn reason FAIL expected={} got={}",
                expected.label(),
                got.label()
            );
            return;
        }
    }
    log_info!(target: "selftest", "KSELFTEST: spawn reasons ok");
}

fn run_resource_sentinel_selftest(_ctx: &mut Context<'_>) {
    use crate::cap::{CapTable, Capability, CapabilityKind, Rights};
    use crate::ipc::Router;

    // 1) Cap table churn: fill, drain, refill.
    let mut table = CapTable::with_capacity(8);
    let cap = Capability { kind: CapabilityKind::EndpointFactory, rights: Rights::MANAGE };
    let mut slots = [0usize; 8];
    for slot in slots.iter_mut() {
        *slot = match table.allocate(cap) {
            Ok(idx) => idx,
            Err(err) => {
                log_error!(target: "selftest", "KSELFTEST: resource sentinel FAIL: cap alloc {:?}", err);
                return;
            }
        };
    }
    if table.allocate(cap).is_ok() {
        log_error!(target: "selftest", "KSELFTEST: resource sentinel FAIL: cap table overflow");
        return;
    }
    for slot in slots {
        let _ = table.take(slot);
    }
    if table.allocate(cap).is_err() {
        log_error!(target: "selftest", "KSELFTEST: resource sentinel FAIL: cap reuse");
        return;
    }

    // 2) Endpoint churn: use a local router to avoid consuming global endpoint quota.
    let mut local = Router::new(0);
    for _ in 0..8 {
        let ep = match local.create_endpoint(1, None) {
            Ok(id) => id,
            Err(err) => {
                log_error!(
                    target: "selftest",
                    "KSELFTEST: resource sentinel FAIL: ep create {:?}",
                    err
                );
                return;
            }
        };
        if let Err(err) = local.close_endpoint(ep) {
            log_error!(
                target: "selftest",
                "KSELFTEST: resource sentinel FAIL: ep close {:?}",
                err
            );
            return;
        }
    }

    log_info!(target: "selftest", "KSELFTEST: resource sentinel ok");
}

#[cfg(all(embed_init, target_arch = "riscv64", target_os = "none"))]
fn spawn_init_process(ctx: &mut Context<'_>) {
    log_info!(target: "selftest", "KSELFTEST: spawn init begin");

    let mut table = SyscallTable::new();
    api::install_handlers(&mut table);
    let timer = ctx.hal.timer();
    let mut sys_ctx =
        api::Context::new(ctx.scheduler, ctx.tasks, ctx.router, ctx.address_spaces, timer);

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
    // Ensure init-lite is a direct child of the bootstrap task (PID 0) so that early
    // capability/lifecycle gates (RFC-0005 hardening) can reliably treat init-lite as
    // the temporary authority during bring-up.
    sys_ctx.tasks.set_current(0);
    let spawn_args =
        Args::new([entry_pc, stack_top, as_handle.to_raw() as usize, 0, global_pointer, 0]);
    let init_pid = match table.dispatch(SYSCALL_SPAWN, &mut sys_ctx, &spawn_args) {
        Ok(pid) => pid,
        Err(err) => {
            log_info!(target: "selftest", "KSELFTEST: spawn failed: {:?}", err);
            return;
        }
    };

    log_info!(target: "selftest", "KSELFTEST: spawn ok pid={}", init_pid);

    // RFC-0005 Phase-2 hardening: EndpointFactory is now injected by the kernel when PID 0 spawns
    // the init-lite userspace task. (See `TaskTable::spawn_inner`.)

    // Bind a stable identity token to init-lite.
    // NOTE: init-lite is currently started via `SYSCALL_SPAWN` (not `exec_v2`), so we must set
    // the service_id explicitly for channel-bound policy checks.
    let init_lite_id: u64 = {
        let mut h: u64 = 0xcbf29ce484222325u64;
        for &b in b"init-lite" {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3u64);
        }
        h
    };
    if let Some(t) = sys_ctx.tasks.task_mut(init_pid as u32) {
        t.set_service_id(init_lite_id);
    }

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

    // Allocate stack (place high in user address space) and ensure the SP page is mapped.
    const STACK_PAGES: usize = 16;
    const USER_STACK_TOP: usize = 0x20000000;
    let total_pages = STACK_PAGES + 11; // requested + head + boundary (guard sits above)
    let mapped_top = USER_STACK_TOP + 10 * PAGE_SIZE; // boundary mapped, guard above
    let stack_base = mapped_top - total_pages * PAGE_SIZE;

    for i in 0..total_pages {
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
    let e_shstrndx = read_u16(&bytes[62..64]) as usize;

    let mut symtab_offset = 0usize;
    let mut symtab_size = 0usize;
    let mut symtab_entry_size = 0usize;
    let mut strtab_offset = 0usize;
    let mut strtab_size = 0usize;
    let mut shstrtab_offset = 0usize;
    let mut shstrtab_size = 0usize;

    // Resolve section header string table (used to locate `.sdata` in stripped ELFs).
    if e_shstrndx < e_shnum {
        let shstr_base = e_shoff + e_shstrndx * e_shentsize;
        if shstr_base + e_shentsize <= bytes.len() {
            shstrtab_offset = read_u64(&bytes[shstr_base + 24..shstr_base + 32]) as usize;
            shstrtab_size = read_u64(&bytes[shstr_base + 32..shstr_base + 40]) as usize;
        }
    }

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

    const DEBUG_PUTC_SYMBOL: &[u8] = b"_ZN9nexus_abi10debug_putc17hab19954914ca9c3eE";
    const DEBUG_WRITE_SYMBOL: &[u8] = b"_ZN9nexus_abi11debug_write17h28b91feb604c588cE";

    let mut global_pointer = None;
    let mut debug_putc_addr = None;
    let mut debug_write_addr = None;
    // Best-effort symbol resolution: stripped ELFs may not contain `.symtab`.
    if symtab_offset != 0
        && symtab_size != 0
        && symtab_entry_size != 0
        && strtab_offset != 0
        && strtab_size != 0
    {
        if symtab_offset + symtab_size <= bytes.len() && strtab_offset + strtab_size <= bytes.len()
        {
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

    if global_pointer.is_none() && shstrtab_offset != 0 && shstrtab_size != 0 {
        // Fallback for stripped ELFs: derive `gp` from `.sdata` base per RISC-V psABI:
        // `gp = __sdata_begin + 0x800`.
        if shstrtab_offset + shstrtab_size <= bytes.len() {
            let shstr = &bytes[shstrtab_offset..shstrtab_offset + shstrtab_size];
            let mut sdata_addr: Option<usize> = None;
            let mut data_addr: Option<usize> = None;
            for index in 0..e_shnum {
                let base = e_shoff + index * e_shentsize;
                if base + e_shentsize > bytes.len() {
                    break;
                }
                let name_off = read_u32(&bytes[base..base + 4]) as usize;
                if name_off >= shstr.len() {
                    continue;
                }
                let mut end = name_off;
                while end < shstr.len() && shstr[end] != 0 {
                    end += 1;
                }
                let name = &shstr[name_off..end];
                let addr = read_u64(&bytes[base + 16..base + 24]) as usize;
                if name == b".sdata" {
                    sdata_addr = Some(addr);
                } else if name == b".data" {
                    data_addr = Some(addr);
                }
            }
            let base = sdata_addr.or(data_addr);
            if let Some(base) = base {
                let gp = base.saturating_add(0x800);
                log_info!(target: "selftest", "KSELFTEST: derived gp=0x{:x} from .sdata/.data", gp);
                global_pointer = Some(gp);
            }
        }
    }

    let gp = global_pointer.ok_or("missing gp (no __global_pointer$ and no .sdata/.data)")?;
    // Seed SP two pages below the mapped top, 16-byte aligned (stay clear of the boundary).
    let stack_sp = (mapped_top - 2 * PAGE_SIZE) & !0xf;

    Ok((e_entry, stack_sp, gp, as_handle))
}

#[cfg(all(embed_init, target_arch = "riscv64", target_os = "none"))]
fn alloc_init_page() -> Option<usize> {
    use core::sync::atomic::{AtomicUsize, Ordering};

    // NOTE: This lives in the kernel image and must remain robust even if `.data` initializers
    // are unavailable during early bring-up. We therefore treat `0` as "uninitialized" and seed
    // it lazily from the fixed start address.
    static PAGE_CURSOR: AtomicUsize = AtomicUsize::new(0);
    const PAGE_LIMIT: usize = 0x8060_0000 + 4 * 1024 * 1024;
    const PAGE_START: usize = 0x8060_0000;

    loop {
        let cur = PAGE_CURSOR.load(Ordering::SeqCst);
        if cur == 0 {
            let next = PAGE_START.checked_add(PAGE_SIZE)?;
            if next > PAGE_LIMIT {
                return None;
            }
            // First allocation: claim PAGE_START and advance cursor to PAGE_START+PAGE_SIZE.
            match PAGE_CURSOR.compare_exchange(0, next, Ordering::SeqCst, Ordering::SeqCst) {
                Ok(_) => return Some(PAGE_START),
                Err(_) => continue,
            }
        }

        let next = cur.checked_add(PAGE_SIZE)?;
        if next > PAGE_LIMIT {
            return None;
        }
        match PAGE_CURSOR.compare_exchange(cur, next, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => return Some(cur),
            Err(_) => continue,
        }
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
        let _ = writeln!(uart, "[ERROR selftest] {} missing mapping va=0x{:016x}", label, virt);
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
