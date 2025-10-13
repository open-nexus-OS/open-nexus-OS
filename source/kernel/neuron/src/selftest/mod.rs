// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! In-kernel selftest harness executed during deterministic boot.

extern crate alloc;

// Host build does not need Vec in minimal selftests; avoid unused import under deny(warnings)
#[cfg(all(target_arch = "riscv64", target_os = "none", feature = "selftest_full"))]
use alloc::vec;
#[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
use core::sync::atomic::{AtomicUsize, Ordering};

// Include the minimal child entry assembly like trap.S so the symbol is always linked.
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
core::arch::global_asm!(include_str!("child_stub.S"));
// Include stack-switch helper for running selftests on a private stack (feature-gated)
#[cfg(all(target_arch = "riscv64", target_os = "none", feature = "selftest_priv_stack"))]
core::arch::global_asm!(include_str!("stack_run.S"));

// Imports for OS build
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
use crate::{
    cap::CapabilityKind, hal::virt::VirtMachine, ipc::Router, mm::PageTable, sched::Scheduler,
    task::TaskTable, uart, BootstrapMsg,
};
#[cfg(all(target_arch = "riscv64", target_os = "none", feature = "selftest_full"))]
use crate::{
    cap::{CapError, Capability, Rights},
    determinism,
    hal::{IrqCtl, Timer as _, Tlb, Uart},
    ipc::{self, header::MessageHeader, IpcError, Message},
    sched::QosClass,
};

// Minimal imports for host build (keep only types required in signatures)
#[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
use crate::{
    hal::virt::VirtMachine, ipc::Router, mm::PageTable, sched::Scheduler, task::TaskTable, uart,
};

pub mod assert;
#[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
static CHILD_RUNS: AtomicUsize = AtomicUsize::new(0);

#[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
fn child_entry_stub() {
    uart::write_line("KSELFTEST: child running");
    CHILD_RUNS.fetch_add(1, Ordering::SeqCst);
    CHILD_RUNS.fetch_add(1, Ordering::SeqCst);
}

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
extern "C" {
    fn child_entry_asm();
}
#[cfg(all(target_arch = "riscv64", target_os = "none", feature = "selftest_priv_stack"))]
extern "C" {
    fn run_selftest_on_stack(
        func: extern "C" fn(*mut core::ffi::c_void),
        arg: *mut core::ffi::c_void,
        new_sp: *mut u8,
    );
}

/// Borrowed references to kernel subsystems used by selftests.
pub struct Context<'a> {
    #[allow(dead_code)]
    pub hal: &'a VirtMachine,
    pub router: &'a mut Router,
    #[allow(dead_code)]
    pub address_space: &'a mut PageTable,
    pub tasks: &'a mut TaskTable,
    pub scheduler: &'a mut Scheduler,
}

#[cfg(all(target_arch = "riscv64", target_os = "none", feature = "selftest_priv_stack"))]
#[repr(align(16))]
struct AlignedStack([u8; 8192]);
#[cfg(all(target_arch = "riscv64", target_os = "none", feature = "selftest_priv_stack"))]
static mut SELFTEST_STACK: AlignedStack = AlignedStack([0; 8192]);

#[cfg(all(target_arch = "riscv64", target_os = "none", feature = "selftest_priv_stack"))]
unsafe fn fill_canaries(ptr: *mut u8, len: usize) {
    let red = core::cmp::min(256, len / 8);
    let mut i = 0usize;
    while i < red {
        unsafe {
            core::ptr::write_volatile(ptr.add(i), 0xA5);
            core::ptr::write_volatile(ptr.add(len - 1 - i), 0x5A);
        }
        i += 1;
    }
}

#[cfg(all(target_arch = "riscv64", target_os = "none", feature = "selftest_priv_stack"))]
unsafe fn check_canaries(ptr: *const u8, len: usize) -> bool {
    let red = core::cmp::min(256, len / 8);
    let mut i = 0usize;
    while i < red {
        let (a, b) = unsafe {
            (core::ptr::read_volatile(ptr.add(i)), core::ptr::read_volatile(ptr.add(len - 1 - i)))
        };
        if a != 0xA5 || b != 0x5A {
            return false;
        }
        i += 1;
    }
    true
}

