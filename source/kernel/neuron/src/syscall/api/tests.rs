// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Host-side unit tests for the syscall API handlers (moved verbatim
//! from the former single-file api.rs `mod tests`).
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: neuron host tests + QEMU marker gates (just test-os / ci-os-smp)
//! ADR: docs/adr/0016-kernel-libs-architecture.md

use super::*;
use crate::{
    cap::{Capability, CapabilityKind, Rights},
    mm::AddressSpaceManager,
    syscall::{
        Args, SyscallTable, SYSCALL_CAP_TRANSFER, SYSCALL_IPC_RECV_V1, SYSCALL_RECV, SYSCALL_SEND,
        SYSCALL_SPAWN,
    },
    task::TaskTable,
    BootstrapMsg,
};

#[derive(Default)]
struct MockTimer {
    now: core::cell::Cell<u64>,
}

impl MockTimer {
    fn set_now(&self, now: u64) {
        self.now.set(now);
    }
}

impl crate::hal::Timer for MockTimer {
    fn now(&self) -> u64 {
        self.now.get()
    }
    fn set_wakeup(&self, _deadline: u64) {}
}

#[test]
fn vmo_pool_stats_track_used_remaining_and_peak() {
    let mut backing = [0xAAu8; PAGE_SIZE * 2];
    let mut pool = VmoPool::with_window(backing.as_mut_ptr() as usize, backing.len());
    let before = pool.stats();
    assert_eq!(before.used, 0);
    assert_eq!(before.remaining, backing.len());

    let (base, len) = pool.allocate(PAGE_SIZE / 2).expect("allocate vmo page");
    assert_eq!(base, backing.as_ptr() as usize);
    assert_eq!(len, PAGE_SIZE);
    assert_eq!(&backing[..PAGE_SIZE], &[0u8; PAGE_SIZE]);

    let after = pool.stats();
    assert_eq!(after.used, PAGE_SIZE);
    assert_eq!(after.remaining, PAGE_SIZE);
    assert_eq!(after.peak_used, PAGE_SIZE);
    assert!(pool.allocate(PAGE_SIZE * 2).is_err());
    assert_eq!(pool.stats().peak_used, PAGE_SIZE);
}

#[test]
fn vmo_pool_free_reuses_coalesces_and_rejects_double_free() {
    let mut backing = [0xAAu8; PAGE_SIZE * 4];
    let mut pool = VmoPool::with_window(backing.as_mut_ptr() as usize, backing.len());
    let (a, _) = pool.allocate(PAGE_SIZE).expect("alloc a");
    let (b, lb) = pool.allocate(PAGE_SIZE).expect("alloc b");
    let (c, lc) = pool.allocate(PAGE_SIZE).expect("alloc c");

    // A freed middle range is reused (and re-zeroed) by the next fitting allocate.
    pool.free(b, lb).expect("free b");
    backing[PAGE_SIZE] = 0xCC;
    let (b2, _) = pool.allocate(PAGE_SIZE / 2).expect("realloc b");
    assert_eq!(b2, b);
    assert_eq!(backing[PAGE_SIZE], 0);

    // Double-free and out-of-span frees are rejected.
    pool.free(b, lb).expect("free b again (was reallocated)");
    assert!(pool.free(b, lb).is_err());
    assert!(pool.free(pool.limit, PAGE_SIZE).is_err());
    assert!(pool.free(a + 1, PAGE_SIZE).is_err());

    // Freeing the tail coalesces through the adjacent free middle back to
    // the bump frontier: only `a` stays used, the rest is one big span.
    pool.free(c, lc).expect("free c");
    assert_eq!(pool.stats().used, PAGE_SIZE);
    let (big, big_len) = pool.allocate(PAGE_SIZE * 3).expect("realloc whole tail");
    assert_eq!(big, a + PAGE_SIZE);
    assert_eq!(big_len, PAGE_SIZE * 3);
}

#[test]
fn cap_table_vmo_overlap_count_sees_aliases() {
    let mut table = crate::cap::CapTable::new();
    let cap =
        Capability { kind: CapabilityKind::Vmo { base: 0x1000, len: 0x2000 }, rights: Rights::MAP };
    table.set(3, cap).unwrap();
    assert_eq!(table.vmo_overlap_count(0x1000, 0x2000), 1);
    // A clone in the same table is an alias.
    table.set(4, cap).unwrap();
    assert_eq!(table.vmo_overlap_count(0x1000, 0x2000), 2);
    // Partial overlap counts; disjoint does not.
    assert_eq!(table.vmo_overlap_count(0x2000, 0x1000), 2);
    assert_eq!(table.vmo_overlap_count(0x3000, 0x1000), 0);
}

#[test]
fn exec_v2_reclaims_address_space_on_load_validation_failure() {
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
    let before = ctx.address_spaces.stats();
    let mut elf = [0u8; 120];
    elf[0..4].copy_from_slice(b"\x7FELF");
    elf[4] = 2; // ELF64
    elf[5] = 1; // little endian
    elf[24..32].copy_from_slice(&0x1000usize.to_le_bytes());
    elf[32..40].copy_from_slice(&64usize.to_le_bytes());
    elf[54..56].copy_from_slice(&56u16.to_le_bytes());
    elf[56..58].copy_from_slice(&1u16.to_le_bytes());
    elf[64..68].copy_from_slice(&1u32.to_le_bytes()); // PT_LOAD
    elf[68..72].copy_from_slice(&4u32.to_le_bytes()); // PF_R
    elf[72..80].copy_from_slice(&112usize.to_le_bytes());
    elf[80..88].copy_from_slice(&0x4000usize.to_le_bytes());
    elf[96..104].copy_from_slice(&8usize.to_le_bytes()); // filesz
    elf[104..112].copy_from_slice(&4usize.to_le_bytes()); // memsz < filesz

    let err = sys_exec_v2(&mut ctx, &Args::new([elf.as_ptr() as usize, elf.len(), 1, 0, 0, 0]))
        .unwrap_err();

    assert_eq!(err, Error::AddressSpace(AddressSpaceError::InvalidArgs));
    let after = ctx.address_spaces.stats();
    assert_eq!(after.live, before.live);
    assert!(after.destroyed >= before.destroyed + 1);
}

#[test]
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
fn send_recv_roundtrip() {
    let mut scheduler = Scheduler::new();
    let mut tasks = TaskTable::new();
    {
        let caps = tasks.bootstrap_mut().caps_mut();
        let _ = caps.set(
            0,
            Capability { kind: CapabilityKind::Endpoint(0), rights: Rights::SEND | Rights::RECV },
        );
    }
    let mut router = ipc::Router::new(1);
    let mut as_manager = AddressSpaceManager::new();
    let kernel_as = as_manager.create().unwrap();
    as_manager.attach(kernel_as, task::Pid::KERNEL).unwrap();
    tasks.bootstrap_mut().address_space = Some(kernel_as);
    let timer = crate::hal::virt::VirtMachine::new();
    let mut ctx =
        Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, timer.timer());
    let mut table = SyscallTable::new();
    install_handlers(&mut table);

    table.dispatch(SYSCALL_SEND, &mut ctx, &Args::new([0, 1, 0, 0, 0, 0])).unwrap();
    let len = table.dispatch(SYSCALL_RECV, &mut ctx, &Args::new([0, 0, 0, 0, 0, 0])).unwrap();
    assert_eq!(len, 0);
    assert!(ctx.last_message().is_some());
}

#[test]
fn ipc_v1_recv_deadline_times_out() {
    let mut scheduler = Scheduler::new();
    let mut tasks = TaskTable::new();
    {
        let caps = tasks.bootstrap_mut().caps_mut();
        caps.set(
            0,
            Capability { kind: CapabilityKind::Endpoint(0), rights: Rights::SEND | Rights::RECV },
        )
        .unwrap();
    }
    let mut router = ipc::Router::new(1);
    let mut as_manager = AddressSpaceManager::new();

    let timer = MockTimer::default();
    timer.set_now(100);

    let mut ctx = Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);

    let mut table = SyscallTable::new();
    install_handlers(&mut table);

    let mut hdr = crate::ipc::header::MessageHeader::new(0, 0, 0, 0, 0).to_le_bytes();
    let mut payload = [0u8; 8];
    let args = Args::new([
        0, // slot 0
        hdr.as_mut_ptr() as usize,
        payload.as_mut_ptr() as usize,
        payload.len(),
        0,   // sys_flags: blocking
        100, // deadline_ns: already expired
    ]);

    let err = table.dispatch(SYSCALL_IPC_RECV_V1, &mut ctx, &args).unwrap_err();
    assert_eq!(err, Error::Ipc(ipc::IpcError::TimedOut));
}

