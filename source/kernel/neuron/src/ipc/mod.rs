// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Kernel-side IPC primitives (endpoints, router)
//! OWNERS: @kernel-ipc-team
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: host (`tests::*`: the accounting census over 2000 random operations, waiter
//!   order, close); QEMU selftests + boot markers
//! PUBLIC API: Router (send/recv), Message, EndpointId
//! DEPENDS_ON: ipc::header::MessageHeader
//! INVARIANTS: Header.len bounds payload; queue depth respected; no cross-layer deps
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::marker::PhantomData;
#[cfg(feature = "failpoints")]
use core::sync::atomic::AtomicBool;
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::sync::block_table::BlockTable;
use crate::sync::lock_rank::Class;
use crate::sync::ranked::RankedLock;

pub mod header;
#[cfg(feature = "ipc_trace_ring")]
pub mod trace;

/// Identifier for a kernel endpoint — what an endpoint capability holds.
pub use crate::cap::EndpointId;

/// Waiter identifier stored in endpoint wait queues.
///
/// In practice this is a userspace task PID (`task::Pid`), but we keep IPC primitives
/// independent from the task table by using a plain integer here.
pub type WaiterId = u32;

/// Error returned by router operations.
#[must_use = "IPC errors must be handled explicitly"]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpcError {
    /// Target endpoint does not exist.
    NoSuchEndpoint,
    /// Queue is full.
    QueueFull,
    /// Queue is empty.
    QueueEmpty,
    /// Permission denied for the requested operation.
    PermissionDenied,
    /// Blocking IPC operation hit its deadline.
    TimedOut,
    /// Not enough resources to complete the IPC operation (e.g. receiver cap table full).
    NoSpace,
    /// The payload is larger than [`IPC_PAYLOAD_MAX`] (`E2BIG`). Distinct from
    /// `NoSpace`, which is the RECEIVER's queue budget: this one cannot succeed
    /// by retrying, and the answer is the VMO bulk path (RFC-0096).
    TooBig,
    /// RFC-0079: an EOF-opted receiver's endpoint had a sender and now has none
    /// (the last SEND cap closed). The endpoint is still alive; there will just
    /// never be another message unless a new sender attaches.
    PeerClosed,
    /// ADR-0082: the receiver is frozen and the send would park until a thaw (full queue, or a
    /// call that waits for its answer). `EDEADLK`.
    Frozen,
}

pub use crate::ipc_payload::{Payload, IPC_PAYLOAD_MAX};

mod endpoint; // the queue
mod eof; // RFC-0079 last-sender EOF accessors (`had_sender` latch, owner, recv-waiter drain)
mod lifetime; // TASK-0333 6a: an endpoint dies with its last capability; its slot is reused
mod table; // TASK-0330 P2: the endpoints at fixed addresses, each under its own lock

/// The kernel IPC message (header + tiered payload + moved cap).
mod message;
pub use message::Message;

/// Router managing all kernel endpoints.
pub struct Router {
    /// Every endpoint ever created, each behind its own lock at a fixed address (ADR-0102).
    slots: BlockTable<table::Cell>,
    /// The registry lock: creation and reuse of a slot, and the owners' byte counters.
    owners: RankedLock<table::Owners>,
    queued_bytes_total: AtomicUsize,
    max_queued_bytes_total: usize,
    max_queued_bytes_per_owner: usize,
    /// RFC-0112: caps in flight in closed endpoints, each released like a `cap_close`.
    in_flight_dropped: RankedLock<Vec<crate::cap::Held>>,
    // Until the IPC syscalls leave the kernel lock (TASK-0330 P2) the router is reached
    // through the kernel guard only.
    _not_send_sync: PhantomData<*mut ()>,
}
static_assertions::assert_not_impl_any!(Router: Send, Sync);

// A DoS valve per owner until the per-app budget (TASK-0352 P3) — not the table's size: the
// table grows with the endpoints, bounded by the pool (ADR-0093; 384 bit at the 29th app).
const MAX_ENDPOINTS_PER_OWNER: usize = 1024;

