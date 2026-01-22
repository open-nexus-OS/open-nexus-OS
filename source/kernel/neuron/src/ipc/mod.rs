// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Kernel-side IPC primitives (endpoints, router)
//! OWNERS: @kernel-ipc-team
//! PUBLIC API: Router (send/recv), Message, EndpointId
//! DEPENDS_ON: ipc::header::MessageHeader
//! INVARIANTS: Header.len bounds payload; queue depth respected; no cross-layer deps
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::collections::VecDeque;
use alloc::vec::Vec;

#[cfg(feature = "failpoints")]
use core::sync::atomic::{AtomicBool, Ordering};

pub mod header;
#[cfg(feature = "ipc_trace_ring")]
pub mod trace;

use header::MessageHeader;

/// Identifier for a kernel endpoint.
pub type EndpointId = u32;

/// Waiter identifier stored in endpoint wait queues.
///
/// In practice this is a userspace task PID (`task::Pid`), but we keep IPC primitives
/// independent from the task table by using a plain integer here.
pub type WaiterId = u32;

/// Error returned by router operations.
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
}

/// Representation of an endpoint queue.
#[derive(Default)]
struct Endpoint {
    queue: VecDeque<Message>,
    depth: usize,
    queued_bytes: usize,
    max_queued_bytes: usize,
    owner: Option<WaiterId>,
    alive: bool,
    recv_waiters: VecDeque<WaiterId>,
    send_waiters: VecDeque<WaiterId>,
}

impl Endpoint {
    fn with_depth(depth: usize, owner: Option<WaiterId>) -> Self {
        // Byte-based DoS hardening: in addition to queue depth, cap the total bytes that can be
        // buffered in an endpoint. This keeps memory use bounded even if messages are large.
        //
        // NOTE: Payloads are already bounded at syscall entry (MAX_FRAME_BYTES); this compounds
        // that bound over the queue depth.
        const MAX_FRAME_BYTES: usize = 8 * 1024;
        let max_queued_bytes = depth.saturating_mul(MAX_FRAME_BYTES);
        Self {
            queue: VecDeque::new(),
            depth,
            queued_bytes: 0,
            max_queued_bytes,
            owner,
            alive: true,
            recv_waiters: VecDeque::new(),
            send_waiters: VecDeque::new(),
        }
    }

    fn push(&mut self, msg: Message) -> core::result::Result<(), (IpcError, Message)> {
        if !self.alive {
            return Err((IpcError::NoSuchEndpoint, msg));
        }
        if self.queue.len() >= self.depth {
            return Err((IpcError::QueueFull, msg));
        }
        let len = msg.payload.len();
        if self.queued_bytes.saturating_add(len) > self.max_queued_bytes {
            return Err((IpcError::NoSpace, msg));
        }
        self.queue.push_back(msg);
        self.queued_bytes = self.queued_bytes.saturating_add(len);
        Ok(())
    }

    fn pop(&mut self) -> Result<Message, IpcError> {
        if !self.alive {
            return Err(IpcError::NoSuchEndpoint);
        }
        let msg = self.queue.pop_front().ok_or(IpcError::QueueEmpty)?;
        self.queued_bytes = self.queued_bytes.saturating_sub(msg.payload.len());
        Ok(msg)
    }

    fn push_front(&mut self, msg: Message) -> Result<(), IpcError> {
        if !self.alive {
            return Err(IpcError::NoSuchEndpoint);
        }
        if self.queue.len() >= self.depth {
            return Err(IpcError::QueueFull);
        }
        let len = msg.payload.len();
        if self.queued_bytes.saturating_add(len) > self.max_queued_bytes {
            return Err(IpcError::NoSpace);
        }
        self.queue.push_front(msg);
        self.queued_bytes = self.queued_bytes.saturating_add(len);
        Ok(())
    }

    fn register_recv_waiter(&mut self, pid: WaiterId) {
        if !self.alive {
            return;
        }
        if self.recv_waiters.iter().any(|p| *p == pid) {
            return;
        }
        self.recv_waiters.push_back(pid);
    }

    fn register_send_waiter(&mut self, pid: WaiterId) {
        if !self.alive {
            return;
        }
        if self.send_waiters.iter().any(|p| *p == pid) {
            return;
        }
        self.send_waiters.push_back(pid);
    }