#[test]
fn ipc_v1_send_queue_full_nonblock() {
    let mut scheduler = Scheduler::new();
    let mut tasks = TaskTable::new();
    let mut router = ipc::Router::new(0);
    let mut as_manager = AddressSpaceManager::new();
    let timer = MockTimer::default();

    // Create a depth-1 endpoint and grant SEND rights in slot 0.
    let endpoint = router.create_endpoint(1, None).unwrap();
    {
        let caps = tasks.bootstrap_mut().caps_mut();
        caps.set(0, Capability { kind: CapabilityKind::Endpoint(endpoint), rights: Rights::SEND })
            .unwrap();
    }

    let mut ctx = Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);

    // Minimal valid header: len=0 (matches payload_len=0).
    let mut hdr = crate::ipc::header::MessageHeader::new(0, 0, 0, 0, 0).to_le_bytes();
    let args = Args::new([
        0,                         // cap slot
        hdr.as_mut_ptr() as usize, // header_ptr
        0,                         // payload_ptr (len=0)
        0,                         // payload_len
        IPC_SYS_NONBLOCK,          // sys_flags
        0,                         // deadline_ns
    ]);

    // First send fills the queue.
    assert!(sys_ipc_send_v1(&mut ctx, &args).is_ok());

    // Second send must fail with QueueFull (mapped to EAGAIN by trap.rs).
    match sys_ipc_send_v1(&mut ctx, &args) {
        Err(Error::Ipc(ipc::IpcError::QueueFull)) => {}
        other => panic!("expected QueueFull, got {:?}", other),
    }
}

#[test]
fn ipc_v1_cap_move_blocking_deadline_times_out_and_preserves_cap() {
    let mut scheduler = Scheduler::new();
    let mut tasks = TaskTable::new();
    let mut router = ipc::Router::new(0);
    let mut as_manager = AddressSpaceManager::new();
    let timer = MockTimer::default();
    timer.set_now(100);

    // Endpoint depth=1, fill it so subsequent send hits QueueFull.
    let endpoint = router.create_endpoint(1, None).unwrap();
    let hdr0 = crate::ipc::header::MessageHeader::new(0, endpoint, 0, 0, 0);
    router.send(endpoint, crate::ipc::Message::new(hdr0, alloc::vec::Vec::new(), None)).unwrap();

    // Sender has SEND on endpoint in slot 0.
    {
        let caps = tasks.bootstrap_mut().caps_mut();
        caps.set(0, Capability { kind: CapabilityKind::Endpoint(endpoint), rights: Rights::SEND })
            .unwrap();
        // Movable cap in slot 3 (no MANAGE). Use a live endpoint to avoid hardening rejection.
        let live_cap_ep = router.create_endpoint(1, None).unwrap();
        caps.set(
            3,
            Capability { kind: CapabilityKind::Endpoint(live_cap_ep), rights: Rights::SEND },
        )
        .unwrap();
    }

    let mut ctx = Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);

    // CAP_MOVE header: src=3, flags=CAP_MOVE, len=0.
    const IPC_HDR_CAP_MOVE: u16 = 1 << 0;
    let mut hdr =
        crate::ipc::header::MessageHeader::new(3, 0, 0, IPC_HDR_CAP_MOVE, 0).to_le_bytes();

    let args = Args::new([
        0,                         // endpoint cap slot
        hdr.as_mut_ptr() as usize, // header_ptr
        0,                         // payload_ptr (len=0)
        0,                         // payload_len
        0,                         // sys_flags (blocking)
        50,                        // deadline_ns (already expired vs now=100)
    ]);

    match sys_ipc_send_v1(&mut ctx, &args) {
        Err(Error::Ipc(ipc::IpcError::TimedOut)) => {}
        other => panic!("expected TimedOut, got {:?}", other),
    }

    // Cap must still be present in slot 3 (rollback guaranteed).
    assert!(ctx.tasks.current_caps_mut().get(3).is_ok());
}

#[test]
fn ipc_v1_cap_move_recv_no_space_requeues_message() {
    let mut scheduler = Scheduler::new();
    let mut tasks = TaskTable::new();
    let mut router = ipc::Router::new(0);
    let mut as_manager = AddressSpaceManager::new();
    let timer = MockTimer::default();

    // Endpoint to receive from.
    let endpoint = router.create_endpoint(2, None).unwrap();

    // Fill all cap slots in the current task so allocation of the moved cap will fail.
    {
        let caps = tasks.bootstrap_mut().caps_mut();
        for i in 0..96 {
            caps.set(
                i,
                Capability { kind: CapabilityKind::Endpoint(i as u32), rights: Rights::SEND },
            )
            .unwrap();
        }
        // Slot 0 must be a RECV cap for the endpoint we will recv from.
        caps.set(0, Capability { kind: CapabilityKind::Endpoint(endpoint), rights: Rights::RECV })
            .unwrap();
    }

    // Enqueue a message carrying a moved cap (some arbitrary endpoint cap).
    let hdr = crate::ipc::header::MessageHeader::new(0, endpoint, 0, 0, 0);
    router
        .send(
            endpoint,
            crate::ipc::Message::new(
                hdr,
                alloc::vec::Vec::new(),
                Some(Capability { kind: CapabilityKind::Endpoint(999), rights: Rights::SEND }),
            ),
        )
        .unwrap();

    let mut ctx = Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);

    let mut out_hdr = [0u8; 16];
    let mut out_buf = [0u8; 8];
    let args = Args::new([
        0,                             // slot 0 (RECV cap)
        out_hdr.as_mut_ptr() as usize, // header_out_ptr
        out_buf.as_mut_ptr() as usize, // payload_out_ptr
        out_buf.len(),                 // payload_out_max
        0,                             // sys_flags (blocking ok)
        0,                             // deadline_ns
    ]);

    match sys_ipc_recv_v1(&mut ctx, &args) {
        Err(Error::Ipc(ipc::IpcError::NoSpace)) => {}
        other => panic!("expected NoSpace, got {:?}", other),
    }

    // Free one cap slot, then retry: recv should succeed and moved cap should be allocated.
    let _ = ctx.tasks.current_caps_mut().take(1);
    let n = sys_ipc_recv_v1(&mut ctx, &args).expect("recv after freeing slot");
    assert_eq!(n, 0);
    // The moved cap should have been allocated into some free slot (likely 1).
    assert!(ctx.tasks.current_caps_mut().get(1).is_ok());
}

#[test]
fn cap_clone_returns_no_space_when_table_full() {
    let mut scheduler = Scheduler::new();
    let mut tasks = TaskTable::new();
    let mut router = ipc::Router::new(0);
    let mut as_manager = AddressSpaceManager::new();
    let timer = MockTimer::default();

    // Fill all cap slots (96). Ensure there is a valid cap at slot 0 to clone.
    {
        let caps = tasks.bootstrap_mut().caps_mut();
        for i in 0..96 {
            caps.set(
                i,
                Capability { kind: CapabilityKind::Endpoint(i as u32), rights: Rights::SEND },
            )
            .unwrap();
        }
    }

    let mut ctx = Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);

    match sys_cap_clone(&mut ctx, &Args::new([0, 0, 0, 0, 0, 0])) {
        Err(Error::Capability(CapError::NoSpace)) => {}
        other => panic!("expected CapError::NoSpace, got {:?}", other),
    }
}

