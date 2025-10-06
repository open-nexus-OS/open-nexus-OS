// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

use std::sync::mpsc::{self, Receiver, Sender};

/// Client endpoint used by tests to send requests and receive responses.
pub struct LoopbackClient {
    request_tx: Sender<Vec<u8>>,
    response_rx: Receiver<Vec<u8>>,
}

impl LoopbackClient {
    fn new(request_tx: Sender<Vec<u8>>, response_rx: Receiver<Vec<u8>>) -> Self {
        Self {
            request_tx,
            response_rx,
        }
    }

    /// Sends a frame to the server and waits for the response.
    pub fn call(&self, frame: Vec<u8>) -> Vec<u8> {
        self.request_tx.send(frame).expect("send frame");
        self.response_rx.recv().expect("recv frame")
    }
}

struct ServerEndpoint {
    request_rx: Receiver<Vec<u8>>,
    response_tx: Sender<Vec<u8>>,
}

impl ServerEndpoint {
    fn new() -> (LoopbackClient, Self) {
        let (request_tx, request_rx) = mpsc::channel();
        let (response_tx, response_rx) = mpsc::channel();
        let client = LoopbackClient::new(request_tx, response_rx);
        let server = Self {
            request_rx,
            response_tx,
        };
        (client, server)
    }

    fn recv(&self) -> Result<Option<Vec<u8>>, LoopbackError> {
        match self.request_rx.recv() {
            Ok(frame) => Ok(Some(frame)),
            Err(_) => Ok(None),
        }
    }

    fn send(&self, frame: &[u8]) -> Result<(), LoopbackError> {
        self.response_tx
            .send(frame.to_vec())
            .map_err(|_| LoopbackError)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LoopbackError;

impl Into<samgrd::TransportError> for LoopbackError {
    fn into(self) -> samgrd::TransportError {
        samgrd::TransportError::Closed
    }
}

impl Into<bundlemgrd::TransportError> for LoopbackError {
    fn into(self) -> bundlemgrd::TransportError {
        bundlemgrd::TransportError::Closed
    }
}

/// Server transport implementing `samgrd::Transport` using in-process channels.
pub struct SamgrServerTransport {
    endpoint: ServerEndpoint,
}

impl SamgrServerTransport {
    fn new(endpoint: ServerEndpoint) -> Self {
        Self { endpoint }
    }
}

impl samgrd::Transport for SamgrServerTransport {
    type Error = LoopbackError;

    fn recv(&mut self) -> Result<Option<Vec<u8>>, Self::Error> {
        self.endpoint.recv()
    }

    fn send(&mut self, frame: &[u8]) -> Result<(), Self::Error> {
        self.endpoint.send(frame)
    }
}

/// Server transport implementing `bundlemgrd::Transport` using in-process channels.
pub struct BundleServerTransport {
    endpoint: ServerEndpoint,
}

impl BundleServerTransport {
    fn new(endpoint: ServerEndpoint) -> Self {
        Self { endpoint }
    }
}

impl bundlemgrd::Transport for BundleServerTransport {
    type Error = LoopbackError;

    fn recv(&mut self) -> Result<Option<Vec<u8>>, Self::Error> {
        self.endpoint.recv()
    }

    fn send(&mut self, frame: &[u8]) -> Result<(), Self::Error> {
        self.endpoint.send(frame)
    }
}

/// Creates a loopback pair for the SAMGR daemon.
pub fn samgr_loopback() -> (LoopbackClient, SamgrServerTransport) {
    let (client, endpoint) = ServerEndpoint::new();
    (client, SamgrServerTransport::new(endpoint))
}

/// Creates a loopback pair for the bundle manager daemon.
pub fn bundle_loopback() -> (LoopbackClient, BundleServerTransport) {
    let (client, endpoint) = ServerEndpoint::new();
    (client, BundleServerTransport::new(endpoint))
}