    fn pop_recv_waiter(&mut self) -> Option<WaiterId> {
        self.recv_waiters.pop_front()
    }

    fn pop_send_waiter(&mut self) -> Option<WaiterId> {
        self.send_waiters.pop_front()
    }

    fn remove_recv_waiter(&mut self, pid: WaiterId) -> bool {
        let before = self.recv_waiters.len();
        self.recv_waiters.retain(|p| *p != pid);
        before != self.recv_waiters.len()
    }

    fn remove_send_waiter(&mut self, pid: WaiterId) -> bool {
        let before = self.send_waiters.len();
        self.send_waiters.retain(|p| *p != pid);
        before != self.send_waiters.len()
    }

    fn close_if_owned_by(&mut self, owner: WaiterId) -> Option<(Vec<WaiterId>, Vec<WaiterId>)> {
        if !self.alive || self.owner != Some(owner) {
            return None;
        }
        self.alive = false;
        self.queue.clear();
        self.queued_bytes = 0;
        let recv: Vec<WaiterId> = self.recv_waiters.drain(..).collect();
        let send: Vec<WaiterId> = self.send_waiters.drain(..).collect();
        Some((recv, send))
    }
}

/// Message combining header and inline payload.
#[derive(Clone, Debug)]
pub struct Message {
    pub header: MessageHeader,
    pub payload: Vec<u8>,
    /// Optional capability moved alongside this message (Phase-2 hardening / scalability).
    pub moved_cap: Option<crate::cap::Capability>,
    /// Expected endpoint id for CAP_MOVE (when moving an Endpoint cap).
    ///
    /// SECURITY/ROBUSTNESS: This is a kernel-internal consistency field used to detect and
    /// correct mismatches between the moved capability's endpoint id at send-time vs
    /// receive-time. It MUST NOT be exposed to userspace directly.
    pub capmove_expected_ep: u32,
    /// Kernel-derived stable identity of the sender service (BootstrapInfo v2).
    ///
    /// This is populated by the syscall layer at send-time, and remains stable even if the sender
    /// exits before the receiver dequeues the message.
    pub sender_service_id: u64,
}

impl Message {
    /// Creates a message and truncates the payload length to match `header.len`.
    pub fn new(
        header: MessageHeader,
        payload: Vec<u8>,
        moved_cap: Option<crate::cap::Capability>,
    ) -> Self {
        let mut payload = payload;
        payload.truncate(header.len as usize);
        Self { header, payload, moved_cap, capmove_expected_ep: 0, sender_service_id: 0 }
    }
}

/// Router managing all kernel endpoints.
pub struct Router {
    endpoints: Vec<Endpoint>,
    queued_bytes_total: usize,
    max_queued_bytes_total: usize,
    owner_queued_bytes: BTreeMap<WaiterId, usize>,
    max_queued_bytes_per_owner: usize,
}

// DoS hardening: keep endpoint creation bounded until we have explicit accounting/quotas.
const MAX_ENDPOINTS: usize = 384;
const MAX_ENDPOINTS_PER_OWNER: usize = 96;

#[cfg(feature = "failpoints")]
static DENY_NEXT_SEND: AtomicBool = AtomicBool::new(false);