#[test]
fn cap_transfer_returns_no_space_when_child_table_full() {
    let mut scheduler = Scheduler::new();
    let mut tasks = TaskTable::new();
    let mut router = ipc::Router::new(0);
    let mut as_manager = AddressSpaceManager::new();
    let timer = MockTimer::default();

    // Selftest-only child creation avoids full address-space/spawn machinery in host tests.
    let child = tasks.selftest_create_dummy_task(task::Pid::KERNEL, &mut scheduler);

    // Cap to transfer lives in parent slot 3.
    {
        let caps = tasks.bootstrap_mut().caps_mut();
        caps.set(3, Capability { kind: CapabilityKind::Endpoint(123), rights: Rights::SEND })
            .unwrap();
    }

    // Fill the child's cap table fully so allocation fails.
    {
        let child_caps = tasks.task_mut(child).unwrap().caps_mut();
        for i in 0..96 {
            let _ = child_caps.set(
                i,
                Capability { kind: CapabilityKind::Endpoint(i as u32), rights: Rights::SEND },
            );
        }
    }

    let mut ctx = Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);

    // Args: child pid, parent slot, rights mask
    let args = Args::new([child.as_index(), 3, Rights::SEND.bits() as usize, 0, 0, 0]);
    match sys_cap_transfer(&mut ctx, &args) {
        Err(Error::Transfer(task::TransferError::Capability(CapError::NoSpace))) => {}
        other => panic!("expected TransferError::Capability(NoSpace), got {:?}", other),
    }
}

#[test]
fn ipc_endpoint_create_quota_enforced() {
    let mut router = ipc::Router::new(0);
    // Keep this aligned with ipc::MAX_ENDPOINTS.
    for _ in 0..384 {
        router.create_endpoint(1, None).expect("create");
    }
    match router.create_endpoint(1, None) {
        Err(ipc::IpcError::NoSpace) => {}
        other => panic!("expected NoSpace, got {:?}", other),
    }
}

#[test]
fn ipc_endpoint_create_owner_quota_enforced() {
    let mut router = ipc::Router::new(0);
    // Keep this aligned with ipc::MAX_ENDPOINTS_PER_OWNER.
    for _ in 0..96 {
        router.create_endpoint(1, Some(7)).expect("create");
    }
    match router.create_endpoint(1, Some(7)) {
        Err(ipc::IpcError::NoSpace) => {}
        other => panic!("expected NoSpace, got {:?}", other),
    }
    // Different owner should still be allowed (global limit not hit yet).
    router.create_endpoint(1, Some(8)).expect("create other owner");
}

#[test]
fn ipc_v1_rights_denied_send() {
    let mut scheduler = Scheduler::new();
    let mut tasks = TaskTable::new();
    {
        let caps = tasks.bootstrap_mut().caps_mut();
        // Slot 0 has RECV only (no SEND).
        caps.set(0, Capability { kind: CapabilityKind::Endpoint(0), rights: Rights::RECV })
            .unwrap();
    }
    let mut router = ipc::Router::new(1);
    let mut as_manager = AddressSpaceManager::new();
    let timer = MockTimer::default();
    let mut ctx = Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);
    let mut table = SyscallTable::new();
    install_handlers(&mut table);

    let hdr = crate::ipc::header::MessageHeader::new(0, 0, 1, 0, 0).to_le_bytes();
    let args = Args::new([
        0,                     // slot 0
        hdr.as_ptr() as usize, // header_ptr
        0,                     // payload_ptr (len=0)
        0,                     // payload_len
        IPC_SYS_NONBLOCK,      // sys_flags
        0,                     // deadline_ns
    ]);
    let err = table.dispatch(SYSCALL_IPC_SEND_V1, &mut ctx, &args).unwrap_err();
    assert_eq!(err, Error::Capability(CapError::PermissionDenied));
}

#[test]
fn ipc_v1_rights_denied_recv() {
    let mut scheduler = Scheduler::new();
    let mut tasks = TaskTable::new();
    {
        let caps = tasks.bootstrap_mut().caps_mut();
        // Slot 0 has SEND only (no RECV).
        caps.set(0, Capability { kind: CapabilityKind::Endpoint(0), rights: Rights::SEND })
            .unwrap();
    }
    let mut router = ipc::Router::new(1);
    let mut as_manager = AddressSpaceManager::new();
    let timer = MockTimer::default();
    let mut ctx = Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);
    let mut table = SyscallTable::new();
    install_handlers(&mut table);

    let mut hdr_out = [0u8; 16];
    let mut payload_out = [0u8; 8];
    let args = Args::new([
        0, // slot 0
        hdr_out.as_mut_ptr() as usize,
        payload_out.as_mut_ptr() as usize,
        payload_out.len(),
        IPC_SYS_NONBLOCK, // sys_flags
        0,                // deadline_ns
    ]);
    let err = table.dispatch(SYSCALL_IPC_RECV_V1, &mut ctx, &args).unwrap_err();
    assert_eq!(err, Error::Capability(CapError::PermissionDenied));
}

#[test]
fn ipc_v1_cap_move_roundtrip_same_task() {
    let mut scheduler = Scheduler::new();
    let mut tasks = TaskTable::new();
    {
        let caps = tasks.bootstrap_mut().caps_mut();
        caps.set(
            0,
            Capability { kind: CapabilityKind::Endpoint(0), rights: Rights::SEND | Rights::RECV },
        )
        .unwrap();
        // Slot 3: moveable VMO cap.
        caps.set(
            3,
            Capability {
                kind: CapabilityKind::Vmo { base: 0x9000_0000, len: PAGE_SIZE },
                rights: Rights::MAP,
            },
        )
        .unwrap();
    }
    let mut router = ipc::Router::new(1);
    let mut as_manager = AddressSpaceManager::new();
    let timer = MockTimer::default();
    let mut ctx = Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);
    let mut table = SyscallTable::new();
    install_handlers(&mut table);

    let payload = [1u8, 2, 3, 4];
    const IPC_HDR_CAP_MOVE: u16 = 1 << 0;
    let mut send_hdr = crate::ipc::header::MessageHeader::new(
        3, // cap slot to move (interpreted only when CAP_MOVE is set)
        0,
        0x55,
        IPC_HDR_CAP_MOVE,
        payload.len() as u32,
    )
    .to_le_bytes();

    // Send with CAP_MOVE (nonblocking).
    let send_args = Args::new([
        0,                              // endpoint cap slot
        send_hdr.as_mut_ptr() as usize, // header ptr
        payload.as_ptr() as usize,      // payload ptr
        payload.len(),                  // payload len
        IPC_SYS_NONBLOCK as usize,      // sys_flags
        0,                              // deadline
    ]);
    table.dispatch(SYSCALL_IPC_SEND_V1, &mut ctx, &send_args).unwrap();

    // Sender cap slot must be empty after send.
    assert_eq!(ctx.tasks.bootstrap_mut().caps_mut().get(3).unwrap_err(), CapError::InvalidSlot);

    // Receive and verify the cap was allocated back into slot 3.
    let mut out_hdr = [0u8; 16];
    let mut out_payload = [0u8; 8];
    let recv_args = Args::new([
        0,
        out_hdr.as_mut_ptr() as usize,
        out_payload.as_mut_ptr() as usize,
        out_payload.len(),
        IPC_SYS_NONBLOCK as usize,
        0,
    ]);
    let n = table.dispatch(SYSCALL_IPC_RECV_V1, &mut ctx, &recv_args).unwrap();
    assert_eq!(n, payload.len());
    assert_eq!(&out_payload[..payload.len()], &payload);

    let hdr = crate::ipc::header::MessageHeader::from_le_bytes(out_hdr);
    let moved_slot = hdr.src as usize;
    let cap = ctx.tasks.bootstrap_mut().caps_mut().get(moved_slot).unwrap();
    assert!(matches!(cap.kind, CapabilityKind::Vmo { .. }));
    // Original slot remains empty (we moved *out* of slot 3).
    assert_eq!(ctx.tasks.bootstrap_mut().caps_mut().get(3).unwrap_err(), CapError::InvalidSlot);
}