#[cfg(feature = "failpoints")]
static DENY_NEXT_SEND: AtomicBool = AtomicBool::new(false);

impl Router {
    /// Creates a router with space for `count` endpoints.
    pub fn new(count: usize) -> Self {
        // Queued payload bytes are bounded across all endpoints (above boot traffic; the
        // per-endpoint budgets still apply) and per owning service, so one service's
        // inboxes cannot take the whole global budget.
        const DEFAULT_MAX_QUEUED_BYTES_TOTAL: usize = 1024 * 1024; // 1 MiB
        const DEFAULT_MAX_QUEUED_BYTES_PER_OWNER: usize = 256 * 1024; // 256 KiB
        let router = Self {
            slots: table::new_slots(),
            owners: RankedLock::new(Class::Registry, BTreeMap::new()),
            queued_bytes_total: AtomicUsize::new(0),
            max_queued_bytes_total: DEFAULT_MAX_QUEUED_BYTES_TOTAL,
            max_queued_bytes_per_owner: DEFAULT_MAX_QUEUED_BYTES_PER_OWNER,
            in_flight_dropped: RankedLock::new(Class::Leaf, Vec::new()),
            _not_send_sync: PhantomData,
        };
        for _ in 0..count {
            // A header the heap refuses ends the first endpoints there (ADR-0091).
            if router.create_endpoint(8, None).is_err() {
                break;
            }
        }
        router
    }

    /// Creates a router with an explicit global queued-bytes budget (used by selftests).
    pub fn new_with_global_bytes_budget(count: usize, max_queued_bytes_total: usize) -> Self {
        let mut out = Self::new(count);
        out.max_queued_bytes_total = max_queued_bytes_total;
        out
    }

    /// Creates a router with explicit global and per-owner queued-bytes budgets (used by selftests).
    pub fn new_with_bytes_budgets(
        count: usize,
        max_queued_bytes_total: usize,
        max_queued_bytes_per_owner: usize,
    ) -> Self {
        let mut out = Self::new_with_global_bytes_budget(count, max_queued_bytes_total);
        out.max_queued_bytes_per_owner = max_queued_bytes_per_owner;
        out
    }

    /// Sends `msg` to the endpoint referenced by `id`.
    pub fn send_returning_message(
        &self,
        id: EndpointId,
        msg: Message,
    ) -> core::result::Result<(), (IpcError, Message)> {
        #[cfg(feature = "debug_uart")]
        log_debug!(target: "ipc", "send target={} len={}", id, msg.header.len);
        #[cfg(feature = "failpoints")]
        if DENY_NEXT_SEND.swap(false, Ordering::SeqCst) {
            return Err((IpcError::PermissionDenied, msg));
        }
        // One hold of the endpoint: the budgets, the queue and the latches change together.
        let Some(mut ep) = self.ep(id) else {
            // The global budget is judged before the endpoint is looked at, as it always was.
            let len = msg.payload.len();
            let spent = self.queued_bytes_total.load(Ordering::Acquire).saturating_add(len)
                > self.max_queued_bytes_total;
            return Err((if spent { IpcError::NoSpace } else { IpcError::NoSuchEndpoint }, msg));
        };
        // A live sender: whatever EOF the last scan latched is stale (TASK-0324 P7-d).
        ep.eof_pending = false;
        let msg_len = msg.payload.len();
        if !self.reserve(&ep, msg_len) {
            return Err((IpcError::NoSpace, msg));
        }
        let res = ep.push(msg);
        match res {
            // RFC-0079: a successful send proves a sender exists — latch it so a later
            // EOF-opted recv can tell "never had a sender" (block) from "had one, now gone"
            // (EOF).
            Ok(()) => ep.had_sender = true,
            Err(_) => self.release(&ep, msg_len),
        }
        #[cfg(feature = "debug_uart")]
        log_debug!(target: "ipc", "send result={:?}", res.as_ref().map_err(|(error, _)| *error));
        res
    }

