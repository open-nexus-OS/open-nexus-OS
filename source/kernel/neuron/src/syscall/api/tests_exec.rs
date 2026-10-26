// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Tests for the exec surface of the syscall API (ADR-0080): a process runs from a
//! sealed image, nothing else — an unsealed or non-VMO capability is refused before any space
//! exists, and the seal and the exec dispatch through the table (number 64 once fell outside
//! a 64-wide window and init died of `invalid-syscall`). Compiled with `mod syscall`, i.e. for
//! the target; the QEMU rung `KSELFTEST: exec image …` is the running proof.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! ADR: docs/adr/0080-a-process-runs-from-a-sealed-image-text-mapped-from-its-frames-data-copied-no-other-exec-path.md

use super::*;
use crate::{
    cap::{Capability, CapabilityKind, Rights},
    mm::AddressSpaceManager,
    syscall::{Args, SyscallTable, SYSCALL_EXEC_VMO, SYSCALL_VMO_SEAL},
    task::TaskTable,
};

#[derive(Default)]
struct MockTimer;

impl crate::hal::Timer for MockTimer {
    fn now(&self) -> u64 {
        0
    }
    fn set_wakeup(&self, _deadline: u64) {}
}

#[test]
fn exec_vmo_refuses_an_unsealed_object_before_any_address_space() {
    let mut scheduler = Scheduler::new();
    let mut tasks = TaskTable::new();
    let mut router = ipc::Router::new(1);
    let endpoint = router.create_endpoint(1, None).unwrap();
    tasks
        .bootstrap_mut()
        .caps_mut()
        .set(0, Capability { kind: CapabilityKind::Endpoint(endpoint), rights: Rights::SEND })
        .unwrap();
    let mut as_manager = AddressSpaceManager::new();
    let timer = MockTimer::default();
    let mut ctx = Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);
    // A `Vmo` capability of an object that is not sealed (here: one the table does not hold
    // at all) is refused before a space is created — and a non-VMO capability too.
    ctx.tasks
        .bootstrap_mut()
        .caps_mut()
        .set(
            5,
            Capability { kind: CapabilityKind::Vmo { id: 7, len: 0x2000 }, rights: Rights::MAP },
        )
        .unwrap();
    let before = ctx.address_spaces.stats();

    let err = sys_exec_vmo(&mut ctx, &Args::new([5, 1, 0, 0, 0, 0])).unwrap_err();
    assert_eq!(err, Error::Capability(CapError::PermissionDenied));
    let err = sys_exec_vmo(&mut ctx, &Args::new([0, 1, 0, 0, 0, 0])).unwrap_err();
    assert_eq!(err, Error::Capability(CapError::PermissionDenied));

    let after = ctx.address_spaces.stats();
    assert_eq!((after.live, after.created), (before.live, before.created));
}

#[test]
fn the_seal_and_the_exec_dispatch_through_the_table() {
    // Both numbers must reach their handler — the refusal they return here is the handler's
    // (no VMO in the slot), not the table's.
    let mut scheduler = Scheduler::new();
    let mut tasks = TaskTable::new();
    let mut router = ipc::Router::new(1);
    let mut as_manager = AddressSpaceManager::new();
    let timer = MockTimer::default();
    let mut ctx = Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);
    let mut table = SyscallTable::new();
    install_handlers(&mut table);
    for number in [SYSCALL_VMO_SEAL, SYSCALL_EXEC_VMO] {
        let err = table.dispatch(number, &mut ctx, &Args::new([9, 1, 0, 0, 0, 0])).unwrap_err();
        assert_ne!(err, Error::InvalidSyscall, "syscall {number} is outside the window");
    }
}
