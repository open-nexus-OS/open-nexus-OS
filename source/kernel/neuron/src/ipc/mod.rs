// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Kernel side IPC primitives.

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::vec::Vec;

#[cfg(feature = "failpoints")]
use core::sync::atomic::{AtomicBool, Ordering};

pub mod header;

use header::MessageHeader;

/// Identifier for a kernel endpoint.
pub type EndpointId = u32;

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
}

/// Representation of an endpoint queue.
#[derive(Default)]
struct Endpoint {
    queue: VecDeque<Message>,
    depth: usize,
}

impl Endpoint {
    fn with_depth(depth: usize) -> Self {
        Self { queue: VecDeque::new(), depth }
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
        crate::uart::write_line("IPC: send enter");
        {
            use core::fmt::Write as _;
            let mut w = crate::uart::raw_writer();
            let _ = write!(w, "IPC-I: target={} len={}\n", id, msg.header.len);
            let _ = write!(w, "IPC-SZ: endpoints={} id={}\n", self.endpoints.len(), id);
        }
        #[cfg(feature = "failpoints")]
        if DENY_NEXT_SEND.swap(false, Ordering::SeqCst) {
            return Err(IpcError::PermissionDenied);
        }
        let res = self.endpoints.get_mut(id as usize).ok_or(IpcError::NoSuchEndpoint)?.push(msg);
        match res {
            Ok(()) => crate::uart::write_line("IPC: send ok"),
            Err(IpcError::QueueFull) => crate::uart::write_line("IPC: send queue full"),
            Err(IpcError::NoSuchEndpoint) => crate::uart::write_line("IPC: send no such endpoint"),
            Err(IpcError::QueueEmpty) => {
                crate::uart::write_line("IPC: send queue empty (unexpected)")
            }
            Err(IpcError::PermissionDenied) => {
                crate::uart::write_line("IPC: send permission denied")
            }
        }
        res
    }

    /// Receives the next message from the endpoint `id`.
    pub fn recv(&mut self, id: EndpointId) -> Result<Message, IpcError> {
        crate::uart::write_line("IPC: recv enter");
        let res = self.endpoints.get_mut(id as usize).ok_or(IpcError::NoSuchEndpoint)?.pop();
        match &res {
            Ok(_) => crate::uart::write_line("IPC: recv ok"),
            Err(IpcError::QueueEmpty) => crate::uart::write_line("IPC: recv empty"),
            Err(IpcError::NoSuchEndpoint) => crate::uart::write_line("IPC: recv no such endpoint"),
            Err(IpcError::QueueFull) => {
                crate::uart::write_line("IPC: recv queue full (unexpected)")
            }
            Err(IpcError::PermissionDenied) => {
                crate::uart::write_line("IPC: recv permission denied (unexpected)")
            }
        }
        res
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
        router.send(0, Message::new(header, payload.clone())).unwrap();
        let received = router.recv(0).unwrap();
        assert_eq!(received.header.ty, 42);
        assert_eq!(received.payload, payload);
    }
}
