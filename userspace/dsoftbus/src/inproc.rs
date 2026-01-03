// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: In-process host transport for DSoftBus-lite (socketless, deterministic tests)
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered by dsoftbus integration tests (in-proc transport)
//!
//! TEST_SCENARIOS (implemented):
//!   - `userspace/dsoftbus/tests/host_transport.rs`: handshake happy path + ping/pong; auth-failure
//!
//! ADR: docs/adr/0005-dsoftbus-architecture.md

#![forbid(unsafe_code)]

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use once_cell::sync::Lazy;
use parking_lot::Mutex;

use identity::{DeviceId, Identity};

use crate::{
    derive_noise_keys, Announcement, AuthError, Authenticator, FramePayload, Session, SessionError,
    Stream, StreamError,
};

/// Port allocator for in-process listeners (used when bind port is 0).
static NEXT_PORT: AtomicU16 = AtomicU16::new(40_000);

/// Registry mapping port -> listener inbox sender.
static LISTENERS: Lazy<Mutex<HashMap<u16, mpsc::Sender<InProcDuplex>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

#[derive(Clone)]
struct InProcDuplex {
    rx: Arc<Mutex<VecDeque<u8>>>,
    tx: Arc<Mutex<VecDeque<u8>>>,
}

impl InProcDuplex {
    fn pair() -> (Self, Self) {
        let a_to_b = Arc::new(Mutex::new(VecDeque::<u8>::new()));
        let b_to_a = Arc::new(Mutex::new(VecDeque::<u8>::new()));

        let a = Self {
            rx: Arc::clone(&b_to_a),
            tx: Arc::clone(&a_to_b),
        };
        let b = Self { rx: a_to_b, tx: b_to_a };
        (a, b)
    }
}

impl Read for InProcDuplex {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut rx = self.rx.lock();
        if rx.is_empty() {
            return Err(std::io::Error::new(std::io::ErrorKind::WouldBlock, "no data"));
        }
        let mut n = 0usize;
        while n < buf.len() {
            match rx.pop_front() {
                Some(b) => {
                    buf[n] = b;
                    n += 1;
                }
                None => break,
            }
        }
        Ok(n)
    }
}

impl Write for InProcDuplex {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut tx = self.tx.lock();
        for &b in buf {
            tx.push_back(b);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// In-process authenticator: no OS sockets, deterministic in tests.
pub struct InProcAuthenticator {
    port: u16,
    rx: mpsc::Receiver<InProcDuplex>,
    identity: Identity,
    noise_secret: [u8; 32],
    noise_public: [u8; 32],
}

impl Drop for InProcAuthenticator {
    fn drop(&mut self) {
        let mut reg = LISTENERS.lock();
        let _ = reg.remove(&self.port);
    }
}

impl InProcAuthenticator {
    pub fn local_noise_public(&self) -> [u8; 32] {
        self.noise_public
    }

    pub fn local_port(&self) -> u16 {
        self.port
    }

    fn alloc_port() -> u16 {
        // Keep within a non-privileged range.
        NEXT_PORT.fetch_add(1, Ordering::SeqCst)
    }

    fn register(port: u16, tx: mpsc::Sender<InProcDuplex>) -> Result<(), AuthError> {
        let mut reg = LISTENERS.lock();
        if reg.contains_key(&port) {
            return Err(AuthError::Io(std::io::Error::new(
                std::io::ErrorKind::AddrInUse,
                "inproc port in use",
            )));
        }
        reg.insert(port, tx);
        Ok(())
    }

    fn lookup(port: u16) -> Result<mpsc::Sender<InProcDuplex>, AuthError> {
        let reg = LISTENERS.lock();
        reg.get(&port).cloned().ok_or_else(|| {
            AuthError::Io(std::io::Error::new(
                std::io::ErrorKind::ConnectionRefused,
                "no inproc listener",
            ))
        })
    }
}

impl Authenticator for InProcAuthenticator {
    type Session = InProcSession;

    fn bind(addr: SocketAddr, identity: Identity) -> Result<Self, AuthError> {
        let port = match addr.port() {
            0 => Self::alloc_port(),
            p => p,
        };
        let (tx, rx) = mpsc::channel::<InProcDuplex>();
        Self::register(port, tx)?;

        let (noise_secret, noise_public) = derive_noise_keys(&identity);
        Ok(Self {
            port,
            rx,
            identity,
            noise_secret,
            noise_public,
        })
    }

    fn accept(&self) -> Result<Self::Session, AuthError> {
        let mut duplex = self
            .rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| {
                AuthError::Io(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "accept timed out",
                ))
            })?;

        let (mut transport, device_id) = crate::host::handshake_accept(
            &self.identity,
            &self.noise_secret,
            &self.noise_public,
            &mut duplex,
        )?;

        let request_id = crate::host::receive_connect_request(&mut duplex, &mut transport)?;
        if request_id != device_id.as_str() {
            crate::host::send_connect_response(&mut duplex, &mut transport, false)?;
            return Err(AuthError::Identity("device mismatch".into()));
        }
        crate::host::send_connect_response(&mut duplex, &mut transport, true)?;

        Ok(InProcSession {
            duplex,
            transport,
            remote_device: device_id,
        })
    }

    fn connect(&self, announcement: &Announcement) -> Result<Self::Session, AuthError> {
        let port = announcement.port();
        let sender = Self::lookup(port)?;

        let (mut client, server) = InProcDuplex::pair();
        sender.send(server).map_err(|_| {
            AuthError::Io(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "send"))
        })?;

        let (mut transport, device_id) = crate::host::handshake_connect(
            &self.identity,
            &self.noise_secret,
            &self.noise_public,
            announcement,
            &mut client,
        )?;

        crate::host::send_connect_request(&mut client, &mut transport, self.identity.device_id())?;
        let ok = crate::host::receive_connect_response(&mut client, &mut transport)?;
        if !ok {
            return Err(AuthError::Identity("connection rejected".into()));
        }
        Ok(InProcSession {
            duplex: client,
            transport,
            remote_device: device_id,
        })
    }
}

pub struct InProcSession {
    duplex: InProcDuplex,
    transport: snow::TransportState,
    remote_device: DeviceId,
}

impl Session for InProcSession {
    type Stream = InProcStream;

    fn remote_device_id(&self) -> &DeviceId {
        &self.remote_device
    }

    fn into_stream(self) -> Result<Self::Stream, SessionError> {
        Ok(InProcStream {
            duplex: self.duplex,
            transport: self.transport,
        })
    }
}

pub struct InProcStream {
    duplex: InProcDuplex,
    transport: snow::TransportState,
}

impl Stream for InProcStream {
    fn send(&mut self, channel: u32, payload: &[u8]) -> Result<(), StreamError> {
        // Reuse the same framing as the host backend (capnp frame).
        let bytes = crate::host::serialize_frame(channel, payload)?;
        let encrypted = crate::host::encrypt_payload(&mut self.transport, &bytes)?;
        crate::host::write_frame(&mut self.duplex, &encrypted).map_err(StreamError::from)
    }

    fn recv(&mut self) -> Result<Option<FramePayload>, StreamError> {
        let frame = crate::host::read_frame(&mut self.duplex).map_err(StreamError::from)?;
        let bytes = crate::host::decrypt_payload(&mut self.transport, &frame)?;
        crate::host::deserialize_frame(&bytes).map(Some)
    }
}