impl Router {
    /// Creates a router with space for `count` endpoints.
    pub fn new(count: usize) -> Self {
        // Global bytes budget: keep total queued payload bytes bounded across all endpoints.
        // Must be comfortably above boot traffic; per-endpoint budgets still apply.
        const DEFAULT_MAX_QUEUED_BYTES_TOTAL: usize = 1 * 1024 * 1024; // 1 MiB
                                                                       // Per-owner budget: cap total queued bytes into a single service (owner PID) across all
                                                                       // endpoints owned by that PID. This prevents one service inbox from consuming the entire
                                                                       // global budget via many endpoints.
        const DEFAULT_MAX_QUEUED_BYTES_PER_OWNER: usize = 256 * 1024; // 256 KiB
        let mut endpoints = Vec::with_capacity(count);
        for _ in 0..count {
            endpoints.push(Endpoint::with_depth(8, None));
        }
        Self {
            endpoints,
            queued_bytes_total: 0,
            max_queued_bytes_total: DEFAULT_MAX_QUEUED_BYTES_TOTAL,
            owner_queued_bytes: BTreeMap::new(),
            max_queued_bytes_per_owner: DEFAULT_MAX_QUEUED_BYTES_PER_OWNER,
        }
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

    fn recompute_accounting(&mut self) {
        self.queued_bytes_total = self.endpoints.iter().map(|ep| ep.queued_bytes).sum();
        self.owner_queued_bytes.clear();
        for ep in &self.endpoints {
            if !ep.alive {
                continue;
            }
            if let Some(owner) = ep.owner {
                *self.owner_queued_bytes.entry(owner).or_insert(0) += ep.queued_bytes;
            }
        }
    }

    /// Sends `msg` to the endpoint referenced by `id`.
    pub fn send_returning_message(
        &mut self,
        id: EndpointId,
        msg: Message,
    ) -> core::result::Result<(), (IpcError, Message)> {
        #[cfg(feature = "debug_uart")]
        {
            log_debug!(target: "ipc", "send enter");
            log_debug!(target: "ipc", "send target={} len={}", id, msg.header.len);
            log_debug!(target: "ipc", "send endpoints={} id={}", self.endpoints.len(), id);
        }
        #[cfg(feature = "failpoints")]
        if DENY_NEXT_SEND.swap(false, Ordering::SeqCst) {
            return Err((IpcError::PermissionDenied, msg));
        }
        let msg_len = msg.payload.len();
        let owner = self.endpoints.get(id as usize).and_then(|ep| ep.owner);
        if self.queued_bytes_total.saturating_add(msg_len) > self.max_queued_bytes_total {
            return Err((IpcError::NoSpace, msg));
        }
        if let Some(owner) = owner {
            let cur = self.owner_queued_bytes.get(&owner).copied().unwrap_or(0);
            if cur.saturating_add(msg_len) > self.max_queued_bytes_per_owner {
                return Err((IpcError::NoSpace, msg));
            }
        }
        let res = match self.endpoints.get_mut(id as usize) {
            Some(ep) => ep.push(msg),
            None => {
                #[cfg(feature = "debug_uart")]
                {
                    use core::fmt::Write as _;
                    let mut u = crate::uart::raw_writer();
                    let _ = writeln!(u, "IPC-ROUTER nosuch send ep=0x{:x}", id);
                }
                return Err((IpcError::NoSuchEndpoint, msg));
            }
        };
        if res.is_ok() {
            self.queued_bytes_total = self.queued_bytes_total.saturating_add(msg_len);
            if let Some(owner) = owner {
                *self.owner_queued_bytes.entry(owner).or_insert(0) += msg_len;
            }
        }
        #[cfg(feature = "debug_uart")]
        {
            match res {
                Ok(()) => log_debug!(target: "ipc", "send ok"),
                Err((IpcError::QueueFull, _)) => log_debug!(target: "ipc", "send queue full"),
                Err((IpcError::NoSuchEndpoint, _)) => {
                    log_debug!(target: "ipc", "send no such endpoint")
                }
                Err((IpcError::QueueEmpty, _)) => {
                    log_debug!(target: "ipc", "send queue empty (unexpected)")
                }
                Err((IpcError::PermissionDenied, _)) => {
                    log_debug!(target: "ipc", "send permission denied")
                }
                Err((IpcError::TimedOut, _)) => {
                    log_debug!(target: "ipc", "send timed out (unexpected)")
                }
                Err((IpcError::NoSpace, _)) => {
                    log_debug!(target: "ipc", "send nospace (unexpected)")
                }
            }
        }
        res
    }

    /// Sends `msg` to the endpoint referenced by `id` and discards the message on error.
    pub fn send(&mut self, id: EndpointId, msg: Message) -> Result<(), IpcError> {
        self.send_returning_message(id, msg).map_err(|(e, _msg)| e)
    }

    /// Receives the next message from the endpoint `id`.
    pub fn recv(&mut self, id: EndpointId) -> Result<Message, IpcError> {
        #[cfg(feature = "debug_uart")]
        log_debug!(target: "ipc", "recv enter");
        let owner = self.endpoints.get(id as usize).and_then(|ep| ep.owner);
        let res = match self.endpoints.get_mut(id as usize) {
            Some(ep) => ep.pop(),
            None => {
                #[cfg(feature = "debug_uart")]
                {
                    use core::fmt::Write as _;
                    let mut u = crate::uart::raw_writer();
                    let _ = writeln!(u, "IPC-ROUTER nosuch recv ep=0x{:x}", id);
                }
                return Err(IpcError::NoSuchEndpoint);
            }
        };
        if let Ok(ref msg) = res {
            self.queued_bytes_total = self.queued_bytes_total.saturating_sub(msg.payload.len());
            if let Some(owner) = owner {
                if let Some(v) = self.owner_queued_bytes.get_mut(&owner) {
                    *v = v.saturating_sub(msg.payload.len());
                    if *v == 0 {
                        self.owner_queued_bytes.remove(&owner);
                    }
                }
            }
        }
        #[cfg(feature = "debug_uart")]
        {
            match &res {
                Ok(_) => log_debug!(target: "ipc", "recv ok"),
                Err(IpcError::QueueEmpty) => log_debug!(target: "ipc", "recv empty"),
                Err(IpcError::NoSuchEndpoint) => {
                    log_debug!(target: "ipc", "recv no such endpoint")
                }
                Err(IpcError::QueueFull) => {
                    log_debug!(target: "ipc", "recv queue full (unexpected)")
                }
                Err(IpcError::PermissionDenied) => {
                    log_debug!(target: "ipc", "recv permission denied (unexpected)")
                }
                Err(IpcError::TimedOut) => {
                    log_debug!(target: "ipc", "recv timed out (unexpected)")
                }
                Err(IpcError::NoSpace) => {
                    log_debug!(target: "ipc", "recv nospace (unexpected)")
                }
            }
        }
        res
    }

    /// Re-queues `msg` at the front of endpoint `id`.
    pub fn requeue_front(&mut self, id: EndpointId, msg: Message) -> Result<(), IpcError> {
        let msg_len = msg.payload.len();
        let owner = self.endpoints.get(id as usize).and_then(|ep| ep.owner);
        if self.queued_bytes_total.saturating_add(msg_len) > self.max_queued_bytes_total {
            return Err(IpcError::NoSpace);
        }
        if let Some(owner) = owner {
            let cur = self.owner_queued_bytes.get(&owner).copied().unwrap_or(0);
            if cur.saturating_add(msg_len) > self.max_queued_bytes_per_owner {
                return Err(IpcError::NoSpace);
            }
        }
        let res =
            self.endpoints.get_mut(id as usize).ok_or(IpcError::NoSuchEndpoint)?.push_front(msg);
        if res.is_ok() {
            self.queued_bytes_total = self.queued_bytes_total.saturating_add(msg_len);
            if let Some(owner) = owner {
                *self.owner_queued_bytes.entry(owner).or_insert(0) += msg_len;
            }
        }
        res
    }

    /// Registers `pid` as a waiter for `recv` on endpoint `id` (queue empty, blocking).
    pub fn register_recv_waiter(&mut self, id: EndpointId, pid: WaiterId) -> Result<(), IpcError> {
        let ep = self.endpoints.get_mut(id as usize).ok_or(IpcError::NoSuchEndpoint)?;
        if !ep.alive {
            return Err(IpcError::NoSuchEndpoint);
        }
        ep.register_recv_waiter(pid);
        Ok(())
    }

    /// Registers `pid` as a waiter for `send` on endpoint `id` (queue full, blocking).
    pub fn register_send_waiter(&mut self, id: EndpointId, pid: WaiterId) -> Result<(), IpcError> {
        let ep = self.endpoints.get_mut(id as usize).ok_or(IpcError::NoSuchEndpoint)?;
        if !ep.alive {
            return Err(IpcError::NoSuchEndpoint);
        }
        ep.register_send_waiter(pid);
        Ok(())
    }

    /// Pops one waiter for `recv` on endpoint `id`.
    pub fn pop_recv_waiter(&mut self, id: EndpointId) -> Result<Option<WaiterId>, IpcError> {
        let ep = self.endpoints.get_mut(id as usize).ok_or(IpcError::NoSuchEndpoint)?;
        if !ep.alive {
            return Err(IpcError::NoSuchEndpoint);
        }
        Ok(ep.pop_recv_waiter())
    }

    /// Pops one waiter for `send` on endpoint `id`.
    pub fn pop_send_waiter(&mut self, id: EndpointId) -> Result<Option<WaiterId>, IpcError> {
        let ep = self.endpoints.get_mut(id as usize).ok_or(IpcError::NoSuchEndpoint)?;
        if !ep.alive {
            return Err(IpcError::NoSuchEndpoint);
        }
        Ok(ep.pop_send_waiter())
    }

    /// Removes `pid` from the recv waiter list, if present.
    pub fn remove_recv_waiter(&mut self, id: EndpointId, pid: WaiterId) -> Result<bool, IpcError> {
        let ep = self.endpoints.get_mut(id as usize).ok_or(IpcError::NoSuchEndpoint)?;
        if !ep.alive {
            return Err(IpcError::NoSuchEndpoint);
        }
        Ok(ep.remove_recv_waiter(pid))
    }

    /// Removes `pid` from the send waiter list, if present.
    pub fn remove_send_waiter(&mut self, id: EndpointId, pid: WaiterId) -> Result<bool, IpcError> {
        let ep = self.endpoints.get_mut(id as usize).ok_or(IpcError::NoSuchEndpoint)?;
        if !ep.alive {
            return Err(IpcError::NoSuchEndpoint);
        }
        Ok(ep.remove_send_waiter(pid))
    }

    /// Creates a new kernel endpoint and returns its identifier.
    pub fn create_endpoint(
        &mut self,
        depth: usize,
        owner: Option<WaiterId>,
    ) -> Result<EndpointId, IpcError> {
        let depth = depth.clamp(1, 256);
        if self.endpoints.len() >= MAX_ENDPOINTS {
            return Err(IpcError::NoSpace);
        }
        if let Some(owner_pid) = owner {
            let mut count: usize = 0;
            for ep in &self.endpoints {
                if ep.alive && ep.owner == Some(owner_pid) {
                    count += 1;
                }
            }
            if count >= MAX_ENDPOINTS_PER_OWNER {
                return Err(IpcError::NoSpace);
            }
        }
        let id = self.endpoints.len() as EndpointId;
        self.endpoints.push(Endpoint::with_depth(depth, owner));
        Ok(id)
    }

    /// Returns true if `id` exists and is alive.
    pub fn endpoint_alive(&self, id: EndpointId) -> bool {
        self.endpoints.get(id as usize).map(|ep| ep.alive).unwrap_or(false)
    }

    /// Closes every endpoint owned by `owner` and returns all drained waiter PIDs.
    pub fn close_endpoints_for_owner(&mut self, owner: WaiterId) -> Vec<WaiterId> {
        let mut out: Vec<WaiterId> = Vec::new();
        for (id, ep) in self.endpoints.iter_mut().enumerate() {
            #[cfg(not(feature = "ipc_trace_ring"))]
            let _ = id;
            if let Some((recv, send)) = ep.close_if_owned_by(owner) {
                #[cfg(feature = "ipc_trace_ring")]
                crate::ipc::trace::record_ep_close(owner, id as u32);
                out.extend(recv);
                out.extend(send);
            }
        }
        // Conservative: recompute totals (small N).
        self.recompute_accounting();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Closes a single endpoint unconditionally (used by privileged MANAGE holders).
    pub fn close_endpoint(&mut self, id: EndpointId) -> Result<Vec<WaiterId>, IpcError> {
        let ep = self.endpoints.get_mut(id as usize).ok_or(IpcError::NoSuchEndpoint)?;
        if !ep.alive {
            return Err(IpcError::NoSuchEndpoint);
        }
        ep.alive = false;
        ep.queue.clear();
        ep.queued_bytes = 0;
        let mut out: Vec<WaiterId> = Vec::new();
        out.extend(ep.recv_waiters.drain(..));
        out.extend(ep.send_waiters.drain(..));
        out.sort_unstable();
        out.dedup();
        // Conservative: recompute totals.
        self.recompute_accounting();
        Ok(out)
    }

    /// Removes `pid` from all endpoint waiter queues (best-effort cleanup on task exit).
    pub fn remove_waiter_from_all(&mut self, pid: WaiterId) {
        for ep in &mut self.endpoints {
            let _ = ep.remove_recv_waiter(pid);
            let _ = ep.remove_send_waiter(pid);
        }
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
    use super::*;
    use alloc::vec;

    #[test]
    fn loopback_roundtrip() {
        let mut router = Router::new(2);
        let header = MessageHeader::new(1, 0, 42, 0, 4);
        let payload = vec![1, 2, 3, 4];
        router.send(0, Message::new(header, payload.clone(), None)).unwrap();
        let received = router.recv(0).unwrap();
        assert_eq!(received.header.ty, 42);
        assert_eq!(received.payload, payload);
    }

    #[test]
    fn endpoint_close_drains_waiters_and_disconnects() {
        let mut router = Router::new(1);
        let ep = router.create_endpoint(4, Some(7)).unwrap();
        router.register_recv_waiter(ep, 100).unwrap();
        router.register_send_waiter(ep, 200).unwrap();
        let drained = router.close_endpoints_for_owner(7);
        assert_eq!(drained, vec![100, 200]);
        let header = MessageHeader::new(1, 0, 1, 0, 0);
        assert_eq!(
            router.send(ep, Message::new(header, vec![], None)).unwrap_err(),
            IpcError::NoSuchEndpoint
        );
        assert_eq!(router.recv(ep).unwrap_err(), IpcError::NoSuchEndpoint);
    }

    #[test]
    fn recv_waiters_are_fifo_and_deduped() {
        let mut router = Router::new(1);
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
        let mut router = Router::new(1);
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
            // Per-endpoint invariants.
            for ep in &router.endpoints {
                if !ep.alive {
                    continue;
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
            }

            // Router accounting invariants.
            let sum_total: usize = router.endpoints.iter().map(|e| e.queued_bytes).sum();
            assert_eq!(sum_total, router.queued_bytes_total, "global queued_bytes_total mismatch");
            assert!(
                router.queued_bytes_total <= router.max_queued_bytes_total,
                "global budget exceeded"
            );

            // Per-owner accounting matches endpoints.
            let mut expected: BTreeMap<WaiterId, usize> = BTreeMap::new();
            for ep in &router.endpoints {
                if !ep.alive {
                    continue;
                }
                if let Some(owner) = ep.owner {
                    *expected.entry(owner).or_insert(0) += ep.queued_bytes;
                }
            }
            // Treat zero entries as optional: some code paths keep owner entries only when non-zero,
            // while others (e.g. recompute) may insert explicit zeros. For budget enforcement,
            // missing == 0 is equivalent.
            let expected_nz: BTreeMap<WaiterId, usize> =
                expected.into_iter().filter(|(_k, v)| *v != 0).collect();
            let actual_nz: BTreeMap<WaiterId, usize> = router
                .owner_queued_bytes
                .iter()
                .filter(|(_k, v)| **v != 0)
                .map(|(k, v)| (*k, *v))
                .collect();
            assert_eq!(expected_nz, actual_nz, "owner_queued_bytes mismatch");
            for (_owner, bytes) in &router.owner_queued_bytes {
                assert!(*bytes <= router.max_queued_bytes_per_owner, "owner budget exceeded");
            }
        }

        let mut router = Router::new_with_bytes_budgets(0, 4096, 1024);
        let mut seed: u64 = 0x4E58_4950_435F_465Au64; // "NXIPC_FZ"

        // Start with a few endpoints so recv/send operations have targets.
        let owners: [WaiterId; 3] = [1, 7, 42];
        for (i, owner) in owners.iter().enumerate() {
            let _ = router.create_endpoint(4 + i, Some(*owner)).unwrap();
        }

        for step in 0..2_000u32 {
            let r = next_u64(&mut seed);
            let op = (r % 9) as u8;
            let ep_count = router.endpoints.len().max(1);
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
                    let payload = vec![0u8; len as usize];
                    let msg = Message::new(hdr, payload, None);
                    let _ = router.send(id, msg);
                }
                // send max payload (512) to exercise budgets
                3 => {
                    let hdr = MessageHeader::new(0, id, 2, 0, 512);
                    let msg = Message::new(hdr, vec![0u8; 512], None);
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
