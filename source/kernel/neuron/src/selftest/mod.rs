// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! In-kernel selftest harness executed during deterministic boot.

extern crate alloc;

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
static mut CHILD_DATA_PAGE: [u8; PAGE_SIZE] = [0; PAGE_SIZE];

/// Borrowed references to kernel subsystems used by selftests.
pub struct Context<'a> {
    #[allow(dead_code)]
    pub hal: &'a VirtMachine,
    pub router: &'a mut Router,
    #[allow(dead_code)]
    pub address_spaces: &'a mut AddressSpaceManager,
    pub tasks: &'a mut TaskTable,
    pub scheduler: &'a mut Scheduler,
}

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
extern "C" fn child_new_as_entry() {
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
    let ptr = unsafe { core::ptr::addr_of_mut!(CHILD_DATA_PAGE) as usize };
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
    let mut table = SyscallTable::new();
    api::install_handlers(&mut table);
    let timer = ctx.hal.timer();
    let mut sys_ctx =
        api::Context::new(ctx.scheduler, ctx.tasks, ctx.router, ctx.address_spaces, timer);

    ensure_data_cap(ctx.tasks);

    let handle_raw = table
        .dispatch(SYSCALL_AS_CREATE, &mut sys_ctx, &Args::new([0; 6]))
        .expect("as_create syscall");
    uart::write_line("KSELFTEST: as create ok");

    const PROT_READ: usize = 1 << 0;
    const PROT_WRITE: usize = 1 << 1;
    const PROT_EXEC: usize = 1 << 2;
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
    let spawn_args = Args::new([entry, 0, handle_raw, 0, 0, 0]);
    let child_pid = table
        .dispatch(SYSCALL_SPAWN, &mut sys_ctx, &spawn_args)
        .expect("spawn syscall");
    let mut spins = 0;
    while CHILD_HEARTBEAT.load(Ordering::SeqCst) != 2 {
        let _ = table.dispatch(SYSCALL_YIELD, &mut sys_ctx, &Args::new([0; 6]));
        spins += 1;
        if spins > 64 {
            break;
        }
    }
    if CHILD_HEARTBEAT.load(Ordering::SeqCst) == 2 {
        uart::write_line("KSELFTEST: spawn newas ok");
    } else {
        uart::write_line("KSELFTEST: spawn newas failed");
    }

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
