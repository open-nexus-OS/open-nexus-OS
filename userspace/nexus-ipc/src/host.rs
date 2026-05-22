// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: In-process IPC emulation for host-based testing
//!
//! OWNERS: @runtime
//!
//! PUBLIC API:
//!   - loopback_channel(): Create client/server pair backed by in-memory channels
//!   - struct LoopbackClient: Client implementation for in-process testing
//!   - struct LoopbackServer: Server implementation for in-process testing
//!   - LoopbackClient::new(): Create client with request sender and response receiver
//!   - LoopbackServer::new(): Create server with request receiver and response sender
//!
//! SECURITY INVARIANTS:
//!   - No unsafe code in loopback operations
//!   - Channel-based communication prevents data races
//!   - Frame boundaries are preserved
//!   - Timeout handling prevents indefinite blocking
//!
//! ERROR CONDITIONS:
//!   - IpcError::Disconnected: Channel disconnected
//!   - IpcError::WouldBlock: Operation would block in non-blocking mode
//!   - IpcError::Timeout: Operation timed out
//!
//! DEPENDENCIES:
//!   - std::sync::mpsc: Channel-based communication
//!   - std::sync::Mutex: protects the single-consumer receiver side
//!
//! FEATURES:
//!   - In-process IPC emulation
//!   - Loopback client/server pairs
//!   - Blocking, non-blocking, and timeout operations
//!   - Channel-based request/response communication
//!   - Integration testing support
//!
//! TEST SCENARIOS:
//!   - test_loopback_roundtrip(): Test client-server communication
//!   - test_timeout_handling(): Test timeout behavior
//!   - test_disconnected_state(): Test channel disconnection
//!   - test_blocking_operations(): Test blocking send/recv
//!   - test_non_blocking_operations(): Test non-blocking send/recv
//!   - test_frame_boundaries(): Test message integrity
//!   - test_concurrent_access(): Test concurrent client/server access
//!   - test_integration_scenarios(): Test integration scenarios
//!
//! ADR: docs/adr/0003-ipc-runtime-architecture.md

use std::sync::{
    mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError},
    Mutex,
};

use crate::{Client, IpcError, Result, Server, Wait};

/// Creates a loopback client/server pair backed by in-memory channels.
pub fn loopback_channel() -> (LoopbackClient, LoopbackServer) {
    let (req_tx, req_rx) = mpsc::channel::<RequestFrame>();
    let (rsp_tx, rsp_rx) = mpsc::channel::<ReplyFrame>();
    (
        LoopbackClient::new(req_tx, Mutex::new(rsp_rx)),
        LoopbackServer::new(Mutex::new(req_rx), rsp_tx),
    )
}

/// Request bytes sent from a loopback client to a server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestFrame(Vec<u8>);

impl RequestFrame {
    fn from_bytes(frame: &[u8]) -> Self {
        Self(frame.to_vec())
    }

    fn into_bytes(self) -> Vec<u8> {
        self.0
    }
}

/// Reply bytes sent from a loopback server to a client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplyFrame(Vec<u8>);

impl ReplyFrame {
    fn from_bytes(frame: &[u8]) -> Self {
        Self(frame.to_vec())
    }

    fn into_bytes(self) -> Vec<u8> {
        self.0
    }
}

/// Client implementation backed by in-memory channels.
pub struct LoopbackClient {
    request_tx: Sender<RequestFrame>,
    response_rx: Mutex<Receiver<ReplyFrame>>,
}

impl LoopbackClient {
    fn new(request_tx: Sender<RequestFrame>, response_rx: Mutex<Receiver<ReplyFrame>>) -> Self {
        Self {
            request_tx,
            response_rx,
        }
    }
}

impl Client for LoopbackClient {
    fn send(&self, frame: &[u8], _wait: Wait) -> Result<()> {
        self.request_tx
            .send(RequestFrame::from_bytes(frame))
            .map_err(|_| IpcError::Disconnected)
    }

    fn recv(&self, wait: Wait) -> Result<Vec<u8>> {
        let receiver = self
            .response_rx
            .lock()
            .map_err(|_| IpcError::Disconnected)?;
        match wait {
            Wait::Blocking => receiver
                .recv()
                .map(ReplyFrame::into_bytes)
                .map_err(|_| IpcError::Disconnected),
            Wait::NonBlocking => receiver
                .try_recv()
                .map_err(|err| match err {
                    TryRecvError::Empty => IpcError::WouldBlock,
                    TryRecvError::Disconnected => IpcError::Disconnected,
                })
                .map(ReplyFrame::into_bytes),
            Wait::Timeout(timeout) => {
                if timeout.is_zero() {
                    return receiver
                        .try_recv()
                        .map_err(|err| match err {
                            TryRecvError::Empty => IpcError::WouldBlock,
                            TryRecvError::Disconnected => IpcError::Disconnected,
                        })
                        .map(ReplyFrame::into_bytes);
                }
                receiver
                    .recv_timeout(timeout)
                    .map_err(|err| match err {
                        RecvTimeoutError::Timeout => IpcError::Timeout,
                        RecvTimeoutError::Disconnected => IpcError::Disconnected,
                    })
                    .map(ReplyFrame::into_bytes)
            }
        }
    }
}

/// Server implementation backed by in-memory channels.
pub struct LoopbackServer {
    request_rx: Mutex<Receiver<RequestFrame>>,
    response_tx: Sender<ReplyFrame>,
}

impl LoopbackServer {
    fn new(request_rx: Mutex<Receiver<RequestFrame>>, response_tx: Sender<ReplyFrame>) -> Self {
        Self {
            request_rx,
            response_tx,
        }
    }
}

impl Server for LoopbackServer {
    fn recv(&self, wait: Wait) -> Result<Vec<u8>> {
        let receiver = self.request_rx.lock().map_err(|_| IpcError::Disconnected)?;
        match wait {
            Wait::Blocking => receiver
                .recv()
                .map(RequestFrame::into_bytes)
                .map_err(|_| IpcError::Disconnected),
            Wait::NonBlocking => receiver
                .try_recv()
                .map_err(|err| match err {
                    TryRecvError::Empty => IpcError::WouldBlock,
                    TryRecvError::Disconnected => IpcError::Disconnected,
                })
                .map(RequestFrame::into_bytes),
            Wait::Timeout(timeout) => {
                if timeout.is_zero() {
                    return receiver
                        .try_recv()
                        .map_err(|err| match err {
                            TryRecvError::Empty => IpcError::WouldBlock,
                            TryRecvError::Disconnected => IpcError::Disconnected,
                        })
                        .map(RequestFrame::into_bytes);
                }
                receiver
                    .recv_timeout(timeout)
                    .map_err(|err| match err {
                        RecvTimeoutError::Timeout => IpcError::Timeout,
                        RecvTimeoutError::Disconnected => IpcError::Disconnected,
                    })
                    .map(RequestFrame::into_bytes)
            }
        }
    }

    fn send(&self, frame: &[u8], _wait: Wait) -> Result<()> {
        self.response_tx
            .send(ReplyFrame::from_bytes(frame))
            .map_err(|_| IpcError::Disconnected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn loopback_roundtrip() {
        let (client, server) = loopback_channel();
        server.send(b"pong", Wait::Blocking).unwrap();
        assert_eq!(client.recv(Wait::Blocking).unwrap(), b"pong");
    }

    #[test]
    fn recv_timeout() {
        let (client, _server) = loopback_channel();
        let err = client
            .recv(Wait::Timeout(Duration::from_millis(10)))
            .unwrap_err();
        assert_eq!(err, IpcError::Timeout);
    }
}