#[test]
fn ipc_v1_nonblocking_queue_empty() {
    let mut scheduler = Scheduler::new();
    let mut tasks = TaskTable::new();
    {
        let caps = tasks.bootstrap_mut().caps_mut();
        caps.set(0, Capability { kind: CapabilityKind::Endpoint(0), rights: Rights::RECV })
            .unwrap();
    }
    let mut router = ipc::Router::new(1);
    let mut as_manager = AddressSpaceManager::new();
    let timer = MockTimer::default();
    let mut ctx = Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);
    let mut table = SyscallTable::new();
    install_handlers(&mut table);

    let mut hdr_out = [0u8; 16];
    let mut payload_out = [0u8; 8];
    let args = Args::new([
        0,
        hdr_out.as_mut_ptr() as usize,
        payload_out.as_mut_ptr() as usize,
        payload_out.len(),
        IPC_SYS_NONBLOCK,
        0,
    ]);
    let err = table.dispatch(SYSCALL_IPC_RECV_V1, &mut ctx, &args).unwrap_err();
    assert_eq!(err, Error::Ipc(ipc::IpcError::QueueEmpty));
}

#[test]
fn ipc_v1_nonblocking_queue_full() {
    let mut scheduler = Scheduler::new();
    let mut tasks = TaskTable::new();
    {
        let caps = tasks.bootstrap_mut().caps_mut();
        caps.set(0, Capability { kind: CapabilityKind::Endpoint(0), rights: Rights::SEND })
            .unwrap();
    }
    let mut router = ipc::Router::new(1);
    let mut as_manager = AddressSpaceManager::new();
    let timer = MockTimer::default();
    let mut ctx = Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);
    let mut table = SyscallTable::new();
    install_handlers(&mut table);

    let hdr = crate::ipc::header::MessageHeader::new(0, 0, 1, 0, 0).to_le_bytes();
    let args = Args::new([0, hdr.as_ptr() as usize, 0, 0, IPC_SYS_NONBLOCK, 0]);

    // Router endpoint depth is 8. Fill it.
    for _ in 0..8 {
        table.dispatch(SYSCALL_IPC_SEND_V1, &mut ctx, &args).expect("send should fit");
    }
    let err = table.dispatch(SYSCALL_IPC_SEND_V1, &mut ctx, &args).unwrap_err();
    assert_eq!(err, Error::Ipc(ipc::IpcError::QueueFull));
}

#[test]
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
fn spawn_and_transfer_syscalls() {
    let mut scheduler = Scheduler::new();
    let mut tasks = TaskTable::new();
    {
        let caps = tasks.bootstrap_mut().caps_mut();
        caps.set(
            0,
            Capability { kind: CapabilityKind::Endpoint(0), rights: Rights::SEND | Rights::RECV },
        )
        .unwrap();
    }
    let mut router = ipc::Router::new(2);
    let mut as_manager = AddressSpaceManager::new();
    let kernel_as = as_manager.create().unwrap();
    as_manager.attach(kernel_as, task::Pid::KERNEL).unwrap();
    tasks.bootstrap_mut().address_space = Some(kernel_as);
    let timer = crate::hal::virt::VirtMachine::new();
    let mut ctx =
        Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, timer.timer());
    let mut table = SyscallTable::new();
    install_handlers(&mut table);

    let child = task::Pid::from_raw(
        table.dispatch(SYSCALL_SPAWN, &mut ctx, &Args::new([0x1000, 0, 0, 0, 0, 0])).unwrap()
            as u32,
    );
    assert_eq!(child, task::Pid::from_raw(1));
    let msg = ctx.router.recv(0).unwrap();
    assert_eq!(msg.payload.len(), core::mem::size_of::<BootstrapMsg>());

    let slot = table
        .dispatch(
            SYSCALL_CAP_TRANSFER,
            &mut ctx,
            &Args::new([child.as_index(), 0, Rights::SEND.bits() as usize, 0, 0, 0]),
        )
        .unwrap();
    assert_ne!(slot, 0);
    let cap = ctx.tasks.caps_of(child).unwrap().get(slot).unwrap();
    assert_eq!(cap.rights, Rights::SEND);

    // Subset mask (2): transfer RECV only.
    let slot2 = table
        .dispatch(
            SYSCALL_CAP_TRANSFER,
            &mut ctx,
            &Args::new([child.as_index(), 0, Rights::RECV.bits() as usize, 0, 0, 0]),
        )
        .unwrap();
    let cap2 = ctx.tasks.caps_of(child).unwrap().get(slot2).unwrap();
    assert_eq!(cap2.rights, Rights::RECV);

    // Superset rejection: MAP is not allowed by the parent cap in slot 0.
    let err = table
        .dispatch(
            SYSCALL_CAP_TRANSFER,
            &mut ctx,
            &Args::new([child.as_index(), 0, Rights::MAP.bits() as usize, 0, 0, 0]),
        )
        .unwrap_err();
    assert_eq!(err, Error::Transfer(task::TransferError::Capability(CapError::PermissionDenied)));
}

#[test]
fn cap_transfer_rejects_invalid_rights_mask() {
    let mut scheduler = Scheduler::new();
    let mut tasks = TaskTable::new();
    {
        let caps = tasks.bootstrap_mut().caps_mut();
        caps.set(
            0,
            Capability { kind: CapabilityKind::Endpoint(0), rights: Rights::SEND | Rights::RECV },
        )
        .unwrap();
    }
    let mut router = ipc::Router::new(1);
    let mut as_manager = AddressSpaceManager::new();
    let timer = MockTimer::default();
    let mut ctx = Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);
    let mut table = SyscallTable::new();
    install_handlers(&mut table);

    // rights_bits contains an unknown bit (bit 31); decode must fail deterministically.
    let invalid_bits = 1u32 << 31;
    let err = table
        .dispatch(
            SYSCALL_CAP_TRANSFER,
            &mut ctx,
            &Args::new([1, 0, invalid_bits as usize, 0, 0, 0]),
        )
        .unwrap_err();
    assert_eq!(err, Error::Transfer(task::TransferError::Capability(CapError::PermissionDenied)));
}

#[test]
fn cap_close_is_local_drop_only() {
    let mut scheduler = Scheduler::new();
    let mut tasks = TaskTable::new();
    // Seed caps before building a Context (which mutably borrows the task table).
    {
        let caps = tasks.bootstrap_mut().caps_mut();
        caps.set(
            0,
            Capability {
                kind: CapabilityKind::Endpoint(0),
                rights: Rights::SEND | Rights::RECV | Rights::MANAGE,
            },
        )
        .unwrap();
        // Also keep a non-MANAGE sender reference to the same endpoint so we can observe
        // "global close" (router returns NoSuchEndpoint).
        caps.set(1, Capability { kind: CapabilityKind::Endpoint(0), rights: Rights::SEND })
            .unwrap();
    }

    let mut router = ipc::Router::new(1);
    let mut as_manager = AddressSpaceManager::new();
    let timer = MockTimer::default();
    let mut ctx = Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);
    let mut table = SyscallTable::new();
    install_handlers(&mut table);

    // Close the cap: local drop only (endpoint stays alive).
    table
        .dispatch(crate::syscall::SYSCALL_CAP_CLOSE, &mut ctx, &Args::new([0, 0, 0, 0, 0, 0]))
        .unwrap();

    // Endpoint is still alive, so sending on the other cap should succeed.
    let hdr = crate::ipc::header::MessageHeader::new(0, 0, 1, 0, 0).to_le_bytes();
    let send_args = Args::new([
        1,                     // slot 1 (SEND)
        hdr.as_ptr() as usize, // header_ptr
        0,
        0,
        IPC_SYS_NONBLOCK,
        0,
    ]);
    table.dispatch(SYSCALL_IPC_SEND_V1, &mut ctx, &send_args).unwrap();
}