#[cfg(all(target_arch = "riscv64", target_os = "none", feature = "selftest_priv_stack"))]
extern "C" fn entry_shim(arg: *mut core::ffi::c_void) {
    // SAFETY: caller passes a valid pointer to Context
    let ctx = unsafe { &mut *(arg as *mut Context<'_>) };
    entry(ctx);
}

/// Run the selftests on a private, canaried stack with timer IRQs masked.
#[cfg(all(target_arch = "riscv64", target_os = "none", feature = "selftest_priv_stack"))]
pub fn entry_on_private_stack(ctx: &mut Context<'_>) {
    // Prepare stack and canaries
    const STK_LEN: usize = 8192;
    const GUARD: usize = 256; // leave a redzone at the top to avoid clobbering canaries
    let stk_ptr = unsafe {
        let base = core::ptr::addr_of_mut!(SELFTEST_STACK) as *mut AlignedStack;
        core::ptr::addr_of_mut!((*base).0) as *mut u8
    };
    let sp_top = unsafe { stk_ptr.add(STK_LEN) };
    unsafe { fill_canaries(stk_ptr, STK_LEN) };
    // Mask S-timer interrupts during test to avoid reentrancy
    unsafe {
        use riscv::register::{sie, sstatus};
        sstatus::clear_sie();
        sie::clear_stimer();
    }
    // Switch to private stack and run
    let arg_ptr = ctx as *mut _ as *mut core::ffi::c_void;
    let guarded_sp = unsafe { sp_top.offset(-(GUARD as isize)) };
    unsafe { run_selftest_on_stack(entry_shim, arg_ptr, guarded_sp as *mut u8) };
    // Re-enable interrupts after selftest
    unsafe {
        use riscv::register::{sie, sstatus};
        sie::set_stimer();
        sstatus::set_sie();
    }
    if !unsafe { check_canaries(stk_ptr as *const u8, STK_LEN) } {
        uart::write_line("SELFTEST: stack canary CORRUPT");
    } else {
        // silent on success in OS stage
    }
}

/// Entrypoint invoked by the kernel after core initialisation completes.
#[cfg(all(target_arch = "riscv64", target_os = "none", feature = "selftest_full"))]
pub fn entry(ctx: &mut Context<'_>) {
    uart::write_line("SELFTEST: begin");
    test_time(ctx);
    uart::write_line("SELFTEST: time ok");
    test_ipc(ctx);
    uart::write_line("SELFTEST: ipc ok");
    test_caps(ctx);
    uart::write_line("SELFTEST: caps ok");
    test_sched(ctx);
    uart::write_line("SELFTEST: sched ok");
    test_spawn(ctx);
    uart::write_line("SELFTEST: end");
}

/// Minimal OS entry when full suite is disabled: only spawn test.
#[cfg(all(target_arch = "riscv64", target_os = "none", not(feature = "selftest_full")))]
pub fn entry(ctx: &mut Context<'_>) {
    uart::write_line("SELFTEST: begin");
    #[cfg(feature = "selftest_time")]
    {
        test_time(ctx);
        uart::write_line("SELFTEST: time ok");
    }
    #[cfg(feature = "selftest_ipc")]
    {
        test_ipc(ctx);
        uart::write_line("SELFTEST: ipc ok");
    }
    test_spawn(ctx);
    uart::write_line("SELFTEST: end");
}

/// Entrypoint (host/full) invoked by the kernel after core initialisation completes.
#[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
pub fn entry(ctx: &mut Context<'_>) {
    uart::write_line("SELFTEST: begin");
    test_time(ctx);
    uart::write_line("SELFTEST: time ok");
    test_ipc(ctx);
    uart::write_line("SELFTEST: ipc ok");
    test_caps(ctx);
    uart::write_line("SELFTEST: caps ok");
    test_map(ctx);
    uart::write_line("SELFTEST: map ok");
    test_sched(ctx);
    uart::write_line("SELFTEST: sched ok");
    test_spawn(ctx);
    uart::write_line("SELFTEST: end");
}

#[cfg(all(
    target_arch = "riscv64",
    target_os = "none",
    any(feature = "selftest_full", feature = "selftest_time")
))]
fn test_time(ctx: &Context<'_>) {
    use crate::st_assert;
    use crate::{
        determinism,
        hal::{IrqCtl, Timer as _, Tlb, Uart},
    };

    uart::write_line("SELFTEST: time step0: acquire timer handle");
    let timer = ctx.hal.timer();
    let start = timer.now();
    uart::write_line("SELFTEST: time step1: read start time");
    st_assert!(start > 0, "timer must advance past zero");
    let second = timer.now();
    uart::write_line("SELFTEST: time step2: verify monotonic now() >= start");
    st_assert!(second >= start, "timer monotonic");
    let deadline = second + determinism::fixed_tick_ns();
    timer.set_wakeup(deadline);
    uart::write_line("SELFTEST: time step3: program wakeup via SBI set_timer");

    let uart = ctx.hal.uart();
    let _: &dyn crate::hal::Uart = uart;
    uart.write_byte(b'\0');
    uart::write_line("SELFTEST: time step4: uart byte and flush TLB");
    ctx.hal.tlb().flush_all();
    uart::write_line("SELFTEST: time step5: disable/enable IRQ line 0");
    ctx.hal.irq().disable(0);
    ctx.hal.irq().enable(0);
    uart::write_line("SELFTEST: time step6: time test complete");
}

