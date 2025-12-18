// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Kernel-side IPC primitives (endpoints, router)
//! OWNERS: @kernel-ipc-team
//! PUBLIC API: Router (send/recv), Message, EndpointId
//! DEPENDS_ON: ipc::header::MessageHeader
//! INVARIANTS: Header.len bounds payload; queue depth respected; no cross-layer deps
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::vec::Vec;

#[cfg(feature = "failpoints")]
use core::sync::atomic::{AtomicBool, Ordering};

pub mod header;

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
}

/// Representation of an endpoint queue.
#[derive(Default)]
struct Endpoint {
    queue: VecDeque<Message>,
    depth: usize,
    recv_waiters: VecDeque<WaiterId>,
    send_waiters: VecDeque<WaiterId>,
}

impl Endpoint {
    fn with_depth(depth: usize) -> Self {
        Self {
            queue: VecDeque::new(),
            depth,
            recv_waiters: VecDeque::new(),
            send_waiters: VecDeque::new(),
        }
    }

    fn push(&mut self, msg: Message) -> Result<(), IpcError> {
        if self.queue.len() >= self.depth {
            return Err(IpcError::QueueFull);
        }
        self.queue.push_back(msg);
        Ok(())
    }

    fn pop(&mut self) -> Result<Message, IpcError> {
        self.queue.pop_front().ok_or(IpcError::QueueEmpty)
    }

    fn register_recv_waiter(&mut self, pid: WaiterId) {
        if self.recv_waiters.iter().any(|p| *p == pid) {
            return;
        }
        self.recv_waiters.push_back(pid);
    }

    fn register_send_waiter(&mut self, pid: WaiterId) {
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
}

/// Message combining header and inline payload.
#[derive(Clone)]
pub struct Message {
    pub header: MessageHeader,
    pub payload: Vec<u8>,
}

impl Message {
    /// Creates a message and truncates the payload length to match `header.len`.
    pub fn new(header: MessageHeader, payload: Vec<u8>) -> Self {
        let mut payload = payload;
        payload.truncate(header.len as usize);
        Self { header, payload }
    }
}

/// Router managing all kernel endpoints.
pub struct Router {
    endpoints: Vec<Endpoint>,
}

#[cfg(feature = "failpoints")]
static DENY_NEXT_SEND: AtomicBool = AtomicBool::new(false);

impl Router {
    /// Creates a router with space for `count` endpoints.
    pub fn new(count: usize) -> Self {
        let mut endpoints = Vec::with_capacity(count);
        for _ in 0..count {
            endpoints.push(Endpoint::with_depth(8));
        }
        Self { endpoints }
    }

    /// Sends `msg` to the endpoint referenced by `id`.
    pub fn send(&mut self, id: EndpointId, msg: Message) -> Result<(), IpcError> {
        #[cfg(feature = "debug_uart")]
        {
            log_debug!(target: "ipc", "send enter");
            log_debug!(target: "ipc", "send target={} len={}", id, msg.header.len);
            log_debug!(target: "ipc", "send endpoints={} id={}", self.endpoints.len(), id);
        }
        #[cfg(feature = "failpoints")]
        if DENY_NEXT_SEND.swap(false, Ordering::SeqCst) {
            return Err(IpcError::PermissionDenied);
        }
        let res = self
            .endpoints
            .get_mut(id as usize)
            .ok_or(IpcError::NoSuchEndpoint)?
            .push(msg);
        #[cfg(feature = "debug_uart")]
        {
            match res {
                Ok(()) => log_debug!(target: "ipc", "send ok"),
                Err(IpcError::QueueFull) => log_debug!(target: "ipc", "send queue full"),
                Err(IpcError::NoSuchEndpoint) => {
                    log_debug!(target: "ipc", "send no such endpoint")
                }
                Err(IpcError::QueueEmpty) => {
                    log_debug!(target: "ipc", "send queue empty (unexpected)")
                }
                Err(IpcError::PermissionDenied) => {
                    log_debug!(target: "ipc", "send permission denied")
                }
                Err(IpcError::TimedOut) => {
                    log_debug!(target: "ipc", "send timed out (unexpected)")
                }
            }
        }
        res
    }

    /// Receives the next message from the endpoint `id`.
    pub fn recv(&mut self, id: EndpointId) -> Result<Message, IpcError> {
        #[cfg(feature = "debug_uart")]
        log_debug!(target: "ipc", "recv enter");
        let res = self
            .endpoints
            .get_mut(id as usize)
            .ok_or(IpcError::NoSuchEndpoint)?
            .pop();
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
            }
        }
        res
    }

    /// Registers `pid` as a waiter for `recv` on endpoint `id` (queue empty, blocking).
    pub fn register_recv_waiter(&mut self, id: EndpointId, pid: WaiterId) -> Result<(), IpcError> {
        let ep = self
            .endpoints
            .get_mut(id as usize)
            .ok_or(IpcError::NoSuchEndpoint)?;
        ep.register_recv_waiter(pid);
        Ok(())
    }

    /// Registers `pid` as a waiter for `send` on endpoint `id` (queue full, blocking).
    pub fn register_send_waiter(&mut self, id: EndpointId, pid: WaiterId) -> Result<(), IpcError> {
        let ep = self
            .endpoints
            .get_mut(id as usize)
            .ok_or(IpcError::NoSuchEndpoint)?;
        ep.register_send_waiter(pid);
        Ok(())
    }

    /// Pops one waiter for `recv` on endpoint `id`.
    pub fn pop_recv_waiter(&mut self, id: EndpointId) -> Result<Option<WaiterId>, IpcError> {
        let ep = self
            .endpoints
            .get_mut(id as usize)
            .ok_or(IpcError::NoSuchEndpoint)?;
        Ok(ep.pop_recv_waiter())
    }

    /// Pops one waiter for `send` on endpoint `id`.
    pub fn pop_send_waiter(&mut self, id: EndpointId) -> Result<Option<WaiterId>, IpcError> {
        let ep = self
            .endpoints
            .get_mut(id as usize)
            .ok_or(IpcError::NoSuchEndpoint)?;
        Ok(ep.pop_send_waiter())
    }

    /// Removes `pid` from the recv waiter list, if present.
    pub fn remove_recv_waiter(&mut self, id: EndpointId, pid: WaiterId) -> Result<bool, IpcError> {
        let ep = self
            .endpoints
            .get_mut(id as usize)
            .ok_or(IpcError::NoSuchEndpoint)?;
        Ok(ep.remove_recv_waiter(pid))
    }

    /// Removes `pid` from the send waiter list, if present.
    pub fn remove_send_waiter(&mut self, id: EndpointId, pid: WaiterId) -> Result<bool, IpcError> {
        let ep = self
            .endpoints
            .get_mut(id as usize)
            .ok_or(IpcError::NoSuchEndpoint)?;
        Ok(ep.remove_send_waiter(pid))
    }

    /// Creates a new kernel endpoint and returns its identifier.
    pub fn create_endpoint(&mut self, depth: usize) -> EndpointId {
        let depth = depth.clamp(1, 256);
        let id = self.endpoints.len() as EndpointId;
        self.endpoints.push(Endpoint::with_depth(depth));
        id
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
        router
            .send(0, Message::new(header, payload.clone()))
            .unwrap();
        let received = router.recv(0).unwrap();
        assert_eq!(received.header.ty, 42);
        assert_eq!(received.payload, payload);
    }
}