#[test]
fn endpoint_close_denied_without_manage() {
    let mut scheduler = Scheduler::new();
    let mut tasks = TaskTable::new();
    // Seed caps before building a Context (which mutably borrows the task table).
    {
        let caps = tasks.bootstrap_mut().caps_mut();
        // Slot 0: endpoint cap WITHOUT MANAGE (attempting endpoint_close should be denied).
        caps.set(
            0,
            Capability { kind: CapabilityKind::Endpoint(0), rights: Rights::SEND | Rights::RECV },
        )
        .unwrap();
        // Slot 1: a sender ref so we can verify the endpoint is still alive after the denied close.
        caps.set(1, Capability { kind: CapabilityKind::Endpoint(0), rights: Rights::SEND })
            .unwrap();
    }

    let mut router = ipc::Router::new(1);
    let mut as_manager = AddressSpaceManager::new();
    let timer = MockTimer::default();
    let mut ctx = Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);
    let mut table = SyscallTable::new();
    install_handlers(&mut table);

    let err = table
        .dispatch(
            crate::syscall::SYSCALL_IPC_ENDPOINT_CLOSE,
            &mut ctx,
            &Args::new([0, 0, 0, 0, 0, 0]),
        )
        .unwrap_err();
    assert_eq!(err, Error::Capability(CapError::PermissionDenied));

    // Endpoint should still be alive, so send should succeed.
    let hdr = crate::ipc::header::MessageHeader::new(0, 0, 1, 0, 0).to_le_bytes();
    let send_args = Args::new([
        1,                     // slot 1 (SEND)
        hdr.as_ptr() as usize, // header_ptr
        0,
        0,
        IPC_SYS_NONBLOCK,
        0,
    ]);
    table.dispatch(SYSCALL_IPC_SEND_V1, &mut ctx, &send_args).unwrap();
}

#[test]
fn endpoint_close_allowed_with_manage_closes_globally() {
    let mut scheduler = Scheduler::new();
    let mut tasks = TaskTable::new();
    {
        let caps = tasks.bootstrap_mut().caps_mut();
        // Slot 0: MANAGE authority to close.
        caps.set(
            0,
            Capability {
                kind: CapabilityKind::Endpoint(0),
                rights: Rights::SEND | Rights::RECV | Rights::MANAGE,
            },
        )
        .unwrap();
        // Slot 1: non-MANAGE sender reference used to observe global close.
        caps.set(1, Capability { kind: CapabilityKind::Endpoint(0), rights: Rights::SEND })
            .unwrap();
    }

    let mut router = ipc::Router::new(1);
    let mut as_manager = AddressSpaceManager::new();
    let timer = MockTimer::default();
    let mut ctx = Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);
    let mut table = SyscallTable::new();
    install_handlers(&mut table);

    table
        .dispatch(
            crate::syscall::SYSCALL_IPC_ENDPOINT_CLOSE,
            &mut ctx,
            &Args::new([0, 0, 0, 0, 0, 0]),
        )
        .unwrap();

    let hdr = crate::ipc::header::MessageHeader::new(0, 0, 1, 0, 0).to_le_bytes();
    let send_args = Args::new([
        1,                     // slot 1 (still a valid cap, but endpoint should be closed)
        hdr.as_ptr() as usize, // header_ptr
        0,
        0,
        IPC_SYS_NONBLOCK,
        0,
    ]);
    let err = table.dispatch(SYSCALL_IPC_SEND_V1, &mut ctx, &send_args).unwrap_err();
    assert_eq!(err, Error::Ipc(ipc::IpcError::NoSuchEndpoint));
}

#[test]
fn cap_transfer_rejects_manage_right() {
    let mut scheduler = Scheduler::new();
    let mut tasks = TaskTable::new();
    {
        let caps = tasks.bootstrap_mut().caps_mut();
        caps.set(
            0,
            Capability {
                kind: CapabilityKind::Endpoint(0),
                rights: Rights::SEND | Rights::RECV | Rights::MANAGE,
            },
        )
        .unwrap();
    }

    let mut router = ipc::Router::new(1);
    let mut as_manager = AddressSpaceManager::new();
    let timer = MockTimer::default();
    let mut ctx = Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);
    let mut table = SyscallTable::new();
    install_handlers(&mut table);

    // Attempt to transfer MANAGE: should be denied (Phase-2 hardening).
    let err = table
        .dispatch(
            SYSCALL_CAP_TRANSFER,
            &mut ctx,
            // Child PID does not need to exist; the rights check rejects first.
            &Args::new([1, 0, Rights::MANAGE.bits() as usize, 0, 0, 0]),
        )
        .unwrap_err();
    assert_eq!(err, Error::Transfer(task::TransferError::Capability(CapError::PermissionDenied)));
}

#[test]
fn cap_transfer_allows_manage_for_endpoint_factory() {
    let mut scheduler = Scheduler::new();
    let mut tasks = TaskTable::new();
    {
        let caps = tasks.bootstrap_mut().caps_mut();
        caps.set(0, Capability { kind: CapabilityKind::EndpointFactory, rights: Rights::MANAGE })
            .unwrap();
    }
    let mut router = ipc::Router::new(1);
    let mut as_manager = AddressSpaceManager::new();
    let timer = MockTimer::default();
    let mut ctx = Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);
    let mut table = SyscallTable::new();
    install_handlers(&mut table);

    let err = table
        .dispatch(
            SYSCALL_CAP_TRANSFER,
            &mut ctx,
            &Args::new([1, 0, Rights::MANAGE.bits() as usize, 0, 0, 0]),
        )
        .unwrap_err();
    // Fails because child doesn't exist, *not* because MANAGE is rejected for EndpointFactory.
    assert_eq!(err, Error::Transfer(task::TransferError::InvalidChild));
}

#[test]
fn cap_clone_duplicates_local_cap() {
    let mut scheduler = Scheduler::new();
    let mut tasks = TaskTable::new();
    {
        let caps = tasks.bootstrap_mut().caps_mut();
        caps.set(
            3,
            Capability {
                kind: CapabilityKind::Vmo { base: 0x9000_0000, len: PAGE_SIZE },
                rights: Rights::MAP,
            },
        )
        .unwrap();
    }
    let mut router = ipc::Router::new(1);
    let mut as_manager = AddressSpaceManager::new();
    let timer = MockTimer::default();
    let mut ctx = Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);
    let mut table = SyscallTable::new();
    install_handlers(&mut table);

    let new_slot = table
        .dispatch(crate::syscall::SYSCALL_CAP_CLONE, &mut ctx, &Args::new([3, 0, 0, 0, 0, 0]))
        .unwrap();
    assert_ne!(new_slot, 3);
    assert!(matches!(
        ctx.tasks.bootstrap_mut().caps_mut().get(3).unwrap().kind,
        CapabilityKind::Vmo { .. }
    ));
    assert!(matches!(
        ctx.tasks.bootstrap_mut().caps_mut().get(new_slot as usize).unwrap().kind,
        CapabilityKind::Vmo { .. }
    ));
}

#[test]
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
fn cap_transfer_rejects_endpoint_factory_distribution_from_non_bootstrap() {
    let mut scheduler = Scheduler::new();
    let mut tasks = TaskTable::new();
    {
        let caps = tasks.bootstrap_mut().caps_mut();
        caps.set(
            0,
            Capability { kind: CapabilityKind::Endpoint(0), rights: Rights::SEND | Rights::RECV },
        )
        .unwrap();
        // Bootstrap holds the endpoint factory in slot 2.
        caps.set(2, Capability { kind: CapabilityKind::EndpointFactory, rights: Rights::MANAGE })
            .unwrap();
    }
    let mut router = ipc::Router::new(8);
    let mut as_manager = AddressSpaceManager::new();
    let timer = MockTimer::default();
    let mut ctx = Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);
    let mut table = SyscallTable::new();
    install_handlers(&mut table);

    // Spawn pid 1 (init-lite stand-in).
    let pid1 = table
        .dispatch(SYSCALL_SPAWN, &mut ctx, &Args::new([0x1000, 0, 0, 0, 0, 0]))
        .map(|pid| task::Pid::from_raw(pid as u32))
        .unwrap();
    assert_eq!(pid1, task::Pid::from_raw(1));

    // PID 0 -> PID 1 transfer is allowed (bootstrap distribution).
    let factory_slot_pid1 = table
        .dispatch(
            SYSCALL_CAP_TRANSFER,
            &mut ctx,
            &Args::new([pid1.as_index(), 2, Rights::MANAGE.bits() as usize, 0, 0, 0]),
        )
        .unwrap();
    assert_eq!(factory_slot_pid1, 1);

    // Switch to pid 1 and spawn its child pid 2.
    ctx.tasks.set_current(pid1);
    let pid2 = table
        .dispatch(SYSCALL_SPAWN, &mut ctx, &Args::new([0x1000, 0, 0, 0, 0, 0]))
        .map(|pid| task::Pid::from_raw(pid as u32))
        .unwrap();
    assert_eq!(pid2, task::Pid::from_raw(2));

    // PID 1 must NOT be able to distribute EndpointFactory further.
    let err = table
        .dispatch(
            SYSCALL_CAP_TRANSFER,
            &mut ctx,
            &Args::new([
                pid2.as_index(),
                factory_slot_pid1,
                Rights::MANAGE.bits() as usize,
                0,
                0,
                0,
            ]),
        )
        .unwrap_err();
    assert_eq!(err, Error::Transfer(task::TransferError::Capability(CapError::PermissionDenied)));
}