#[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
fn test_time(ctx: &Context<'_>) {
    use crate::st_assert;
    use crate::{determinism, hal::Timer as _, hal::Tlb};

    uart::write_line("SELFTEST: time step0: acquire timer handle (host)");
    let timer = ctx.hal.timer();
    let start = timer.now();
    let second = timer.now();
    st_assert!(second >= start, "timer monotonic on host");
    let _ = ctx.hal.uart();
    ctx.hal.tlb().flush_all();
    let _tick = determinism::fixed_tick_ns();
    let _ = ctx.hal.irq();
}

#[cfg(all(
    target_arch = "riscv64",
    target_os = "none",
    any(feature = "selftest_full", feature = "selftest_ipc")
))]
fn test_ipc(ctx: &mut Context<'_>) {
    use crate::ipc::{self, header::MessageHeader, IpcError, Message};
    use crate::{st_assert, st_expect_eq, st_expect_err};
    use alloc::vec;

    uart::write_line("SELFTEST: ipc step0: send/recv zero-length on endpoint 0");
    let zero = Message::new(MessageHeader::new(0, 0, 1, 0, 0), vec![]);
    uart::write_line("SELFTEST: ipc step0-pre: msg created");
    uart::write_line("SELFTEST: ipc step0-pre: calling send(ep0)");
    ctx.router.send(0, zero).expect("bootstrap endpoint must exist");
    uart::write_line("SELFTEST: ipc step0a: sent to ep0");
    uart::write_line("SELFTEST: ipc step0b: recv begin");
    let zero_recv = ctx.router.recv(0).expect("message available");
    uart::write_line("SELFTEST: ipc step0c: recv ok");
    st_expect_eq!(zero_recv.payload.len(), 0usize);

    uart::write_line("SELFTEST: ipc step1: send/recv max payload");
    let payload = vec![0xA5; 4096];
    let max = Message::new(MessageHeader::new(0, 0, 2, 0, 4096), payload.clone());
    ctx.router.send(0, max).expect("max payload");
    uart::write_line("SELFTEST: ipc step1a: sent max to ep0");
    let max_recv = ctx.router.recv(0).expect("max recv");
    uart::write_line("SELFTEST: ipc step1b: recv max ok");
    st_expect_eq!(max_recv.payload.len(), 4096usize);
    st_assert!(max_recv.payload.iter().all(|&b| b == 0xA5), "payload integrity");

    uart::write_line("SELFTEST: ipc step2: expect NoSuchEndpoint on id 4");
    st_expect_err!(
        ctx.router.send(4, Message::new(MessageHeader::new(0, 4, 3, 0, 0), vec![])),
        IpcError::NoSuchEndpoint
    );

    uart::write_line("SELFTEST: ipc step3: expect QueueEmpty on recv(0)");
    st_expect_err!(ctx.router.recv(0), IpcError::QueueEmpty);

    #[cfg(feature = "failpoints")]
    {
        uart::write_line("SELFTEST: ipc step4: expect PermissionDenied via failpoint");
        ipc::failpoints::deny_next_send();
        st_expect_err!(
            ctx.router.send(0, Message::new(MessageHeader::new(0, 0, 4, 0, 0), vec![])),
            IpcError::PermissionDenied
        );
    }
}

