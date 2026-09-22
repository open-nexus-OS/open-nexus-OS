// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Host-side tests for the device-capability surface of the syscall API
//! (RFC-0098 C3, TASK-0245 P4): the PLIC line travels inside `DeviceMmio` and
//! `cap_query` reports it; read-only VMO aliases query as their own kind.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Unstable

use super::*;
use crate::{
    cap::{Capability, CapabilityKind, Rights},
    mm::AddressSpaceManager,
    syscall::{Args, SyscallTable},
    task::{self, TaskTable},
};

#[derive(Default)]
struct MockTimer;

impl crate::hal::Timer for MockTimer {
    fn now(&self) -> u64 {
        0
    }
    fn set_wakeup(&self, _deadline: u64) {}
}

/// RFC-0098 C3 (TASK-0245 P4): a device capability carries its PLIC line; `cap_query`
/// hands it to the driver (kind 2), a read-only VMO alias queries as kind 3 with no line.
#[test]
fn cap_query_reports_the_device_irq_and_ro_aliases() {
    use super::SYSCALL_CAP_QUERY;

    let mut scheduler = Scheduler::new();
    let mut tasks = TaskTable::new();
    let mut router = ipc::Router::new(0);
    let mut as_manager = AddressSpaceManager::new();
    let timer = MockTimer::default();
    {
        let caps = tasks.bootstrap_mut().caps_mut();
        caps.set(
            48,
            Capability {
                kind: CapabilityKind::DeviceMmio { base: 0x2000_0000, len: 0x1000, irq: 7 },
                rights: Rights::MAP,
            },
        )
        .unwrap();
        caps.set(
            49,
            Capability {
                kind: CapabilityKind::VmoRo { base: 0x3000_0000, len: 0x2000 },
                rights: Rights::MAP,
            },
        )
        .unwrap();
    }
    let kernel_as = as_manager.create().unwrap();
    as_manager.attach(kernel_as, task::Pid::KERNEL).unwrap();
    tasks.bootstrap_mut().address_space = Some(kernel_as);
    let mut ctx = Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);
    let mut table = SyscallTable::new();
    install_handlers(&mut table);

    let mut out = [0u8; 24];
    let args = Args::new([48, out.as_mut_ptr() as usize, 0, 0, 0, 0]);
    table.dispatch(SYSCALL_CAP_QUERY, &mut ctx, &args).unwrap();
    assert_eq!(u32::from_le_bytes([out[0], out[1], out[2], out[3]]), 2, "device kind");
    assert_eq!(u32::from_le_bytes([out[4], out[5], out[6], out[7]]), 7, "the PLIC line");
    assert_eq!(u64::from_le_bytes(out[8..16].try_into().unwrap()), 0x2000_0000);

    let args = Args::new([49, out.as_mut_ptr() as usize, 0, 0, 0, 0]);
    table.dispatch(SYSCALL_CAP_QUERY, &mut ctx, &args).unwrap();
    assert_eq!(u32::from_le_bytes([out[0], out[1], out[2], out[3]]), 3, "read-only alias kind");
    assert_eq!(u32::from_le_bytes([out[4], out[5], out[6], out[7]]), 0, "no line on a VMO");
    assert_eq!(u64::from_le_bytes(out[16..24].try_into().unwrap()), 0x2000);
}