#[test]
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
fn ipc_endpoint_create_for_denies_non_parent_owner() {
    let mut scheduler = Scheduler::new();
    let mut tasks = TaskTable::new();
    {
        let caps = tasks.bootstrap_mut().caps_mut();
        // Seed bootstrap endpoint for spawn syscall.
        caps.set(
            0,
            Capability { kind: CapabilityKind::Endpoint(0), rights: Rights::SEND | Rights::RECV },
        )
        .unwrap();
        // Seed endpoint factory in bootstrap (slot 2) so it can be transferred to pid1.
        caps.set(2, Capability { kind: CapabilityKind::EndpointFactory, rights: Rights::MANAGE })
            .unwrap();
    }
    let mut router = ipc::Router::new(8);
    let mut as_manager = AddressSpaceManager::new();
    let timer = MockTimer::default();
    let mut ctx = Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);
    let mut table = SyscallTable::new();
    install_handlers(&mut table);

    // Spawn pid 1 (init-lite stand-in) and switch to it.
    let pid1 = table
        .dispatch(SYSCALL_SPAWN, &mut ctx, &Args::new([0x1000, 0, 0, 0, 0, 0]))
        .map(|pid| task::Pid::from_raw(pid as u32))
        .unwrap();
    assert_eq!(pid1, task::Pid::from_raw(1));
    ctx.tasks.set_current(pid1);

    // Transfer EndpointFactory into pid1 slot 1.
    let factory_slot = table
        .dispatch(
            SYSCALL_CAP_TRANSFER,
            &mut ctx,
            &Args::new([pid1.as_index(), 2, Rights::MANAGE.bits() as usize, 0, 0, 0]),
        )
        .unwrap();
    assert_eq!(factory_slot, 1);

    // Spawn pid 2 (child of pid1) and pid 3 (also child of pid1).
    let pid2 = table
        .dispatch(SYSCALL_SPAWN, &mut ctx, &Args::new([0x1000, 0, 0, 0, 0, 0]))
        .map(|pid| task::Pid::from_raw(pid as u32))
        .unwrap();
    let pid3 = table
        .dispatch(SYSCALL_SPAWN, &mut ctx, &Args::new([0x1000, 0, 0, 0, 0, 0]))
        .map(|pid| task::Pid::from_raw(pid as u32))
        .unwrap();
    assert_eq!(pid2, task::Pid::from_raw(2));
    assert_eq!(pid3, task::Pid::from_raw(3));

    // Switch to pid2 and attempt to create an endpoint owned by pid3.
    // Denied because pid2 is not the parent of pid3 (both are siblings under pid1).
    ctx.tasks.set_current(pid2);
    // Give pid2 the factory (init-lite would normally hold it; for test we transfer to pid2).
    let factory_slot_pid2 = table
        .dispatch(
            SYSCALL_CAP_TRANSFER,
            &mut ctx,
            &Args::new([pid2.as_index(), factory_slot, Rights::MANAGE.bits() as usize, 0, 0, 0]),
        )
        .unwrap();
    assert_ne!(factory_slot_pid2, 0);

    let err = table
        .dispatch(
            crate::syscall::SYSCALL_IPC_ENDPOINT_CREATE_FOR,
            &mut ctx,
            &Args::new([factory_slot_pid2, pid3.as_index(), 8, 0, 0, 0]),
        )
        .unwrap_err();
    assert_eq!(err, Error::Capability(CapError::PermissionDenied));
}

#[test]
#[cfg(all(target_arch = "riscv64", target_os = "none"))]
fn endpoint_create_is_init_lite_only() {
    let mut scheduler = Scheduler::new();
    let mut tasks = TaskTable::new();
    {
        let caps = tasks.bootstrap_mut().caps_mut();
        caps.set(
            0,
            Capability { kind: CapabilityKind::Endpoint(0), rights: Rights::SEND | Rights::RECV },
        )
        .unwrap();
    }
    let mut router = ipc::Router::new(1);
    let mut as_manager = AddressSpaceManager::new();
    let timer = MockTimer::default();
    let mut ctx = Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);
    let mut table = SyscallTable::new();
    install_handlers(&mut table);

    // Spawn pid 1 (init-lite stand-in) and switch to it.
    let pid1 = table
        .dispatch(SYSCALL_SPAWN, &mut ctx, &Args::new([0x1000, 0, 0, 0, 0, 0]))
        .map(|pid| task::Pid::from_raw(pid as u32))
        .unwrap();
    assert_eq!(pid1, task::Pid::from_raw(1));
    ctx.tasks.set_current(pid1);

    // pid 1 may create endpoints.
    let slot = table
        .dispatch(SYSCALL_IPC_ENDPOINT_CREATE, &mut ctx, &Args::new([8, 0, 0, 0, 0, 0]))
        .unwrap();
    assert_ne!(slot, 0);

    // Spawn pid 2 (regular service stand-in, child of init-lite) and switch to it.
    let pid2 = table
        .dispatch(SYSCALL_SPAWN, &mut ctx, &Args::new([0x1000, 0, 0, 0, 0, 0]))
        .map(|pid| task::Pid::from_raw(pid as u32))
        .unwrap();
    assert_eq!(pid2, task::Pid::from_raw(2));
    ctx.tasks.set_current(pid2);

    // pid 2 is userspace too, but must be denied by the endpoint-factory gate.
    let err = table
        .dispatch(SYSCALL_IPC_ENDPOINT_CREATE, &mut ctx, &Args::new([8, 0, 0, 0, 0, 0]))
        .unwrap_err();
    assert_eq!(err, Error::Capability(CapError::PermissionDenied));
}

#[test]
fn qos_self_set_downward_and_get_ok() {
    let mut scheduler = Scheduler::new();
    let mut tasks = TaskTable::new();
    let mut router = ipc::Router::new(0);
    let mut as_manager = AddressSpaceManager::new();
    let timer = MockTimer::default();
    let mut ctx = Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);

    let self_pid = ctx.tasks.current_pid();
    let set_args =
        Args::new([TASK_QOS_OP_SET, self_pid.as_index(), QosClass::Normal as usize, 0, 0, 0]);
    assert_eq!(sys_task_qos(&mut ctx, &set_args).unwrap(), 0);
    assert_eq!(ctx.tasks.current_task().qos(), QosClass::Normal);

    let get_args = Args::new([TASK_QOS_OP_GET_SELF, 0, 0, 0, 0, 0]);
    let got = sys_task_qos(&mut ctx, &get_args).unwrap();
    assert_eq!(got, QosClass::Normal as usize);
}

#[test]
fn test_reject_qos_set_unauthorized_self_escalation() {
    let mut scheduler = Scheduler::new();
    let mut tasks = TaskTable::new();
    let mut router = ipc::Router::new(0);
    let mut as_manager = AddressSpaceManager::new();
    let timer = MockTimer::default();
    let mut ctx = Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);

    let self_pid = ctx.tasks.current_pid();
    let down =
        Args::new([TASK_QOS_OP_SET, self_pid.as_index(), QosClass::Normal as usize, 0, 0, 0]);
    assert_eq!(sys_task_qos(&mut ctx, &down).unwrap(), 0);
    assert_eq!(ctx.tasks.current_task().qos(), QosClass::Normal);

    let up =
        Args::new([TASK_QOS_OP_SET, self_pid.as_index(), QosClass::Interactive as usize, 0, 0, 0]);
    let err = sys_task_qos(&mut ctx, &up).unwrap_err();
    assert_eq!(err, Error::Capability(CapError::PermissionDenied));
    assert_eq!(ctx.tasks.current_task().qos(), QosClass::Normal);
}

