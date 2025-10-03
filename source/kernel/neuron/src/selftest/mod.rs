// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! In-kernel selftest harness executed during deterministic boot.

extern crate alloc;

use alloc::vec;

use crate::{
    cap::{CapError, CapTable, Capability, CapabilityKind, Rights},
    determinism,
    hal::virt::VirtMachine,
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
}

fn test_ipc(ctx: &mut Context<'_>) {
    use crate::{st_assert, st_expect_eq, st_expect_err};

    let zero = Message::new(MessageHeader::new(0, 0, 1, 0, 0), vec![]);
    ctx.router.send(0, zero).expect("bootstrap endpoint must exist");
    let zero_recv = ctx.router.recv(0).expect("message available");
    st_expect_eq!(zero_recv.payload.len(), 0usize);

    let payload = vec![0xA5; 4096];
    let max = Message::new(MessageHeader::new(0, 0, 2, 0, 4096), payload.clone());
    ctx.router.send(0, max).expect("max payload");
    let max_recv = ctx.router.recv(0).expect("max recv");
    st_expect_eq!(max_recv.payload.len(), 4096usize);
    st_assert!(max_recv.payload.iter().all(|&b| b == 0xA5), "payload integrity");

    st_expect_err!(
        ctx.router.send(3, Message::new(MessageHeader::new(0, 3, 3, 0, 0), vec![])),
        IpcError::NoSuchEndpoint
    );

    st_expect_err!(ctx.router.recv(0), IpcError::QueueEmpty);

    #[cfg(feature = "failpoints")]
    {
        ipc::failpoints::deny_next_send();
        st_expect_err!(
            ctx.router.send(0, Message::new(MessageHeader::new(0, 0, 4, 0, 0), vec![])),
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

    let new_cap = Capability { kind: CapabilityKind::Endpoint(2), rights: Rights::SEND | Rights::RECV };
    ctx.caps.set(2, new_cap).expect("install new capability");
    let fetched = ctx.caps.get(2).expect("fetch newly installed cap");
    st_expect_eq!(fetched.kind, CapabilityKind::Endpoint(2));
}

fn test_map(ctx: &mut Context<'_>) {
    use crate::st_expect_err;

    let flags = PageFlags::VALID | PageFlags::READ | PageFlags::WRITE;
    ctx.address_space.map(0, 0, flags).expect("first mapping");

    st_expect_err!(ctx.address_space.map(1, PAGE_SIZE, flags), MapError::Unaligned);

    #[cfg(feature = "failpoints")]
    {
        mm::failpoints::deny_next_map();
        st_expect_err!(
            ctx.address_space.map(PAGE_SIZE * 2, PAGE_SIZE * 3, flags),
            MapError::PermissionDenied
        );
    }

    st_expect_err!(ctx.address_space.map(0, PAGE_SIZE, flags), MapError::Overlap);
    st_expect_err!(ctx.address_space.map(PAGE_SIZE * 2048, 0, flags), MapError::OutOfRange);
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
}