    /// Sends `msg` to the endpoint referenced by `id` and discards the message on error.
    pub fn send(&self, id: EndpointId, msg: Message) -> Result<(), IpcError> {
        self.send_returning_message(id, msg).map_err(|(e, _msg)| e)
    }

    /// Receives the next message from the endpoint `id`.
    pub fn recv(&self, id: EndpointId) -> Result<Message, IpcError> {
        let mut ep = self.ep(id).ok_or(IpcError::NoSuchEndpoint)?;
        let res = ep.pop();
        if let Ok(ref msg) = res {
            self.release(&ep, msg.payload.len());
        }
        #[cfg(feature = "debug_uart")]
        log_debug!(target: "ipc", "recv ep={} result={:?}", id, res.as_ref().map(|_| ()));
        res
    }

    /// Re-queues `msg` at the front of endpoint `id`.
    pub fn requeue_front(&self, id: EndpointId, msg: Message) -> Result<(), IpcError> {
        let msg_len = msg.payload.len();
        let Some(mut ep) = self.ep(id) else {
            let spent = self.queued_bytes_total.load(Ordering::Acquire).saturating_add(msg_len)
                > self.max_queued_bytes_total;
            return Err(if spent { IpcError::NoSpace } else { IpcError::NoSuchEndpoint });
        };
        if !self.reserve(&ep, msg_len) {
            return Err(IpcError::NoSpace);
        }
        let res = ep.push_front(msg);
        if res.is_err() {
            self.release(&ep, msg_len);
        }
        res
    }

    /// Registers `pid` as a waiter for `recv` on endpoint `id` (queue empty, blocking).
    pub fn register_recv_waiter(&self, id: EndpointId, pid: WaiterId) -> Result<(), IpcError> {
        let mut ep = self.ep(id).ok_or(IpcError::NoSuchEndpoint)?;
        if !ep.alive {
            return Err(IpcError::NoSuchEndpoint);
        }
        ep.register_recv_waiter(pid);
        Ok(())
    }

    /// Registers `pid` as a waiter for `send` on endpoint `id` (queue full, blocking).
    pub fn register_send_waiter(&self, id: EndpointId, pid: WaiterId) -> Result<(), IpcError> {
        let mut ep = self.ep(id).ok_or(IpcError::NoSuchEndpoint)?;
        if !ep.alive {
            return Err(IpcError::NoSuchEndpoint);
        }
        ep.register_send_waiter(pid);
        Ok(())
    }

    /// Pops one waiter for `recv` on endpoint `id`.
    pub fn pop_recv_waiter(&self, id: EndpointId) -> Result<Option<WaiterId>, IpcError> {
        let mut ep = self.ep(id).ok_or(IpcError::NoSuchEndpoint)?;
        if !ep.alive {
            return Err(IpcError::NoSuchEndpoint);
        }
        Ok(ep.pop_recv_waiter())
    }

    /// Pops one waiter for `send` on endpoint `id`.
    pub fn pop_send_waiter(&self, id: EndpointId) -> Result<Option<WaiterId>, IpcError> {
        let mut ep = self.ep(id).ok_or(IpcError::NoSuchEndpoint)?;
        if !ep.alive {
            return Err(IpcError::NoSuchEndpoint);
        }
        Ok(ep.pop_send_waiter())
    }

    /// Removes `pid` from the recv waiter list, if present.
    pub fn remove_recv_waiter(&self, id: EndpointId, pid: WaiterId) -> Result<bool, IpcError> {
        let mut ep = self.ep(id).ok_or(IpcError::NoSuchEndpoint)?;
        if !ep.alive {
            return Err(IpcError::NoSuchEndpoint);
        }
        Ok(ep.remove_recv_waiter(pid))
    }

    /// Removes `pid` from the send waiter list, if present.
    pub fn remove_send_waiter(&self, id: EndpointId, pid: WaiterId) -> Result<bool, IpcError> {
        let mut ep = self.ep(id).ok_or(IpcError::NoSuchEndpoint)?;
        if !ep.alive {
            return Err(IpcError::NoSuchEndpoint);
        }
        Ok(ep.remove_send_waiter(pid))
    }

