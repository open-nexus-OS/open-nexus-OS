// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Userspace networking contract v1 (minimal sockets facade)
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 8 unit tests
//!
//! PUBLIC API:
//!   - NetError: cross-backend error model for minimal sockets facade
//!   - NetIpAddr/NetSocketAddr: basic address types (IPv4-only v1)
//!   - Buffer bounds: MAX_UDP_DATAGRAM_BYTES, MAX_TCP_WRITE_BYTES
//!   - Sockets facade traits: NetStack, UdpSocket, TcpListener, TcpStream
//!   - Fake backend (host tests): fake::FakeNet
//!
//! TEST_SCENARIOS (implemented):
//!   - udp_payload_is_bounded()
//!   - tcp_write_is_bounded()
//!   - socket_addr_constructor_is_stable()
//!   - fake_udp_delivers_datagrams_deterministically()
//!   - fake_tcp_connect_and_accept()
//!   - fake_accept_respects_deadline()
//!   - fake_read_respects_deadline()
//!   - fake_udp_multibind_broadcasts_to_all_receivers()
//!
//! ADR: docs/adr/0005-dsoftbus-architecture.md

#![cfg_attr(nexus_env = "os", no_std)]
#![forbid(unsafe_code)]

use core::fmt;

/// Max UDP payload size supported by the v1 facade (bounded by contract).
pub const MAX_UDP_DATAGRAM_BYTES: usize = 1500;

/// Max bytes accepted by a single TCP write call in the v1 facade.
pub const MAX_TCP_WRITE_BYTES: usize = 16 * 1024;

/// Monotonic time used by the facade (contract-only, backend-defined units).
pub type NetInstant = u64;

/// IPv4 address (v1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NetIpAddrV4(pub [u8; 4]);

/// Socket address (IPv4, v1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NetSocketAddrV4 {
    pub ip: NetIpAddrV4,
    pub port: u16,
}

/// Minimal sockets facade error model (v1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetError {
    /// Feature is not available in this build/backend.
    Unsupported,
    /// Operation would block; caller should poll/drive progress and retry.
    WouldBlock,
    /// Deadline expired.
    TimedOut,
    /// Input was invalid (address/length/state).
    InvalidInput(&'static str),
    /// Bind failed due to address/port in use.
    AddrInUse,
    /// TCP stream is not connected.
    NotConnected,
    /// Peer disconnected/connection reset observed.
    Disconnected,
    /// Bounded internal resources exhausted (explicit backpressure).
    NoBufs,
    /// Unexpected internal failure; must not be used to hide normal flow control.
    Internal(&'static str),
}

impl fmt::Display for NetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetError::Unsupported => write!(f, "unsupported"),
            NetError::WouldBlock => write!(f, "would block"),
            NetError::TimedOut => write!(f, "timed out"),
            NetError::InvalidInput(msg) => write!(f, "invalid input: {msg}"),
            NetError::AddrInUse => write!(f, "address in use"),
            NetError::NotConnected => write!(f, "not connected"),
            NetError::Disconnected => write!(f, "disconnected"),
            NetError::NoBufs => write!(f, "no buffers"),
            NetError::Internal(msg) => write!(f, "internal error: {msg}"),
        }
    }
}

#[cfg(nexus_env = "host")]
impl std::error::Error for NetError {}

impl NetSocketAddrV4 {
    pub fn new(ip: [u8; 4], port: u16) -> Self {
        Self { ip: NetIpAddrV4(ip), port }
    }
}

pub fn validate_udp_payload_len(len: usize) -> Result<(), NetError> {
    if len > MAX_UDP_DATAGRAM_BYTES {
        return Err(NetError::InvalidInput("udp payload too large"));
    }
    Ok(())
}

pub fn validate_tcp_write_len(len: usize) -> Result<(), NetError> {
    if len > MAX_TCP_WRITE_BYTES {
        return Err(NetError::InvalidInput("tcp write too large"));
    }
    Ok(())
}

/// Minimal sockets facade (contract-level). Backends must be deterministic and bounded.
pub trait NetStack {
    type Udp: UdpSocket;
    type TcpListener: TcpListener;
    type TcpStream: TcpStream;

    /// Drives networking progress. Must be safe to call frequently.
    fn poll(&mut self, now: NetInstant);

    /// Optional hint for when the next poll is required.
    fn next_wake(&self) -> Option<NetInstant>;

    fn udp_bind(&mut self, local: NetSocketAddrV4) -> Result<Self::Udp, NetError>;
    fn tcp_listen(
        &mut self,
        local: NetSocketAddrV4,
        backlog: usize,
    ) -> Result<Self::TcpListener, NetError>;
    fn tcp_connect(
        &mut self,
        remote: NetSocketAddrV4,
        deadline: Option<NetInstant>,
    ) -> Result<Self::TcpStream, NetError>;
}