#[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
fn test_ipc(ctx: &mut Context<'_>) {
    use crate::ipc::{header::MessageHeader, IpcError, Message};
    use crate::{st_expect_eq, st_expect_err};
    use alloc::vec;

    uart::write_line("SELFTEST: ipc step0: send/recv zero-length on endpoint 0 (host)");
    let zero = Message::new(MessageHeader::new(0, 0, 1, 0, 0), vec![]);
    ctx.router.send(0, zero).expect("bootstrap endpoint must exist");
    let zero_recv = ctx.router.recv(0).expect("message available");
    st_expect_eq!(zero_recv.payload.len(), 0usize);
    st_expect_err!(ctx.router.recv(0), IpcError::QueueEmpty);
}

#[cfg(all(
    target_arch = "riscv64",
    target_os = "none",
    any(feature = "selftest_full", feature = "selftest_caps")
))]
fn test_caps(ctx: &mut Context<'_>) {
    use crate::{st_assert, st_expect_eq, st_expect_err};

    uart::write_line("SELFTEST: caps step0: bootstrap cap rights");
    let caps = ctx.tasks.current_caps_mut();
    let loopback = caps.get(0).expect("bootstrap cap");
    st_assert!(loopback.rights.contains(Rights::SEND));
    st_assert!(loopback.rights.contains(Rights::RECV));

    uart::write_line("SELFTEST: caps step1: derive SEND right");
    let derived = caps.derive(0, Rights::SEND).expect("derive send right");
    st_expect_eq!(derived.rights, Rights::SEND);

    uart::write_line("SELFTEST: caps step2: expect PermissionDenied for MAP derivation");
    st_expect_err!(caps.derive(0, Rights::MAP), CapError::PermissionDenied);
    uart::write_line("SELFTEST: caps step3: expect InvalidSlot for get(999)");
    st_expect_err!(caps.get(999), CapError::InvalidSlot);

    uart::write_line("SELFTEST: caps step4: install and verify endpoint cap in slot 2");
    let new_cap =
        Capability { kind: CapabilityKind::Endpoint(2), rights: Rights::SEND | Rights::RECV };
    caps.set(2, new_cap).expect("install new capability");
    let fetched = caps.get(2).expect("fetch newly installed cap");
    st_expect_eq!(fetched.kind, CapabilityKind::Endpoint(2));

    uart::write_line("SELFTEST: caps step5: install and read back IRQ cap");
    let irq_cap = Capability { kind: CapabilityKind::Irq(5), rights: Rights::MANAGE };
    caps.set(3, irq_cap).expect("install irq cap");
    match caps.get(3).expect("fetch irq cap").kind {
        CapabilityKind::Irq(line) => st_expect_eq!(line, 5u32),
        other => panic!("unexpected capability: {other:?}"),
    }
}

#[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
fn test_caps(ctx: &mut Context<'_>) {
    use crate::cap::{CapError, Capability, CapabilityKind, Rights};
    use crate::{st_assert, st_expect_eq, st_expect_err};

    uart::write_line("SELFTEST: caps step0: bootstrap cap rights (host)");
    let caps = ctx.tasks.current_caps_mut();
    let loopback = caps.get(0).expect("bootstrap cap");
    st_assert!(loopback.rights.contains(Rights::SEND));
    st_assert!(loopback.rights.contains(Rights::RECV));

    let derived = caps.derive(0, Rights::SEND).expect("derive send right");
    st_expect_eq!(derived.rights, Rights::SEND);

    st_expect_err!(caps.derive(0, Rights::MAP), CapError::PermissionDenied);
    st_expect_err!(caps.get(999), CapError::InvalidSlot);

    let new_cap =
        Capability { kind: CapabilityKind::Endpoint(2), rights: Rights::SEND | Rights::RECV };
    caps.set(2, new_cap).expect("install new capability");
    let fetched = caps.get(2).expect("fetch newly installed cap");
    st_expect_eq!(fetched.kind, CapabilityKind::Endpoint(2));
}

