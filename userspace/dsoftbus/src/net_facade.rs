// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: DSoftBus host transport over the nexus-net sockets facade (contract-backed)
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered by dsoftbus integration tests (facade transport)
//!
//! TEST_SCENARIOS (implemented):
//!   - `userspace/dsoftbus/tests/facade_transport.rs`: handshake happy path + ping/pong, auth-failure, deterministic accept timeout
//!
//! ADR: docs/adr/0005-dsoftbus-architecture.md

#![forbid(unsafe_code)]

use parking_lot::Mutex;
use std::io::{Read, Write};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use identity::{DeviceId, Identity};
use nexus_net::{NetError, NetInstant, NetSocketAddrV4, NetStack, TcpListener, TcpStream};

use crate::{Announcement, AuthError, Session, SessionError, Stream, StreamError};

// Tick-based deadlines are deterministic and independent of wall-clock time. Keep this large enough
// that cross-thread scheduling in host tests cannot exhaust the budget before a peer gets CPU time.
const DEFAULT_DEADLINE_TICKS: NetInstant = 20_000;

fn to_v4(addr: SocketAddr) -> Result<NetSocketAddrV4, AuthError> {
    match addr.ip() {
        std::net::IpAddr::V4(v4) => Ok(NetSocketAddrV4::new(v4.octets(), addr.port())),
        std::net::IpAddr::V6(_) => Err(AuthError::Protocol("ipv6 unsupported".into())),
    }
}

fn neterr_to_io(err: NetError) -> std::io::Error {
    match err {
        NetError::WouldBlock => std::io::Error::new(std::io::ErrorKind::WouldBlock, "would block"),
        NetError::TimedOut => std::io::Error::new(std::io::ErrorKind::TimedOut, "timed out"),
        NetError::Disconnected => {
            std::io::Error::new(std::io::ErrorKind::ConnectionReset, "disconnected")
        }
        NetError::InvalidInput(msg) => std::io::Error::new(std::io::ErrorKind::InvalidInput, msg),
        other => std::io::Error::other(other.to_string()),
    }
}

struct NetTcpIo<S: TcpStream> {
    inner: S,
}

impl<S: TcpStream> NetTcpIo<S> {
    fn new(inner: S) -> Self {
        Self { inner }
    }
}

impl<S: TcpStream> Read for NetTcpIo<S> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self.inner.read(None, buf) {
            Ok(n) => Ok(n),
            Err(e) => Err(neterr_to_io(e)),
        }
    }
}