    /// Returns true if `id` exists and is alive.
    pub fn endpoint_alive(&self, id: EndpointId) -> bool {
        self.ep(id).is_some_and(|ep| ep.alive)
    }

    /// Returns true if `id` exists, is alive, and has at least one queued message.
    ///
    /// Non-consuming readiness probe used by `waitset_wait` (RFC-0033) to scan members
    /// without dequeuing; the caller then `recv`s the ready endpoint normally. A dead or
    /// unknown endpoint reads as not-pending (the waitset deregisters from it on wake).
    pub fn pending(&self, id: EndpointId) -> bool {
        self.ep(id).is_some_and(|ep| ep.alive && !ep.queue.is_empty())
    }

    /// Closes every endpoint owned by `owner` and returns all drained waiter PIDs.
    pub fn close_endpoints_for_owner(&self, owner: WaiterId) -> Vec<WaiterId> {
        let mut out: Vec<WaiterId> = Vec::new();
        let mut dropped = Vec::new();
        {
            // The registry lock across the walk: no endpoint of `owner` is created beside it.
            let mut owners = self.owners.lock();
            self.each(|id, ep| {
                #[cfg(not(feature = "ipc_trace_ring"))]
                let _ = id;
                let queued = ep.queued_bytes;
                if let Some((recv, send)) = ep.close_if_owned_by(owner, &mut dropped) {
                    self.release(ep, queued);
                    #[cfg(feature = "ipc_trace_ring")]
                    crate::ipc::trace::record_ep_close(owner, id as u32);
                    out.extend(recv);
                    out.extend(send);
                }
            });
            // The owner's counter goes with its last endpoint.
            owners.remove(&owner);
        }
        self.retire(dropped);
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Closes a single endpoint unconditionally (used by privileged MANAGE holders).
    pub fn close_endpoint(&self, id: EndpointId) -> Result<Vec<WaiterId>, IpcError> {
        let mut dropped = Vec::new();
        let mut out: Vec<WaiterId> = Vec::new();
        {
            let mut ep = self.ep(id).ok_or(IpcError::NoSuchEndpoint)?;
            if !ep.alive {
                return Err(IpcError::NoSuchEndpoint);
            }
            ep.alive = false;
            self.release(&ep, ep.queued_bytes);
            ep.clear_queue_into(&mut dropped);
            out.extend(ep.recv_waiters.drain(..));
            let ep = &mut *ep;
            out.extend(ep.send_waiters.drain(..).chain(ep.writable_waiters.drain(..)));
        }
        // ADR-0102: what the dead queue carried is released after the endpoint, never under it.
        self.retire(dropped);
        out.sort_unstable();
        out.dedup();
        Ok(out)
    }

    /// Removes `pid` from all endpoint waiter queues (best-effort cleanup on task exit).
    pub fn remove_waiter_from_all(&self, pid: WaiterId) {
        self.each(|_, ep| {
            let _ = ep.remove_recv_waiter(pid);
            let _ = ep.remove_send_waiter(pid);
        });
    }
}

#[cfg(feature = "failpoints")]
pub mod failpoints {
    use super::DENY_NEXT_SEND;
    use core::sync::atomic::Ordering;