#[test]
fn test_reject_qos_set_unauthorized_other_pid_even_with_factory_cap() {
    let mut scheduler = Scheduler::new();
    let mut tasks = TaskTable::new();
    let mut router = ipc::Router::new(0);
    let mut as_manager = AddressSpaceManager::new();
    let timer = MockTimer::default();
    let target = tasks.selftest_create_dummy_task(task::Pid::KERNEL, &mut scheduler);
    tasks
        .bootstrap_mut()
        .caps_mut()
        .set(1, Capability { kind: CapabilityKind::EndpointFactory, rights: Rights::MANAGE })
        .unwrap();
    let mut ctx = Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);

    let args = Args::new([TASK_QOS_OP_SET, target.as_index(), QosClass::Idle as usize, 0, 0, 0]);
    let err = sys_task_qos(&mut ctx, &args).unwrap_err();
    assert_eq!(err, Error::Capability(CapError::PermissionDenied));
}

#[test]
fn test_reject_invalid_qos_class() {
    let mut scheduler = Scheduler::new();
    let mut tasks = TaskTable::new();
    let mut router = ipc::Router::new(0);
    let mut as_manager = AddressSpaceManager::new();
    let timer = MockTimer::default();
    let mut ctx = Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);

    let self_pid = ctx.tasks.current_pid();
    let args = Args::new([TASK_QOS_OP_SET, self_pid.as_index(), 0xFF, 0, 0, 0]);
    let err = sys_task_qos(&mut ctx, &args).unwrap_err();
    assert_eq!(err, Error::AddressSpace(AddressSpaceError::InvalidArgs));
}

#[test]
fn test_reject_qos_target_pid_overflow() {
    let mut scheduler = Scheduler::new();
    let mut tasks = TaskTable::new();
    let mut router = ipc::Router::new(0);
    let mut as_manager = AddressSpaceManager::new();
    let timer = MockTimer::default();
    let mut ctx = Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);

    let args = Args::new([TASK_QOS_OP_SET, usize::MAX, QosClass::Normal as usize, 0, 0, 0]);
    let err = sys_task_qos(&mut ctx, &args).unwrap_err();
    assert_eq!(err, Error::AddressSpace(AddressSpaceError::InvalidArgs));
}

#[test]
fn test_reject_qos_class_wire_overflow() {
    let mut scheduler = Scheduler::new();
    let mut tasks = TaskTable::new();
    let mut router = ipc::Router::new(0);
    let mut as_manager = AddressSpaceManager::new();
    let timer = MockTimer::default();
    let mut ctx = Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);

    let self_pid = ctx.tasks.current_pid();
    let args = Args::new([TASK_QOS_OP_SET, self_pid.as_index(), usize::MAX, 0, 0, 0]);
    let err = sys_task_qos(&mut ctx, &args).unwrap_err();
    assert_eq!(err, Error::AddressSpace(AddressSpaceError::InvalidArgs));
}

#[test]
fn qos_set_other_pid_via_privileged_path_ok() {
    let mut scheduler = Scheduler::new();
    let mut tasks = TaskTable::new();
    let mut router = ipc::Router::new(0);
    let mut as_manager = AddressSpaceManager::new();
    let timer = MockTimer::default();
    let target = tasks.selftest_create_dummy_task(task::Pid::KERNEL, &mut scheduler);
    let mut ctx = Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);
    ctx.tasks.current_task_mut().set_service_id(service_id_from_name(b"execd"));

    let args =
        Args::new([TASK_QOS_OP_SET, target.as_index(), QosClass::Interactive as usize, 0, 0, 0]);
    assert_eq!(sys_task_qos(&mut ctx, &args).unwrap(), 0);
    assert_eq!(ctx.tasks.task(target).unwrap().qos(), QosClass::Interactive);
}

#[test]
fn qos_self_escalation_via_policyd_path_ok() {
    let mut scheduler = Scheduler::new();
    let mut tasks = TaskTable::new();
    let mut router = ipc::Router::new(0);
    let mut as_manager = AddressSpaceManager::new();
    let timer = MockTimer::default();
    let mut ctx = Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);
    ctx.tasks.current_task_mut().set_service_id(service_id_from_name(b"policyd"));

    let self_pid = ctx.tasks.current_pid();
    let down =
        Args::new([TASK_QOS_OP_SET, self_pid.as_index(), QosClass::Normal as usize, 0, 0, 0]);
    assert_eq!(sys_task_qos(&mut ctx, &down).unwrap(), 0);
    assert_eq!(ctx.tasks.current_task().qos(), QosClass::Normal);

    let up =
        Args::new([TASK_QOS_OP_SET, self_pid.as_index(), QosClass::Interactive as usize, 0, 0, 0]);
    assert_eq!(sys_task_qos(&mut ctx, &up).unwrap(), 0);
    assert_eq!(ctx.tasks.current_task().qos(), QosClass::Interactive);
}

// ==========================================================================
// MMIO capability negative tests (security floor for device access model)
// ==========================================================================

/// Test that mapping without a capability in the slot is rejected.
/// Security invariant: MMIO access must be capability-gated.
#[test]
fn test_reject_mmio_no_cap() {
    use super::SYSCALL_MMIO_MAP;

    let mut scheduler = Scheduler::new();
    let mut tasks = TaskTable::new();
    let mut router = ipc::Router::new(0);
    let mut as_manager = AddressSpaceManager::new();
    let timer = MockTimer::default();

    // Slot 48 is empty (no capability).
    // The task has an address space but no MMIO capability.
    let kernel_as = as_manager.create().unwrap();
    as_manager.attach(kernel_as, task::Pid::KERNEL).unwrap();
    tasks.bootstrap_mut().address_space = Some(kernel_as);

    let mut ctx = Context::new(&mut scheduler, &mut tasks, &mut router, &mut as_manager, &timer);
    let mut table = SyscallTable::new();
    install_handlers(&mut table);

    // Attempt to map via empty slot 48.
    let args = Args::new([
        48,          // slot (empty)
        0x2000_0000, // va (page-aligned)
        0,           // offset
        0,
        0,
        0,
    ]);
    let err = table.dispatch(SYSCALL_MMIO_MAP, &mut ctx, &args).unwrap_err();
    assert_eq!(err, Error::Capability(CapError::InvalidSlot));
}