#[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
fn test_map(_ctx: &mut Context<'_>) {
    use crate::mm::{MapError, PageFlags, PageTable, PAGE_SIZE};
    use crate::st_expect_err;

    uart::write_line("SELFTEST: map begin");
    // Perform mapping tests on a private page table to avoid mutating the live kernel AS.
    let mut pt = PageTable::new();
    uart::write_line("SELFTEST: map step0: first mapping and lookup");
    let flags = PageFlags::VALID | PageFlags::READ | PageFlags::WRITE;
    pt.map(0, 0, flags).expect("first mapping");
    let entry = pt.lookup(0).expect("mapping present");
    crate::st_assert!(entry & flags.bits() != 0, "mapping retains flags");
    crate::st_expect_eq!(pt.lookup(PAGE_SIZE), None);
    let _root = pt.root_ppn();

    uart::write_line("SELFTEST: map step1: expect Unaligned");
    st_expect_err!(pt.map(1, PAGE_SIZE, flags), MapError::Unaligned);

    #[cfg(feature = "failpoints")]
    {
        uart::write_line("SELFTEST: map step2: expect PermissionDenied via failpoint");
        crate::mm::failpoints::deny_next_map();
        st_expect_err!(pt.map(PAGE_SIZE * 2, PAGE_SIZE * 3, flags), MapError::PermissionDenied);
    }

    uart::write_line("SELFTEST: map step3: expect Overlap and OutOfRange");
    uart::write_line("SELFTEST: map step3a: assert Overlap");
    st_expect_err!(pt.map(0, PAGE_SIZE, flags), MapError::Overlap);
    uart::write_line("SELFTEST: map step3a ok");
    // Removed yield to avoid reliance on timer interrupt delivery during CI
    uart::write_line("SELFTEST: map step3b: assert OutOfRange");
    st_expect_err!(pt.map(PAGE_SIZE * 2048, 0, flags), MapError::OutOfRange);
    uart::write_line("SELFTEST: map step3b ok");
    // Removed yield to avoid reliance on timer interrupt delivery during CI
    uart::write_line("SELFTEST: map ok");
}

#[cfg(all(
    target_arch = "riscv64",
    target_os = "none",
    any(feature = "selftest_full", feature = "selftest_sched")
))]
fn test_sched(ctx: &mut Context<'_>) {
    use crate::{st_assert, st_expect_eq};

    uart::write_line("SELFTEST: sched step0: verify timeslice");
    st_expect_eq!(ctx.scheduler.timeslice_ns(), determinism::fixed_tick_ns());
    uart::write_line("SELFTEST: sched step0 ok");

    uart::write_line("SELFTEST: sched step1: enqueue and schedule-order checks");
    let mut sched = Scheduler::new();
    sched.enqueue(1, QosClass::Normal);
    sched.enqueue(2, QosClass::Normal);
    sched.enqueue(3, QosClass::PerfBurst);
    st_expect_eq!(sched.schedule_next(), Some(3));
    uart::write_line("SELFTEST: sched step1a ok (burst first)");
    let second = sched.schedule_next().expect("second task");
    st_assert!(second == 1 || second == 2, "normal class after burst");
    uart::write_line("SELFTEST: sched step1b ok (normal after burst)");
    let third = sched.schedule_next().expect("third task");
    st_assert!(third != second, "rotation ensures yield switch");
    uart::write_line("SELFTEST: sched step1c ok (rotation)");
    sched.enqueue(4, QosClass::Normal);
    st_expect_eq!(sched.schedule_next(), Some(4));
    uart::write_line("SELFTEST: sched step1d ok (newly enqueued first)");
    sched.yield_current();
    st_expect_eq!(sched.schedule_next(), Some(4));
    uart::write_line("SELFTEST: sched step1e ok (yield -> 4)");
    uart::write_line("SELFTEST: sched ok");
}