impl<S: TcpStream> Write for NetTcpIo<S> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self.inner.write(None, buf) {
            Ok(n) => Ok(n),
            Err(e) => Err(neterr_to_io(e)),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Authenticator running over a `nexus-net` sockets facade backend.
///
/// This is host-first (Phase 0): it allows validating DSoftBus transport behavior against the
/// sockets facade contract without OS networking.
pub struct FacadeAuthenticator<N: NetStack> {
    net: Arc<Mutex<N>>,
    listener_addr: NetSocketAddrV4,
    listener: Mutex<N::TcpListener>,
    identity: Identity,
    noise_secret: [u8; 32],
    noise_public: [u8; 32],
    tick: AtomicU64,
}

impl<N> FacadeAuthenticator<N>
where
    N: NetStack + Send + Sync + 'static,
    N::TcpStream: Send + 'static,
    N::TcpListener: Send + 'static,
{
    pub fn new(net: N, bind: SocketAddr, identity: Identity) -> Result<Self, AuthError> {
        let mut net = net;
        let local = to_v4(bind)?;
        let listener = net
            .tcp_listen(local, 8)
            .map_err(|e| AuthError::Io(neterr_to_io(e)))?;
        let listener_addr = listener.local_addr();

        let (noise_secret, noise_public) = crate::derive_noise_keys(&identity);
        Ok(Self {
            net: Arc::new(Mutex::new(net)),
            listener_addr,
            listener: Mutex::new(listener),
            identity,
            noise_secret,
            noise_public,
            tick: AtomicU64::new(0),
        })
    }

    pub fn local_noise_public(&self) -> [u8; 32] {
        self.noise_public
    }

    pub fn local_port(&self) -> u16 {
        self.listener_addr.port
    }

    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    pub fn accept(
        &self,
    ) -> Result<FacadeSession<<N::TcpListener as TcpListener>::Stream>, AuthError> {
        let start = self.tick.load(Ordering::SeqCst);
        let deadline = start.saturating_add(DEFAULT_DEADLINE_TICKS);
        loop {
            let now = self.tick.fetch_add(1, Ordering::SeqCst).saturating_add(1);
            if now > deadline {
                return Err(AuthError::Io(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "accept timed out",
                )));
            }
            let mut net = self.net.lock();
            net.poll(now);
            match self.listener.lock().accept(Some(deadline)) {
                Ok(stream) => {
                    let mut io = NetTcpIo::new(stream);
                    let (mut transport, device_id) = crate::host::handshake_accept(
                        &self.identity,
                        &self.noise_secret,
                        &self.noise_public,
                        &mut io,
                    )?;
                    let request_id = crate::host::receive_connect_request(&mut io, &mut transport)?;
                    if request_id != device_id.as_str() {
                        crate::host::send_connect_response(&mut io, &mut transport, false)?;
                        return Err(AuthError::Identity("device mismatch".into()));
                    }
                    crate::host::send_connect_response(&mut io, &mut transport, true)?;
                    return Ok(FacadeSession {
                        io,
                        transport,
                        remote_device: device_id,
                    });
                }
                Err(NetError::WouldBlock) => {
                    std::thread::yield_now();
                    continue;
                }
                Err(NetError::TimedOut) => {
                    return Err(AuthError::Io(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "accept timed out",
                    )))
                }
                Err(e) => return Err(AuthError::Io(neterr_to_io(e))),
            }
        }
    }

    pub fn connect(
        &self,
        announcement: &Announcement,
    ) -> Result<FacadeSession<N::TcpStream>, AuthError> {
        let start = self.tick.load(Ordering::SeqCst);
        let deadline = start.saturating_add(DEFAULT_DEADLINE_TICKS);
        let remote = NetSocketAddrV4 {
            ip: nexus_net::NetIpAddrV4([127, 0, 0, 1]),
            port: announcement.port(),
        };

        let stream = loop {
            let now = self.tick.fetch_add(1, Ordering::SeqCst).saturating_add(1);
            if now > deadline {
                return Err(AuthError::Io(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "connect timed out",
                )));
            }
            let mut net = self.net.lock();
            net.poll(now);
            match net.tcp_connect(remote, Some(deadline)) {
                Ok(s) => break s,
                Err(NetError::WouldBlock) => {
                    std::thread::yield_now();
                    continue;
                }
                Err(NetError::TimedOut) => {
                    return Err(AuthError::Io(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "connect timed out",
                    )))
                }
                Err(e) => return Err(AuthError::Io(neterr_to_io(e))),
            }
        };

        let mut io = NetTcpIo::new(stream);
        let (mut transport, device_id) = crate::host::handshake_connect(
            &self.identity,
            &self.noise_secret,
            &self.noise_public,
            announcement,
            &mut io,
        )?;
        crate::host::send_connect_request(&mut io, &mut transport, self.identity.device_id())?;
        let ok = crate::host::receive_connect_response(&mut io, &mut transport)?;
        if !ok {
            return Err(AuthError::Identity("connection rejected".into()));
        }
        Ok(FacadeSession {
            io,
            transport,
            remote_device: device_id,
        })
    }

    // Note: we intentionally model deadlines as ticks (deterministic host-first).
}

pub struct FacadeSession<S: TcpStream> {
    io: NetTcpIo<S>,
    transport: snow::TransportState,
    remote_device: DeviceId,
}

impl<S: TcpStream + Send + 'static> Session for FacadeSession<S> {
    type Stream = FacadeStream<S>;

    fn remote_device_id(&self) -> &DeviceId {
        &self.remote_device
    }

    fn into_stream(self) -> Result<Self::Stream, SessionError> {
        Ok(FacadeStream {
            io: self.io,
            transport: self.transport,
        })
    }
}

pub struct FacadeStream<S: TcpStream> {
    io: NetTcpIo<S>,
    transport: snow::TransportState,
}

impl<S: TcpStream> Stream for FacadeStream<S> {
    fn send(&mut self, channel: u32, payload: &[u8]) -> Result<(), StreamError> {
        let frame = crate::host::serialize_frame(channel, payload)?;
        let encrypted = crate::host::encrypt_payload(&mut self.transport, &frame)?;
        crate::host::write_frame(&mut self.io, &encrypted).map_err(StreamError::from)
    }

    fn recv(&mut self) -> Result<Option<crate::FramePayload>, StreamError> {
        let frame = crate::host::read_frame(&mut self.io).map_err(StreamError::from)?;
        let bytes = crate::host::decrypt_payload(&mut self.transport, &frame)?;
        crate::host::deserialize_frame(&bytes).map(Some)
    }
}
