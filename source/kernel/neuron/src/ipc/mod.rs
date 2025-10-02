// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Kernel side IPC primitives.

use alloc::collections::VecDeque;
use alloc::vec::Vec;

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
        self.endpoints
            .get_mut(id as usize)
            .ok_or(IpcError::NoSuchEndpoint)?
            .push(msg)
    }

    /// Receives the next message from the endpoint `id`.
    pub fn recv(&mut self, id: EndpointId) -> Result<Message, IpcError> {
        self.endpoints
            .get_mut(id as usize)
            .ok_or(IpcError::NoSuchEndpoint)?
            .pop()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