#[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
fn test_sched(ctx: &mut Context<'_>) {
    use crate::determinism;
    use crate::sched::{QosClass, Scheduler};
    use crate::{st_assert, st_expect_eq};

    uart::write_line("SELFTEST: sched step0: verify timeslice (host)");
    st_expect_eq!(ctx.scheduler.timeslice_ns(), determinism::fixed_tick_ns());
    let mut sched = Scheduler::new();
    sched.enqueue(1, QosClass::Normal);
    sched.enqueue(2, QosClass::Normal);
    sched.enqueue(3, QosClass::PerfBurst);
    let first = sched.schedule_next().expect("first");
    st_assert!(first == 3 || first == 1 || first == 2, "some task scheduled");
}

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
pub(crate) fn test_spawn(ctx: &mut Context<'_>) {
    uart::write_line("SELFTEST: spawn begin");
    let entry_pc = unsafe { core::mem::transmute::<_, usize>(child_entry_asm as *const ()) } as u64;
    let stack_top = 0u64;

    let parent = ctx.tasks.current_pid();
    let child = match ctx.tasks.spawn(parent, entry_pc, stack_top, 0, 0, ctx.scheduler, ctx.router)
    {
        Ok(pid) => pid,
        Err(_) => {
            uart::write_line("SELFTEST: spawn FAIL: syscall");
            loop {
                crate::arch::riscv::wait_for_interrupt();
            }
        }
    };
    if child == parent {
        uart::write_line("SELFTEST: spawn FAIL: child==parent");
        loop {
            crate::arch::riscv::wait_for_interrupt();
        }
    }

    match ctx.router.recv(0) {
        Ok(msg) => {
            if msg.payload.len() != core::mem::size_of::<BootstrapMsg>() {
                uart::write_line("SELFTEST: spawn FAIL: bad bootstrap len");
                loop {
                    crate::arch::riscv::wait_for_interrupt();
                }
            }
        }
        Err(_) => {
            uart::write_line("SELFTEST: spawn FAIL: no bootstrap msg");
            loop {
                crate::arch::riscv::wait_for_interrupt();
            }
        }
    }

    let ok_caps = ctx
        .tasks
        .caps_of(child)
        .and_then(|caps| caps.get(0).ok())
        .map(|cap| matches!(cap.kind, CapabilityKind::Endpoint(0)))
        .unwrap_or(false);
    if !ok_caps {
        uart::write_line("SELFTEST: spawn FAIL: child cap[0] not ep0");
        loop {
            crate::arch::riscv::wait_for_interrupt();
        }
    }

    // Call the child entry stub once to ensure it is executable and returns.
    unsafe {
        let func: fn() = core::mem::transmute(entry_pc as usize);
        func();
    }
    uart::write_line("KSELFTEST: spawn ok");
}

#[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
fn test_spawn(ctx: &mut Context<'_>) {
    use crate::cap::CapabilityKind;
    use crate::{st_assert, st_expect_eq};

    uart::write_line("SELFTEST: spawn begin");
    CHILD_RUNS.store(0, Ordering::SeqCst);
    let entry = child_entry_stub as usize as u64;
    let stack_top = 0u64;
    let parent = ctx.tasks.current_pid();
    let child = ctx
        .tasks
        .spawn(parent, entry, stack_top, 0, 0, ctx.scheduler, ctx.router)
        .expect("spawn child task");
    st_assert!(child != parent, "child pid differs from parent");
    let bootstrap = ctx.router.recv(0).expect("bootstrap message enqueued");
    st_expect_eq!(bootstrap.payload.len(), 32usize);
    let cap = ctx
        .tasks
        .caps_of(child)
        .expect("child task present")
        .get(0)
        .expect("bootstrap capability copied");
    st_expect_eq!(cap.kind, CapabilityKind::Endpoint(0));
    st_expect_eq!(CHILD_RUNS.load(Ordering::SeqCst), 0usize);
    unsafe {
        let func: fn() = core::mem::transmute(entry as usize);
        func();
    }
    st_expect_eq!(CHILD_RUNS.load(Ordering::SeqCst), 2usize);
    uart::write_line("KSELFTEST: spawn ok");
}

#[allow(dead_code)]
fn test_trap_helpers() {
    use crate::{st_assert, st_expect_eq};

    let mut frame = crate::trap::TrapFrame::default();
    frame.sepc = 0x2000;
    frame.scause = 9;
    frame.stval = 0x4000;
    crate::trap::record(&frame);
    let recorded = crate::trap::last_trap().expect("trap recorded");
    st_expect_eq!(recorded.sepc, frame.sepc);
    st_expect_eq!(recorded.stval, frame.stval);
    let description = crate::trap::describe_cause(recorded.scause);
    st_assert!(!description.is_empty(), "trap description available");
    let mut buffer = alloc::string::String::new();
    crate::trap::fmt_trap(&recorded, &mut buffer).expect("format trap");
    st_assert!(!buffer.is_empty(), "trap formatting produced output");
    let interrupt_code = (usize::MAX - (usize::MAX >> 1)) | 1;
    st_assert!(crate::trap::is_interrupt(interrupt_code), "interrupt bit detected");
}
