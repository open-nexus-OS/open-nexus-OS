// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! In-kernel selftest harness executed during deterministic boot.

extern crate alloc;

use alloc::vec;

use crate::{
    cap::{CapError, CapTable, Capability, CapabilityKind, Rights},
    determinism,
    hal::{virt::VirtMachine, IrqCtl, Timer as _, Tlb, Uart},
    ipc::{self, header::MessageHeader, IpcError, Message, Router},
    mm::{self, MapError, PageFlags, PageTable, PAGE_SIZE},
    sched::{QosClass, Scheduler},
    uart,
};

pub mod assert;

/// Borrowed references to kernel subsystems used by selftests.
pub struct Context<'a> {
    pub hal: &'a VirtMachine,
    pub router: &'a mut Router,
    pub address_space: &'a mut PageTable,
    pub caps: &'a mut CapTable,
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
    test_trap_helpers();
    uart::write_line("SELFTEST: sched ok");
    uart::write_line("SELFTEST: end");
}

fn test_time(ctx: &Context<'_>) {
    use crate::st_assert;

    let timer = ctx.hal.timer();
    let start = timer.now();
    st_assert!(start > 0, "timer must advance past zero");
    let second = timer.now();
    st_assert!(second >= start, "timer monotonic");
    let deadline = second + determinism::fixed_tick_ns();
    timer.set_wakeup(deadline);

    let uart = ctx.hal.uart();
    let _: &dyn crate::hal::Uart = uart;
    uart.write_byte(b'\0');
    ctx.hal.tlb().flush_all();
    ctx.hal.irq().disable(0);
    ctx.hal.irq().enable(0);
}

fn test_ipc(ctx: &mut Context<'_>) {
    use crate::{st_assert, st_expect_eq, st_expect_err};

    let zero = Message::new(MessageHeader::new(0, 0, 1, 0, 0), vec![]);
    ctx.router
        .send(0, zero)
        .expect("bootstrap endpoint must exist");
    let zero_recv = ctx.router.recv(0).expect("message available");
    st_expect_eq!(zero_recv.payload.len(), 0usize);

    let payload = vec![0xA5; 4096];
    let max = Message::new(MessageHeader::new(0, 0, 2, 0, 4096), payload.clone());
    ctx.router.send(0, max).expect("max payload");
    let max_recv = ctx.router.recv(0).expect("max recv");
    st_expect_eq!(max_recv.payload.len(), 4096usize);
    st_assert!(
        max_recv.payload.iter().all(|&b| b == 0xA5),
        "payload integrity"
    );

    st_expect_err!(
        ctx.router
            .send(3, Message::new(MessageHeader::new(0, 3, 3, 0, 0), vec![])),
        IpcError::NoSuchEndpoint
    );

    st_expect_err!(ctx.router.recv(0), IpcError::QueueEmpty);

    #[cfg(feature = "failpoints")]
    {
        ipc::failpoints::deny_next_send();
        st_expect_err!(
            ctx.router
                .send(0, Message::new(MessageHeader::new(0, 0, 4, 0, 0), vec![])),
            IpcError::PermissionDenied
        );
    }
}

fn test_caps(ctx: &mut Context<'_>) {
    use crate::{st_assert, st_expect_eq, st_expect_err};

    let loopback = ctx.caps.get(0).expect("bootstrap cap");
    st_assert!(loopback.rights.contains(Rights::SEND));
    st_assert!(loopback.rights.contains(Rights::RECV));

    let derived = ctx.caps.derive(0, Rights::SEND).expect("derive send right");
    st_expect_eq!(derived.rights, Rights::SEND);

    st_expect_err!(ctx.caps.derive(0, Rights::MAP), CapError::PermissionDenied);
    st_expect_err!(ctx.caps.get(999), CapError::InvalidSlot);

    let new_cap = Capability {
        kind: CapabilityKind::Endpoint(2),
        rights: Rights::SEND | Rights::RECV,
    };
    ctx.caps.set(2, new_cap).expect("install new capability");
    let fetched = ctx.caps.get(2).expect("fetch newly installed cap");
    st_expect_eq!(fetched.kind, CapabilityKind::Endpoint(2));

    let irq_cap = Capability {
        kind: CapabilityKind::Irq(5),
        rights: Rights::MANAGE,
    };
    ctx.caps.set(3, irq_cap).expect("install irq cap");
    match ctx.caps.get(3).expect("fetch irq cap").kind {
        CapabilityKind::Irq(line) => st_expect_eq!(line, 5u32),
        other => panic!("unexpected capability: {other:?}"),
    }
}

fn test_map(ctx: &mut Context<'_>) {
    use crate::st_expect_err;

    let flags = PageFlags::VALID | PageFlags::READ | PageFlags::WRITE;
    ctx.address_space.map(0, 0, flags).expect("first mapping");
    let entry = ctx.address_space.lookup(0).expect("mapping present");
    crate::st_assert!(entry & flags.bits() != 0, "mapping retains flags");
    crate::st_expect_eq!(ctx.address_space.lookup(PAGE_SIZE), None);
    let _root = ctx.address_space.root_ppn();

    st_expect_err!(
        ctx.address_space.map(1, PAGE_SIZE, flags),
        MapError::Unaligned
    );

    #[cfg(feature = "failpoints")]
    {
        mm::failpoints::deny_next_map();
        st_expect_err!(
            ctx.address_space.map(PAGE_SIZE * 2, PAGE_SIZE * 3, flags),
            MapError::PermissionDenied
        );
    }

    st_expect_err!(
        ctx.address_space.map(0, PAGE_SIZE, flags),
        MapError::Overlap
    );
    st_expect_err!(
        ctx.address_space.map(PAGE_SIZE * 2048, 0, flags),
        MapError::OutOfRange
    );
}

fn test_sched(ctx: &mut Context<'_>) {
    use crate::{st_assert, st_expect_eq};

    st_expect_eq!(ctx.scheduler.timeslice_ns(), determinism::fixed_tick_ns());

    let mut sched = Scheduler::new();
    sched.enqueue(1, QosClass::Normal);
    sched.enqueue(2, QosClass::Normal);
    sched.enqueue(3, QosClass::PerfBurst);
    st_expect_eq!(sched.schedule_next(), Some(3));
    let second = sched.schedule_next().expect("second task");
    st_assert!(second == 1 || second == 2, "normal class after burst");
    let third = sched.schedule_next().expect("third task");
    st_assert!(third != second, "rotation ensures yield switch");
    sched.enqueue(4, QosClass::Normal);
    st_expect_eq!(sched.schedule_next(), Some(4));
    sched.yield_current();
    st_expect_eq!(sched.schedule_next(), Some(1));
}

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
    st_assert!(
        crate::trap::is_interrupt(interrupt_code),
        "interrupt bit detected"
    );
}
