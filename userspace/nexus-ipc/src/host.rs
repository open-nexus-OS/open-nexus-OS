// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: In-process IPC emulation for host-based tests
//! INTENT: Loopback client/server pairs for integration testing
//! IDL (target): loopbackChannel(), send(frame), recv(wait)
//! DEPS: std::sync::mpsc (channels), parking_lot::Mutex (synchronization)
//! READINESS: Host backend ready; used for testing
//! TESTS: Loopback roundtrip, timeout handling, disconnected state
//! In-process IPC emulation for host-based tests.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};

use parking_lot::Mutex;

use crate::{Client, IpcError, Result, Server, Wait};

/// Creates a loopback client/server pair backed by in-memory channels.
pub fn loopback_channel() -> (LoopbackClient, LoopbackServer) {
    let (req_tx, req_rx) = mpsc::channel::<Vec<u8>>();
    let (rsp_tx, rsp_rx) = mpsc::channel::<Vec<u8>>();
    (
        LoopbackClient::new(req_tx, Mutex::new(rsp_rx)),
        LoopbackServer::new(Mutex::new(req_rx), rsp_tx),
    )
}

/// Client implementation backed by in-memory channels.
pub struct LoopbackClient {
    request_tx: Sender<Vec<u8>>,
    response_rx: Mutex<Receiver<Vec<u8>>>,
}

impl LoopbackClient {
    fn new(request_tx: Sender<Vec<u8>>, response_rx: Mutex<Receiver<Vec<u8>>>) -> Self {
        Self { request_tx, response_rx }
    }
}

impl Client for LoopbackClient {
    fn send(&self, frame: &[u8], _wait: Wait) -> Result<()> {
        self.request_tx.send(frame.to_vec()).map_err(|_| IpcError::Disconnected)
    }

    fn recv(&self, wait: Wait) -> Result<Vec<u8>> {
        let receiver = self.response_rx.lock();
        match wait {
            Wait::Blocking => receiver.recv().map_err(|_| IpcError::Disconnected),
            Wait::NonBlocking => receiver.try_recv().map_err(|err| match err {
                TryRecvError::Empty => IpcError::WouldBlock,
                TryRecvError::Disconnected => IpcError::Disconnected,
            }),
            Wait::Timeout(timeout) => {
                if timeout.is_zero() {
                    return receiver.try_recv().map_err(|err| match err {
                        TryRecvError::Empty => IpcError::WouldBlock,
                        TryRecvError::Disconnected => IpcError::Disconnected,
                    });
                }
                receiver.recv_timeout(timeout).map_err(|err| match err {
                    RecvTimeoutError::Timeout => IpcError::Timeout,
                    RecvTimeoutError::Disconnected => IpcError::Disconnected,
                })
            }
        }
    }
}

/// Server implementation backed by in-memory channels.
pub struct LoopbackServer {
    request_rx: Mutex<Receiver<Vec<u8>>>,
    response_tx: Sender<Vec<u8>>,
}

impl LoopbackServer {
    fn new(request_rx: Mutex<Receiver<Vec<u8>>>, response_tx: Sender<Vec<u8>>) -> Self {
        Self { request_rx, response_tx }
    }
}

impl Server for LoopbackServer {
    fn recv(&self, wait: Wait) -> Result<Vec<u8>> {
        let receiver = self.request_rx.lock();
        match wait {
            Wait::Blocking => receiver.recv().map_err(|_| IpcError::Disconnected),
            Wait::NonBlocking => receiver.try_recv().map_err(|err| match err {
                TryRecvError::Empty => IpcError::WouldBlock,
                TryRecvError::Disconnected => IpcError::Disconnected,
            }),
            Wait::Timeout(timeout) => {
                if timeout.is_zero() {
                    return receiver.try_recv().map_err(|err| match err {
                        TryRecvError::Empty => IpcError::WouldBlock,
                        TryRecvError::Disconnected => IpcError::Disconnected,
                    });
                }
                receiver.recv_timeout(timeout).map_err(|err| match err {
                    RecvTimeoutError::Timeout => IpcError::Timeout,
                    RecvTimeoutError::Disconnected => IpcError::Disconnected,
                })
            }
        }
    }

    fn send(&self, frame: &[u8], _wait: Wait) -> Result<()> {
        self.response_tx.send(frame.to_vec()).map_err(|_| IpcError::Disconnected)
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
        let err = client.recv(Wait::Timeout(Duration::from_millis(10))).unwrap_err();
        assert_eq!(err, IpcError::Timeout);
    }
}
