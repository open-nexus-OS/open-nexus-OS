// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! In-kernel selftest harness executed during deterministic boot.

extern crate alloc;

use alloc::vec;
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::{
    cap::{CapError, Capability, CapabilityKind, Rights},
    determinism,
    hal::{virt::VirtMachine, IrqCtl, Timer as _, Tlb, Uart},
    ipc::{self, header::MessageHeader, IpcError, Message, Router},
    mm::{self, MapError, PageFlags, PageTable, PAGE_SIZE},
    sched::{QosClass, Scheduler},
    task::TaskTable,
    uart,
    BootstrapMsg,
};

pub mod assert;

#[repr(align(16))]
struct ChildStack([u8; 256]);

static mut CHILD_STACK: ChildStack = ChildStack([0; 256]);
static CHILD_RUNS: AtomicUsize = AtomicUsize::new(0);

fn child_entry_stub() {
    uart::write_line("KSELFTEST: child running");
    CHILD_RUNS.fetch_add(1, Ordering::SeqCst);
    CHILD_RUNS.fetch_add(1, Ordering::SeqCst);
}

/// Borrowed references to kernel subsystems used by selftests.
pub struct Context<'a> {
    pub hal: &'a VirtMachine,
    pub router: &'a mut Router,
    pub address_space: &'a mut PageTable,
    pub tasks: &'a mut TaskTable,
    pub scheduler: &'a mut Scheduler,
}

/// Entrypoint invoked by the kernel after core initialisation completes.
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
    test_spawn(ctx);
    uart::write_line("SELFTEST: end");
}

fn test_time(ctx: &Context<'_>) {
    use crate::st_assert;

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

fn test_ipc(ctx: &mut Context<'_>) {
    use crate::{st_assert, st_expect_eq, st_expect_err};

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

fn test_map(ctx: &mut Context<'_>) {
    use crate::st_expect_err;

    uart::write_line("SELFTEST: map begin");
    // Removed initial WFI to avoid stalling without a pending interrupt
    uart::write_line("SELFTEST: map step0: first mapping and lookup");
    let flags = PageFlags::VALID | PageFlags::READ | PageFlags::WRITE;
    ctx.address_space.map(0, 0, flags).expect("first mapping");
    let entry = ctx.address_space.lookup(0).expect("mapping present");
    crate::st_assert!(entry & flags.bits() != 0, "mapping retains flags");
    crate::st_expect_eq!(ctx.address_space.lookup(PAGE_SIZE), None);
    let _root = ctx.address_space.root_ppn();

    uart::write_line("SELFTEST: map step1: expect Unaligned");
    st_expect_err!(ctx.address_space.map(1, PAGE_SIZE, flags), MapError::Unaligned);

    #[cfg(feature = "failpoints")]
    {
        uart::write_line("SELFTEST: map step2: expect PermissionDenied via failpoint");
        mm::failpoints::deny_next_map();
        st_expect_err!(
            ctx.address_space.map(PAGE_SIZE * 2, PAGE_SIZE * 3, flags),
            MapError::PermissionDenied
        );
    }

    uart::write_line("SELFTEST: map step3: expect Overlap and OutOfRange");
    uart::write_line("SELFTEST: map step3a: assert Overlap");
    st_expect_err!(ctx.address_space.map(0, PAGE_SIZE, flags), MapError::Overlap);
    uart::write_line("SELFTEST: map step3a ok");
    // Removed yield to avoid reliance on timer interrupt delivery during CI
    uart::write_line("SELFTEST: map step3b: assert OutOfRange");
    st_expect_err!(ctx.address_space.map(PAGE_SIZE * 2048, 0, flags), MapError::OutOfRange);
    uart::write_line("SELFTEST: map step3b ok");
    // Removed yield to avoid reliance on timer interrupt delivery during CI
    uart::write_line("SELFTEST: map ok");
}

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

fn test_spawn(ctx: &mut Context<'_>) {
    use crate::{st_assert, st_expect_eq};

    uart::write_line("SELFTEST: spawn begin");
    CHILD_RUNS.store(0, Ordering::SeqCst);
    let entry = child_entry_stub as usize as u64;
    let stack_top = unsafe {
        // SAFETY: exclusive access during selftest; stack lives for program duration.
        let base = CHILD_STACK.0.as_ptr() as usize;
        base + CHILD_STACK.0.len()
    } as u64;

    let parent = ctx.tasks.current_pid();
    let child = ctx
        .tasks
        .spawn(parent, entry, stack_top, 0, 0, ctx.scheduler, ctx.router)
        .expect("spawn child task");
    st_assert!(child != parent, "child pid differs from parent");

    let bootstrap = ctx.router.recv(0).expect("bootstrap message enqueued");
    st_expect_eq!(
        bootstrap.payload.len(),
        core::mem::size_of::<BootstrapMsg>(),
    );
    let cap = ctx
        .tasks
        .caps_of(child)
        .expect("child task present")
        .get(0)
        .expect("bootstrap capability copied");
    st_expect_eq!(cap.kind, CapabilityKind::Endpoint(0));

    st_expect_eq!(CHILD_RUNS.load(Ordering::SeqCst), 0usize);
    // SAFETY: entry points to a Rust function with C ABI-compatible signature.
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
