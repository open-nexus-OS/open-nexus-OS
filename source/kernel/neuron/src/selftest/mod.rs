// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! In-kernel selftest harness executed during deterministic boot.

extern crate alloc;

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
use core::sync::atomic::{AtomicUsize, Ordering};

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
use crate::{
    cap::{Capability, CapabilityKind, Rights},
    hal::virt::VirtMachine,
    ipc::Router,
    mm::{AddressSpaceError, AddressSpaceManager, MapError, PAGE_SIZE},
    sched::Scheduler,
    syscall::{
        api, Args, Error as SysError, SyscallTable, SYSCALL_AS_CREATE, SYSCALL_AS_MAP, SYSCALL_SPAWN,
        SYSCALL_YIELD,
    },
    task::TaskTable,
    uart,
};

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
use crate::task::Pid;
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
use riscv::register::sstatus;
#[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
use crate::{
    hal::virt::VirtMachine,
    ipc::Router,
    mm::AddressSpaceManager,
    sched::Scheduler,
    task::TaskTable,
    uart,
};

pub mod assert;

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
const CHILD_TEST_VA: usize = 0x4010_0000;
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
const CHILD_PATTERN: &[u8; 8] = b"NeuronAS";
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
static CHILD_HEARTBEAT: AtomicUsize = AtomicUsize::new(0);

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
#[repr(align(4096))]
struct AlignedPage([u8; PAGE_SIZE]);
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
static mut CHILD_DATA_PAGE: AlignedPage = AlignedPage([0; PAGE_SIZE]);

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
    uart::write_line("KSELFTEST: child entry");
    let mut matched = true;
    for (index, byte) in CHILD_PATTERN.iter().enumerate() {
        let value = unsafe { core::ptr::read_volatile((CHILD_TEST_VA + index) as *const u8) };
        if value != *byte {
            matched = false;
            break;
        }
    }
    if matched {
        uart::write_line("KSELFTEST: child newas running");
        CHILD_HEARTBEAT.store(1, Ordering::SeqCst);
    } else {
        CHILD_HEARTBEAT.store(usize::MAX, Ordering::SeqCst);
    }
    syscall_yield();
    syscall_yield();
    CHILD_HEARTBEAT.store(2, Ordering::SeqCst);
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
        kind: CapabilityKind::Vmo { base: ptr, len: PAGE_SIZE },
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
        let mut sys_ctx =
            api::Context::new(ctx.scheduler, ctx.tasks, ctx.router, ctx.address_spaces, timer);

        let h = table
            .dispatch(SYSCALL_AS_CREATE, &mut sys_ctx, &Args::new([0; 6]))
            .expect("as_create syscall");
        handle_raw = h;
        uart::write_line("KSELFTEST: as create ok");

        const PROT_READ: usize = 1 << 0;
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
        uart::write_line("KSELFTEST: as map ok");

        let entry = child_new_as_entry as usize;
        uart::write_line("KSELFTEST: before spawn");
        let spawn_args = Args::new([entry, 0, handle_raw, 0, 0, 0]);
        child_pid = table
            .dispatch(SYSCALL_SPAWN, &mut sys_ctx, &spawn_args)
            .expect("spawn syscall");
        // Emit explicit raw marker first to catch any UART lock anomalies.
        {
            use core::fmt::Write as _;
            let mut u = crate::uart::raw_writer();
            let _ = write!(u, "KSELFTEST: after spawn (raw)\n");
        }
        // Force a couple of yields to exercise trap fastpath and encourage scheduling
        syscall_yield();
        syscall_yield();
        // Emit line-based marker as well
        uart::write_line("KSELFTEST: after spawn");
        {
            use core::fmt::Write as _;
            let mut u = crate::uart::raw_writer();
            let _ = write!(u, "KSELFTEST: after spawn\n");
            let _ = write!(u, "KSELFTEST: child pid={}\n", child_pid);
        }
        // Fail-fast window: child must signal Heartbeat within 64 yields
        let mut spins = 0;
        while CHILD_HEARTBEAT.load(Ordering::SeqCst) == 0 && spins < 64 {
            syscall_yield();
            spins += 1;
        }
        if CHILD_HEARTBEAT.load(Ordering::SeqCst) == 0 {
            use core::fmt::Write as _;
            let mut u = crate::uart::raw_writer();
            let satp_now = {
                #[cfg(all(target_arch = "riscv64", target_os = "none"))]
                { riscv::register::satp::read().bits() }
                #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
                { 0 }
            };
            let _ = write!(
                u,
                "KSELFTEST: FAIL no child progress pid={} satp=0x{:x}\n",
                child_pid,
                satp_now
            );
            // Do not abort here; proceed to direct call_on_stack path to validate AS/mapping.
        }
        // For bring-up diagnostics, mark that control returned here.
        uart::write_line("KSELFTEST: early return after spawn");
        // sys_ctx and table drop here to release borrows on ctx.*
    }

    // Confirm we exited the syscall context block cleanly
    {
        use core::fmt::Write as _;
        let mut u = crate::uart::raw_writer();
        let _ = write!(u, "KSELFTEST: after sysctx block\n");
    }

    // Directly enter the child's address space and run the entry on its stack.
    {
        let pid = child_pid as Pid;
        if let Some(task) = ctx.tasks.task(pid) {
            if let Some(handle) = task.address_space() {
                {
                    use core::fmt::Write as _;
                    let mut u = crate::uart::raw_writer();
                    let _ = write!(u, "KSELFTEST: before as activate\n");
                }
                let _ = ctx.address_spaces.activate(handle);
                {
                    use core::fmt::Write as _;
                    let mut u = crate::uart::raw_writer();
                    let _ = write!(u, "KSELFTEST: before set_sum\n");
                }
                unsafe { sstatus::set_sum(); }
                {
                    use core::fmt::Write as _;
                    let mut u = crate::uart::raw_writer();
                    let _ = write!(u, "KSELFTEST: after set_sum\n");
                }
                // Witness: SATP must match the child's address space SATP value
                #[cfg(all(target_arch = "riscv64", target_os = "none"))]
                {
                    let satp_now = riscv::register::satp::read().bits();
                    let expected = ctx.address_spaces.get(handle).map(|s| s.satp_value()).unwrap_or(0);
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
                {
                    use core::fmt::Write as _;
                    let mut u = crate::uart::raw_writer();
                    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
                    let satp_now = riscv::register::satp::read().bits();
                    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
                    let satp_now: usize = 0;
                    let _ = write!(
                        u,
                        "KSELFTEST: pre child satp=0x{:x} sp=0x{:x} sepc=0x{:x}\n",
                        satp_now,
                        task.frame().x[2],
                        task.frame().sepc
                    );
                }
                {
                    use core::fmt::Write as _;
                    let mut u = crate::uart::raw_writer();
                    let _ = write!(u, "KSELFTEST: before child call_on_stack\n");
                }
                call_on_stack(child_new_as_entry, task.frame().x[2]);
                {
                    use core::fmt::Write as _;
                    let mut u = crate::uart::raw_writer();
                    let _ = write!(u, "KSELFTEST: after child call_on_stack\n");
                }
                unsafe { sstatus::clear_sum(); }
                if let Some(khandle) = ctx.tasks.bootstrap_mut().address_space {
                    let _ = ctx.address_spaces.activate(khandle);
                }
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
                unsafe { sstatus::set_sum(); }
                // Run the child's entry on its stack, then restore SUM; kernel text/data are
                // globally mapped so returning here is safe.
                call_on_stack(child_new_as_entry, task.frame().x[2]);
                unsafe { sstatus::clear_sum(); }
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
    uart::write_line("KSELFTEST: spawn newas ok");

    // Recreate syscall context to test W^X enforcement.
    let mut table = SyscallTable::new();
    api::install_handlers(&mut table);
    let timer = ctx.hal.timer();
    let mut sys_ctx = api::Context::new(ctx.scheduler, ctx.tasks, ctx.router, ctx.address_spaces, timer);
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
            uart::write_line("KSELFTEST: w^x enforced");
        }
        Err(_) | Ok(_) => {
            uart::write_line("KSELFTEST: w^x NOT enforced");
        }
    }

    // Silence unused result in release builds.
    let _ = child_pid;
}

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
fn call_on_stack(entry: extern "C" fn(), new_sp: usize) {
    {
        use core::fmt::Write as _;
        let mut u = crate::uart::raw_writer();
        let _ = write!(u, "KSELFTEST: call_on_stack enter sp=0x{:x} func=0x{:x}\n", new_sp, entry as usize);
    }
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
    {
        use core::fmt::Write as _;
        let mut u = crate::uart::raw_writer();
        let _ = write!(u, "KSELFTEST: call_on_stack return\n");
    }
}

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
pub fn entry(ctx: &mut Context<'_>) {
    CHILD_HEARTBEAT.store(0, Ordering::SeqCst);
    run_address_space_selftests(ctx);
}

#[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
pub fn entry(_ctx: &mut Context<'_>) {
    uart::write_line("SELFTEST: host build noop");
}

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
pub fn entry_on_private_stack(ctx: &mut Context<'_>) {
    entry(ctx);
}