    /// Forces the next `send` invocation to error with [`IpcError::PermissionDenied`].
    #[allow(dead_code)]
    pub fn deny_next_send() {
        DENY_NEXT_SEND.store(true, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::header::MessageHeader;
    use super::*;
    use alloc::vec;

    #[test]
    fn loopback_roundtrip() {
        let router = Router::new(2);
        let header = MessageHeader::new(1, 0, 42, 0, 4);
        let payload = vec![1, 2, 3, 4];
        router.send(0, Message::new(header, Payload::from_slice(&payload), None)).unwrap();
        let received = router.recv(0).unwrap();
        assert_eq!(received.header.ty, 42);
        assert_eq!(received.payload.as_slice(), &payload[..]);
        assert!(!received.payload.is_heap(), "4 bytes must ride inline");
    }

    /// TASK-0330 P2: a syscall off the kernel lock holds the header of the endpoint its
    /// capability named. While it does, the dead slot's id stays that endpoint's — it goes to
    /// a new one only once the capability and the pin are both gone.
    #[test]
    fn a_pinned_dead_slot_keeps_its_id() {
        use crate::cap::{CapTable, Rights};
        let router = Router::new(1);
        let ep = router.create_endpoint(4, Some(7)).unwrap();
        let mut table = CapTable::with_capacity(4);
        let slot = table.try_allocate(router.mint(ep, Rights::SEND).unwrap()).map_err(|_| ());
        let slot = slot.unwrap();
        assert!(table.pin_endpoint(slot, Rights::RECV).is_err(), "a right the slot lacks");
        let pin = table.pin_endpoint(slot, Rights::SEND).unwrap();
        assert_eq!(pin.id(), ep);
        let _ = router.close_endpoints_for_owner(7);
        assert_eq!(router.reusable_dead_endpoint(), None, "a capability still names it");
        let _ = table.take(slot).unwrap();
        assert_eq!(router.reusable_dead_endpoint(), None, "a syscall still works on it");
        assert!(router.revive_endpoint(ep, 4, None).is_err());
        drop(pin);
        assert_eq!(router.reusable_dead_endpoint(), Some(ep));
        assert_eq!(router.revive_endpoint(ep, 4, None), Ok(ep));
    }

    /// TASK-0330 P4: a capability dropped off the kernel lock keeps its endpoint's header for
    /// the judgement after the drop (`cap::Dropped`), and a dying task's exit pins what its
    /// table named (`CapTable::endpoint_pins`, each endpoint once): until both are done the
    /// dead slot's id goes to no new endpoint.
    #[test]
    fn test_reject_reuse_of_a_dead_slot_while_its_drop_is_judged() {
        use crate::cap::{CapTable, CapabilityKind, Rights};
        let router = Router::new(2);
        let ep = router.create_endpoint(4, Some(7)).unwrap();
        let other = router.create_endpoint(4, Some(8)).unwrap();
        let mut table = CapTable::with_capacity(4);
        let mut give = |id, rights| table.try_allocate(router.mint(id, rights).unwrap()).ok();
        let first = give(ep, Rights::SEND).unwrap();
        assert!(give(ep, Rights::RECV).is_some() && give(other, Rights::SEND).is_some());
        let mut pins = Vec::new();
        table.endpoint_pins(&mut pins);
        assert_eq!(pins.iter().map(|pin| pin.id()).collect::<Vec<_>>(), [ep, other]);
        let dropped = table.take_held(first).unwrap().dropped();
        assert_eq!(dropped.cap.kind, CapabilityKind::Endpoint(ep));
        let _ = router.close_endpoints_for_owner(7);
        drop(table);
        assert_eq!(router.reusable_dead_endpoint(), None, "the exit's pins hold it");
        drop(pins);
        assert_eq!(router.reusable_dead_endpoint(), None, "the drop's judgement holds it");
        assert!(router.revive_endpoint(ep, 4, None).is_err());
        drop(dropped);
        assert_eq!(router.reusable_dead_endpoint(), Some(ep));
    }

    /// TASK-0324 P7-d: the EOF latch a waitset reads — set by the last-peer scan, cleared by a
    /// send (a live sender) and by the receiver's own observation; never on a dead endpoint.
    #[test]
    fn eof_latch_is_cleared_by_a_send_and_by_the_receiver() {
        let router = Router::new(1);
        let ep = router.create_endpoint(4, Some(7)).unwrap();
        assert!(!router.eof_pending(ep));
        router.set_eof_pending(ep);
        assert!(router.eof_pending(ep));
        let header = MessageHeader::new(1, 0, 1, 0, 0);
        router.send(ep, Message::new(header, Payload::empty(), None)).unwrap();
        assert!(!router.eof_pending(ep), "a sender wrote: the latch is stale");
        router.set_eof_pending(ep);
        router.clear_eof_pending(ep);
        assert!(!router.eof_pending(ep));
        router.set_eof_pending(ep);
        let _ = router.close_endpoints_for_owner(7);
        assert!(!router.eof_pending(ep), "a dead endpoint is never ready");
    }

    #[test]
    fn endpoint_close_drains_waiters_and_disconnects() {
        let router = Router::new(1);
        let ep = router.create_endpoint(4, Some(7)).unwrap();
        router.register_recv_waiter(ep, 100).unwrap();
        router.register_send_waiter(ep, 200).unwrap();
        let drained = router.close_endpoints_for_owner(7);
        assert_eq!(drained, vec![100, 200]);
        let header = MessageHeader::new(1, 0, 1, 0, 0);
        assert_eq!(
            router.send(ep, Message::new(header, Payload::empty(), None)).unwrap_err(),
            IpcError::NoSuchEndpoint
        );
        assert_eq!(router.recv(ep).unwrap_err(), IpcError::NoSuchEndpoint);
    }

    #[test]
    fn recv_waiters_are_fifo_and_deduped() {
        let router = Router::new(1);
        let ep = router.create_endpoint(1, Some(1)).unwrap();
        router.register_recv_waiter(ep, 10).unwrap();
        router.register_recv_waiter(ep, 11).unwrap();
        router.register_recv_waiter(ep, 12).unwrap();
        // Duplicate registration should not change FIFO order.
        router.register_recv_waiter(ep, 11).unwrap();
        assert_eq!(router.pop_recv_waiter(ep).unwrap(), Some(10));
        assert_eq!(router.pop_recv_waiter(ep).unwrap(), Some(11));
        assert_eq!(router.pop_recv_waiter(ep).unwrap(), Some(12));
        assert_eq!(router.pop_recv_waiter(ep).unwrap(), None);
    }

    #[test]
    fn send_waiters_are_fifo_and_deduped() {
        let router = Router::new(1);
        let ep = router.create_endpoint(1, Some(1)).unwrap();
        router.register_send_waiter(ep, 20).unwrap();
        router.register_send_waiter(ep, 21).unwrap();
        router.register_send_waiter(ep, 22).unwrap();
        // Duplicate registration should not change FIFO order.
        router.register_send_waiter(ep, 21).unwrap();
        assert_eq!(router.pop_send_waiter(ep).unwrap(), Some(20));
        assert_eq!(router.pop_send_waiter(ep).unwrap(), Some(21));
        assert_eq!(router.pop_send_waiter(ep).unwrap(), Some(22));
        assert_eq!(router.pop_send_waiter(ep).unwrap(), None);
    }

    #[test]
    fn state_machine_fuzz_router_invariants_deterministic() {
        // Deterministic stress mix (NOT a fuzzer framework):
        // repeatedly mutate router state and assert invariants so accidental regressions show up
        // in host `cargo test`.
        fn next_u64(state: &mut u64) -> u64 {
            // xorshift64*
            let mut x = *state;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            *state = x;
            x.wrapping_mul(0x2545F4914F6CDD1D)
        }

        fn assert_invariants(router: &Router) {
            // Per-endpoint invariants, and the census the router's counters are judged against.
            let mut sum_total = 0usize;
            let mut expected: BTreeMap<WaiterId, usize> = BTreeMap::new();
            router.each(|_, ep| {
                sum_total += ep.queued_bytes;
                if !ep.alive {
                    assert_eq!(ep.queued_bytes, 0, "a dead endpoint holds bytes");
                    return;
                }
                if let Some(owner) = ep.owner {
                    *expected.entry(owner).or_insert(0) += ep.queued_bytes;
                }
                assert!(ep.queue.len() <= ep.depth, "queue depth exceeded");
                let sum: usize = ep.queue.iter().map(|m| m.payload.len()).sum();
                assert_eq!(sum, ep.queued_bytes, "queued_bytes mismatch");
                assert!(ep.queued_bytes <= ep.max_queued_bytes, "endpoint byte budget exceeded");

                // Waiter queues have no duplicates.
                for w in ep.recv_waiters.iter() {
                    assert_eq!(
                        ep.recv_waiters.iter().filter(|x| *x == w).count(),
                        1,
                        "duplicate recv waiter"
                    );
                }
                for w in ep.send_waiters.iter() {
                    assert_eq!(
                        ep.send_waiters.iter().filter(|x| *x == w).count(),
                        1,
                        "duplicate send waiter"
                    );
                }
            });

            // Router accounting invariants: the counters are exact, never recomputed.
            let total = router.queued_bytes_total.load(Ordering::Relaxed);
            assert_eq!(sum_total, total, "global queued_bytes_total mismatch");
            assert!(total <= router.max_queued_bytes_total, "global budget exceeded");

            // Per-owner accounting matches the endpoints (an owner without a counter holds
            // nothing: its counter goes when its endpoints are closed).
            let expected_nz: BTreeMap<WaiterId, usize> =
                expected.into_iter().filter(|(_k, v)| *v != 0).collect();
            let actual_nz: BTreeMap<WaiterId, usize> = router
                .owners
                .lock()
                .iter()
                .map(|(owner, bytes)| (*owner, bytes.load(Ordering::Relaxed)))
                .filter(|(_owner, bytes)| *bytes != 0)
                .collect();
            assert_eq!(expected_nz, actual_nz, "owner queued bytes mismatch");
            for bytes in actual_nz.values() {
                assert!(*bytes <= router.max_queued_bytes_per_owner, "owner budget exceeded");
            }
        }

        let router = Router::new_with_bytes_budgets(0, 4096, 1024);
        let mut seed: u64 = 0x4E58_4950_435F_465Au64; // "NXIPC_FZ"

        // Start with a few endpoints so recv/send operations have targets.
        let owners: [WaiterId; 3] = [1, 7, 42];
        for (i, owner) in owners.iter().enumerate() {
            let _ = router.create_endpoint(4 + i, Some(*owner)).unwrap();
        }

        for step in 0..2_000u32 {
            let r = next_u64(&mut seed);
            let op = (r % 9) as u8;
            let ep_count = router.slot_count().max(1);
            let id = (r as usize % ep_count) as EndpointId;
            let owner = owners[(r as usize >> 8) % owners.len()];

            match op {
                // create endpoint (bounded by MAX_ENDPOINTS quota internally)
                0 => {
                    let depth = 1 + ((r as usize >> 16) % 4);
                    let _ = router.create_endpoint(depth, Some(owner));
                }
                // send small payload
                1 | 2 => {
                    let len = ((r as usize >> 24) % 32) as u32;
                    let hdr = MessageHeader::new(0, id, 1, 0, len);
                    let msg =
                        Message::new(hdr, Payload::from_slice(&vec![0u8; len as usize]), None);
                    let _ = router.send(id, msg);
                }
                // send max payload (512) to exercise budgets
                3 => {
                    let hdr = MessageHeader::new(0, id, 2, 0, 512);
                    let msg = Message::new(hdr, Payload::from_vec(vec![0u8; 512]), None);
                    let _ = router.send(id, msg);
                }
                // recv
                4 => {
                    let _ = router.recv(id);
                }
                // close endpoint
                5 => {
                    let _ = router.close_endpoint(id);
                }
                // close by owner (wakes waiters)
                6 => {
                    let _ = router.close_endpoints_for_owner(owner);
                }
                // register recv waiter
                7 => {
                    let pid: WaiterId = 100 + ((step as u32) % 8);
                    let _ = router.register_recv_waiter(id, pid);
                }
                // register send waiter
                _ => {
                    let pid: WaiterId = 200 + ((step as u32) % 8);
                    let _ = router.register_send_waiter(id, pid);
                }
            }

            assert_invariants(&router);
        }
    }
}