pub trait UdpSocket {
    fn local_addr(&self) -> NetSocketAddrV4;
    fn send_to(&mut self, buf: &[u8], remote: NetSocketAddrV4) -> Result<usize, NetError>;
    fn recv_from(&mut self, buf: &mut [u8]) -> Result<(usize, NetSocketAddrV4), NetError>;
}

pub trait TcpListener {
    type Stream: TcpStream;
    fn local_addr(&self) -> NetSocketAddrV4;
    fn accept(&mut self, deadline: Option<NetInstant>) -> Result<Self::Stream, NetError>;
}

pub trait TcpStream {
    fn read(&mut self, deadline: Option<NetInstant>, buf: &mut [u8]) -> Result<usize, NetError>;
    fn write(&mut self, deadline: Option<NetInstant>, buf: &[u8]) -> Result<usize, NetError>;
    fn close(&mut self);
}

#[cfg(nexus_env = "host")]
pub mod fake;

#[cfg(all(test, nexus_env = "host"))]
mod tests {
    use super::*;
    use crate::fake::{loopback, FakeNet};

    #[test]
    fn udp_payload_is_bounded() {
        assert!(validate_udp_payload_len(MAX_UDP_DATAGRAM_BYTES).is_ok());
        assert_eq!(
            validate_udp_payload_len(MAX_UDP_DATAGRAM_BYTES + 1),
            Err(NetError::InvalidInput("udp payload too large"))
        );
    }

    #[test]
    fn tcp_write_is_bounded() {
        assert!(validate_tcp_write_len(MAX_TCP_WRITE_BYTES).is_ok());
        assert_eq!(
            validate_tcp_write_len(MAX_TCP_WRITE_BYTES + 1),
            Err(NetError::InvalidInput("tcp write too large"))
        );
    }

    #[test]
    fn socket_addr_constructor_is_stable() {
        let addr = NetSocketAddrV4::new([10, 0, 2, 15], 1234);
        assert_eq!(addr.ip.0, [10, 0, 2, 15]);
        assert_eq!(addr.port, 1234);
    }

    #[test]
    fn fake_udp_delivers_datagrams_deterministically() {
        let mut net = FakeNet::new();
        let mut a = net.udp_bind(loopback(0)).expect("udp bind a");
        let mut b = net.udp_bind(loopback(0)).expect("udp bind b");

        let msg = b"hello";
        a.send_to(msg, b.local_addr()).expect("udp send");

        let mut buf = [0u8; 64];
        let (n, from) = b.recv_from(&mut buf).expect("udp recv");
        assert_eq!(n, msg.len());
        assert_eq!(&buf[..n], msg);
        assert_eq!(from, a.local_addr());
    }

    #[test]
    fn fake_tcp_connect_and_accept() {
        let mut net = FakeNet::new();
        let mut listener = net.tcp_listen(loopback(0), 1).expect("listen");
        let addr = listener.local_addr();

        let mut client = net.tcp_connect(addr, None).expect("connect");
        let mut server = listener.accept(None).expect("accept");

        client.write(None, b"ping").expect("client write");
        let mut buf = [0u8; 8];
        let n = server.read(None, &mut buf).expect("server read");
        assert_eq!(&buf[..n], b"ping");
    }

    #[test]
    fn fake_accept_respects_deadline() {
        let mut net = FakeNet::new();
        net.poll(10);
        let mut listener = net.tcp_listen(loopback(0), 1).expect("listen");
        assert!(matches!(listener.accept(Some(5)), Err(NetError::TimedOut)));
    }

    #[test]
    fn fake_read_respects_deadline() {
        let mut net = FakeNet::new();
        net.poll(10);
        let mut listener = net.tcp_listen(loopback(0), 1).expect("listen");
        let addr = listener.local_addr();
        let mut client = net.tcp_connect(addr, None).expect("connect");
        let mut server = listener.accept(None).expect("accept");

        let mut buf = [0u8; 1];
        assert_eq!(server.read(Some(5), &mut buf), Err(NetError::TimedOut));
        client.write(None, b"x").expect("write");
        let n = server.read(None, &mut buf).expect("read");
        assert_eq!(n, 1);
        assert_eq!(buf[0], b'x');
    }

    #[test]
    fn fake_udp_multibind_broadcasts_to_all_receivers() {
        let mut net = FakeNet::new();
        let addr = loopback(0);
        let mut a = net.udp_bind(addr).expect("udp bind a");
        let local = a.local_addr();
        let mut b = net.udp_bind(local).expect("udp bind b");

        let msg = b"hi";
        a.send_to(msg, local).expect("send");

        let mut buf1 = [0u8; 8];
        let mut buf2 = [0u8; 8];
        let (n1, _) = a.recv_from(&mut buf1).expect("recv a");
        let (n2, _) = b.recv_from(&mut buf2).expect("recv b");
        assert_eq!(&buf1[..n1], msg);
        assert_eq!(&buf2[..n2], msg);
    }
}
