// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The task table's in-kernel tests (riscv/none only — they build a scheduler, a router and an
//! address-space manager); split out of `mod.rs` under the structure ratchet (TASK-0333 6a).

use super::*;
use crate::cap::{CapError, Capability, CapabilityKind, Rights};
use crate::ipc::Router;
use crate::mm::AddressSpaceManager;
use crate::sched::Scheduler;

#[test]
fn bootstrap_task_present() {
    let table = TaskTable::new();
    assert_eq!(table.current_pid(), Pid::KERNEL);
    assert_eq!(table.current_task().pid, Pid::KERNEL);
}

#[test]
fn transfer_capability_respects_rights() {
    let mut table = TaskTable::new();
    {
        let caps = table.bootstrap_mut().caps_mut();
        caps.set(
            0,
            Capability { kind: CapabilityKind::Endpoint(0), rights: Rights::SEND | Rights::RECV },
        )
        .unwrap();
    }
    let mut scheduler = Scheduler::new();
    let mut router = Router::new(1);
    let mut spaces = AddressSpaceManager::new();
    let bootstrap_as = spaces.create().unwrap();
    spaces.attach(bootstrap_as, Pid::KERNEL).unwrap();
    table.bootstrap_mut().address_space = Some(bootstrap_as);
    let entry = VirtAddr::instr_aligned(0).unwrap();
    let child = table
        .spawn(
            Pid::KERNEL,
            entry,
            None,
            None,
            0,
            SlotIndex(0),
            &mut scheduler,
            &mut router,
            &mut spaces,
        )
        .unwrap();

    let slot = table.transfer_cap(Pid::KERNEL, child, 0, Rights::RECV).unwrap();
    assert_ne!(slot, 0);
    let cap = table.caps_of(child).unwrap().get(slot).unwrap();
    assert_eq!(cap.kind, CapabilityKind::Endpoint(0));
    assert_eq!(cap.rights, Rights::RECV);

    let err = table.transfer_cap(Pid::KERNEL, child, 0, Rights::MAP).unwrap_err();
    assert_eq!(err, TransferError::Capability(CapError::PermissionDenied));
}