/// Test that mapping with the wrong capability kind (Endpoint instead of DeviceMmio) is rejected.
/// Security invariant: Only DeviceMmio capabilities can be used for MMIO mapping.
#[test]
fn test_reject_mmio_wrong_cap_kind() {
    use super::SYSCALL_MMIO_MAP;

    let mut scheduler = Scheduler::new();
    let mut tasks = TaskTable::new();
    let mut router = ipc::Router::new(0);
    let mut as_manager = AddressSpaceManager::new();
    let timer = MockTimer::default();

    // Set up an Endpoint capability in slot 48 (wrong kind for MMIO).
    {
        let caps = tasks.bootstrap_mut().caps_mut();
        caps.set(
            48,
            Capability {
                kind: CapabilityKind::Endpoint(0),
                rights: Rights::MAP | Rights::SEND | Rights::RECV,
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

    // Attempt to map via slot 48 (Endpoint, not DeviceMmio).
    let args = Args::new([
        48,          // slot (has Endpoint, not DeviceMmio)
        0x2000_0000, // va (page-aligned)
        0,           // offset
        0,
        0,
        0,
    ]);
    let err = table.dispatch(SYSCALL_MMIO_MAP, &mut ctx, &args).unwrap_err();
    assert_eq!(err, Error::Capability(CapError::PermissionDenied));
}

/// Test that mapping beyond the device window bounds is rejected.
/// Security invariant: MMIO mappings must be bounded to the device window.
#[test]
fn test_reject_mmio_outside_window() {
    use super::SYSCALL_MMIO_MAP;

    let mut scheduler = Scheduler::new();
    let mut tasks = TaskTable::new();
    let mut router = ipc::Router::new(0);
    let mut as_manager = AddressSpaceManager::new();
    let timer = MockTimer::default();

    // Set up a DeviceMmio capability with a small window (2 pages = 0x2000 bytes).
    const MMIO_BASE: usize = 0x1000_0000;
    const MMIO_LEN: usize = 0x2000; // 2 pages
    {
        let caps = tasks.bootstrap_mut().caps_mut();
        caps.set(
            48,
            Capability {
                kind: CapabilityKind::DeviceMmio { base: MMIO_BASE, len: MMIO_LEN },
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

    // Attempt to map at offset 0x2000 (equals len, therefore out of bounds).
    let args = Args::new([
        48,          // slot (DeviceMmio)
        0x2000_0000, // va (page-aligned)
        MMIO_LEN,    // offset = len (out of bounds)
        0,
        0,
        0,
    ]);
    let err = table.dispatch(SYSCALL_MMIO_MAP, &mut ctx, &args).unwrap_err();
    assert_eq!(err, Error::Capability(CapError::PermissionDenied));

    // Also test offset way beyond the window.
    let args_far = Args::new([
        48,          // slot
        0x2000_0000, // va
        0x1_0000,    // offset = 64KiB (way beyond 8KiB window)
        0,
        0,
        0,
    ]);
    let err_far = table.dispatch(SYSCALL_MMIO_MAP, &mut ctx, &args_far).unwrap_err();
    assert_eq!(err_far, Error::Capability(CapError::PermissionDenied));
}

/// Test that mapping without MAP rights is rejected.
/// Security invariant: MMIO mapping requires Rights::MAP.
#[test]
fn test_reject_mmio_insufficient_rights() {
    use super::SYSCALL_MMIO_MAP;

    let mut scheduler = Scheduler::new();
    let mut tasks = TaskTable::new();
    let mut router = ipc::Router::new(0);
    let mut as_manager = AddressSpaceManager::new();
    let timer = MockTimer::default();

    // Set up a DeviceMmio capability WITHOUT Rights::MAP (only SEND, which is meaningless).
    const MMIO_BASE: usize = 0x1000_0000;
    const MMIO_LEN: usize = 0x2000;
    {
        let caps = tasks.bootstrap_mut().caps_mut();
        caps.set(
            48,
            Capability {
                kind: CapabilityKind::DeviceMmio { base: MMIO_BASE, len: MMIO_LEN },
                rights: Rights::SEND, // Wrong rights for MMIO
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

    // Attempt to map with insufficient rights.
    let args = Args::new([
        48,          // slot
        0x2000_0000, // va
        0,           // offset (valid)
        0,
        0,
        0,
    ]);
    let err = table.dispatch(SYSCALL_MMIO_MAP, &mut ctx, &args).unwrap_err();
    assert_eq!(err, Error::Capability(CapError::PermissionDenied));
}

/// Test that executable MMIO mappings are rejected (no EXEC in leaf flags).
/// Security invariant: device MMIO is USER|RW only (never executable).
#[test]
fn test_reject_mmio_exec() {
    use super::SYSCALL_MMIO_MAP;
    use crate::mm::page_table::PageFlags;

    let mut scheduler = Scheduler::new();
    let mut tasks = TaskTable::new();
    let mut router = ipc::Router::new(0);
    let mut as_manager = AddressSpaceManager::new();
    let timer = MockTimer::default();

    const MMIO_BASE: usize = 0x1000_0000;
    const MMIO_LEN: usize = 0x1000;
    const MMIO_VA: usize = 0x2000_0000;
    {
        let caps = tasks.bootstrap_mut().caps_mut();
        caps.set(
            48,
            Capability {
                kind: CapabilityKind::DeviceMmio { base: MMIO_BASE, len: MMIO_LEN },
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

    let args = Args::new([48, MMIO_VA, 0, 0, 0, 0]);
    table.dispatch(SYSCALL_MMIO_MAP, &mut ctx, &args).unwrap();

    let handle = ctx.tasks.current_task().address_space().unwrap();
    let flags = ctx.address_spaces.get(handle).unwrap().page_table().leaf_flags(MMIO_VA).unwrap();
    assert!(flags.contains(PageFlags::USER));
    assert!(flags.contains(PageFlags::READ));
    assert!(flags.contains(PageFlags::WRITE));
    assert!(!flags.contains(PageFlags::EXECUTE));
}

/// Test that non-page-aligned virtual addresses are rejected.
#[test]
fn test_reject_mmio_unaligned_va() {
    use super::SYSCALL_MMIO_MAP;
    use crate::mm::address_space::AddressSpaceError;

    let mut scheduler = Scheduler::new();
    let mut tasks = TaskTable::new();
    let mut router = ipc::Router::new(0);
    let mut as_manager = AddressSpaceManager::new();
    let timer = MockTimer::default();

    const MMIO_BASE: usize = 0x1000_0000;
    const MMIO_LEN: usize = 0x1000;
    {
        let caps = tasks.bootstrap_mut().caps_mut();
        caps.set(
            48,
            Capability {
                kind: CapabilityKind::DeviceMmio { base: MMIO_BASE, len: MMIO_LEN },
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

    let args = Args::new([48, 0x2000_0001, 0, 0, 0, 0]); // va not page-aligned
    let err = table.dispatch(SYSCALL_MMIO_MAP, &mut ctx, &args).unwrap_err();
    assert_eq!(err, Error::AddressSpace(AddressSpaceError::InvalidArgs));
}

/// Test that non-page-aligned offsets are rejected.
#[test]
fn test_reject_mmio_unaligned_offset() {
    use super::SYSCALL_MMIO_MAP;

    let mut scheduler = Scheduler::new();
    let mut tasks = TaskTable::new();
    let mut router = ipc::Router::new(0);
    let mut as_manager = AddressSpaceManager::new();
    let timer = MockTimer::default();

    const MMIO_BASE: usize = 0x1000_0000;
    const MMIO_LEN: usize = 0x2000;
    {
        let caps = tasks.bootstrap_mut().caps_mut();
        caps.set(
            48,
            Capability {
                kind: CapabilityKind::DeviceMmio { base: MMIO_BASE, len: MMIO_LEN },
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

    let args = Args::new([48, 0x2000_0000, 1, 0, 0, 0]); // offset not page-aligned
    let err = table.dispatch(SYSCALL_MMIO_MAP, &mut ctx, &args).unwrap_err();
    assert_eq!(err, Error::Capability(CapError::PermissionDenied));
}

/// Test that remapping the same VA deterministically fails (no silent overwrite).
#[test]
fn test_reject_mmio_overlap_same_va() {
    use super::SYSCALL_MMIO_MAP;
    use crate::mm::{address_space::AddressSpaceError, page_table::MapError};

    let mut scheduler = Scheduler::new();
    let mut tasks = TaskTable::new();
    let mut router = ipc::Router::new(0);
    let mut as_manager = AddressSpaceManager::new();
    let timer = MockTimer::default();

    const MMIO_BASE: usize = 0x1000_0000;
    const MMIO_LEN: usize = 0x2000;
    const MMIO_VA: usize = 0x2000_0000;
    {
        let caps = tasks.bootstrap_mut().caps_mut();
        caps.set(
            48,
            Capability {
                kind: CapabilityKind::DeviceMmio { base: MMIO_BASE, len: MMIO_LEN },
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

    let args = Args::new([48, MMIO_VA, 0, 0, 0, 0]);
    table.dispatch(SYSCALL_MMIO_MAP, &mut ctx, &args).unwrap();

    // Second map to the same VA must fail (overlap).
    let err = table.dispatch(SYSCALL_MMIO_MAP, &mut ctx, &args).unwrap_err();
    assert_eq!(err, Error::AddressSpace(AddressSpaceError::Mapping(MapError::Overlap)));
}
