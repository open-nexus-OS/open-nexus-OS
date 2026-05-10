// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Deterministic in-memory backend for nexus-net contract tests (host-first)
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered by 8 unit tests in `src/lib.rs`
//!
//! TEST_SCENARIOS (implemented, in `src/lib.rs`):
//!   - fake_udp_delivers_datagrams_deterministically()
//!   - fake_udp_multibind_broadcasts_to_all_receivers()
//!   - fake_tcp_connect_and_accept()
//!   - fake_accept_respects_deadline()
//!   - fake_read_respects_deadline()
//!
//! ADR: docs/adr/0005-dsoftbus-architecture.md

#![forbid(unsafe_code)]

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::{
    validate_tcp_write_len, validate_udp_payload_len, NetError, NetInstant, NetIpAddrV4,
    NetSocketAddrV4, NetStack, TcpListener, TcpStream, UdpSocket,
};

type UdpDatagram = (Vec<u8>, NetSocketAddrV4);
type UdpRecvQueue = VecDeque<UdpDatagram>;
type UdpMultiBind = Vec<UdpRecvQueue>;
type UdpState = HashMap<NetSocketAddrV4, UdpMultiBind>;

#[derive(Default)]
struct State {
    // UDP receive queues per bound socket, keyed by local bind address.
    udp: UdpState,
    tcp_listeners: HashMap<NetSocketAddrV4, VecDeque<FakeTcpStream>>,
}

/// Deterministic, bounded in-memory network backend.
///
/// Scope: host-first tests and contract validation. This is not a performance model.
#[derive(Clone, Default)]
pub struct FakeNet {
    state: Arc<Mutex<State>>,
    next_port: Arc<AtomicU16>,
    now: Arc<AtomicU64>,
}

impl FakeNet {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(State::default())),
            next_port: Arc::new(AtomicU16::new(10_000)),
            now: Arc::new(AtomicU64::new(0)),
        }
    }

    fn alloc_ephemeral(&mut self) -> u16 {
        let p = self.next_port.fetch_add(1, Ordering::SeqCst);
        // Ensure we stay non-zero and in a non-privileged range.
        if p < 10_000 {
            self.next_port.store(10_000, Ordering::SeqCst);
            10_000
        } else {
            p
        }
    }
}

impl NetStack for FakeNet {
    type Udp = FakeUdpSocket;
    type TcpListener = FakeTcpListener;
    type TcpStream = FakeTcpStream;

    fn poll(&mut self, now: NetInstant) {
        self.now.store(now, Ordering::SeqCst);
    }

    fn next_wake(&self) -> Option<NetInstant> {
        None
    }

    fn udp_bind(&mut self, mut local: NetSocketAddrV4) -> Result<Self::Udp, NetError> {
        if local.port == 0 {
            local.port = self.alloc_ephemeral();
        }
        let mut s = self.state.lock().map_err(|_| NetError::Internal("poisoned mutex"))?;
        let slots = s.udp.entry(local).or_default();
        let idx = slots.len();
        slots.push(VecDeque::new());
        Ok(FakeUdpSocket { state: Arc::clone(&self.state), local, idx })
    }

    fn tcp_listen(
        &mut self,
        mut local: NetSocketAddrV4,
        _backlog: usize,
    ) -> Result<Self::TcpListener, NetError> {
        if local.port == 0 {
            local.port = self.alloc_ephemeral();
        }
        let mut s = self.state.lock().map_err(|_| NetError::Internal("poisoned mutex"))?;
        if s.tcp_listeners.contains_key(&local) {
            return Err(NetError::AddrInUse);
        }
        s.tcp_listeners.insert(local, VecDeque::new());
        Ok(FakeTcpListener { state: Arc::clone(&self.state), local, now: Arc::clone(&self.now) })
    }

    fn tcp_connect(
        &mut self,
        remote: NetSocketAddrV4,
        deadline: Option<NetInstant>,
    ) -> Result<Self::TcpStream, NetError> {
        let now = self.now.load(Ordering::SeqCst);
        if let Some(dl) = deadline {
            if now > dl {
                return Err(NetError::TimedOut);
            }
        }
        let mut s = self.state.lock().map_err(|_| NetError::Internal("poisoned mutex"))?;
        let queue =
            s.tcp_listeners.get_mut(&remote).ok_or(NetError::InvalidInput("no listener"))?;

        // Create a connected pair (client ↔ server) with bounded mailboxes.
        let a_to_b = Arc::new(Mutex::new(VecDeque::<u8>::new()));
        let b_to_a = Arc::new(Mutex::new(VecDeque::<u8>::new()));
        let closed = Arc::new(AtomicBool::new(false));

        let client = FakeTcpStream {
            rx: Arc::clone(&b_to_a),
            tx: Arc::clone(&a_to_b),
            closed: Arc::clone(&closed),
            now: Arc::clone(&self.now),
        };
        let server = FakeTcpStream { rx: a_to_b, tx: b_to_a, closed, now: Arc::clone(&self.now) };

        queue.push_back(server);
        Ok(client)
    }
}

pub struct FakeUdpSocket {
    state: Arc<Mutex<State>>,
    local: NetSocketAddrV4,
    idx: usize,
}

impl UdpSocket for FakeUdpSocket {
    fn local_addr(&self) -> NetSocketAddrV4 {
        self.local
    }

    fn send_to(&mut self, buf: &[u8], remote: NetSocketAddrV4) -> Result<usize, NetError> {
        validate_udp_payload_len(buf.len())?;
        let mut s = self.state.lock().map_err(|_| NetError::Internal("poisoned mutex"))?;
        let slots =
            s.udp.get_mut(&remote).ok_or(NetError::InvalidInput("udp destination not bound"))?;
        // Broadcast semantics (host-first): deliver to every socket bound to `remote`.
        for q in slots.iter_mut() {
            q.push_back((buf.to_vec(), self.local));
        }
        Ok(buf.len())
    }

    fn recv_from(&mut self, buf: &mut [u8]) -> Result<(usize, NetSocketAddrV4), NetError> {
        let mut s = self.state.lock().map_err(|_| NetError::Internal("poisoned mutex"))?;
        let slots = s.udp.get_mut(&self.local).ok_or(NetError::InvalidInput("udp not bound"))?;
        let q = slots.get_mut(self.idx).ok_or(NetError::Internal("udp socket index"))?;
        match q.pop_front() {
            Some((payload, from)) => {
                if payload.len() > buf.len() {
                    return Err(NetError::NoBufs);
                }
                buf[..payload.len()].copy_from_slice(&payload);
                Ok((payload.len(), from))
            }
            None => Err(NetError::WouldBlock),
        }
    }
}

pub struct FakeTcpListener {
    state: Arc<Mutex<State>>,
    local: NetSocketAddrV4,
    now: Arc<AtomicU64>,
}

impl TcpListener for FakeTcpListener {
    type Stream = FakeTcpStream;

    fn local_addr(&self) -> NetSocketAddrV4 {
        self.local
    }

    fn accept(&mut self, _deadline: Option<NetInstant>) -> Result<Self::Stream, NetError> {
        let now = self.now.load(Ordering::SeqCst);
        if let Some(dl) = _deadline {
            if now > dl {
                return Err(NetError::TimedOut);
            }
        }
        let mut s = self.state.lock().map_err(|_| NetError::Internal("poisoned mutex"))?;
        let q = s
            .tcp_listeners
            .get_mut(&self.local)
            .ok_or(NetError::InvalidInput("listener missing"))?;
        match q.pop_front() {
            Some(stream) => Ok(stream),
            None => Err(NetError::WouldBlock),
        }
    }
}

#[derive(Clone, Debug)]
pub struct FakeTcpStream {
    rx: Arc<Mutex<VecDeque<u8>>>,
    tx: Arc<Mutex<VecDeque<u8>>>,
    closed: Arc<AtomicBool>,
    now: Arc<AtomicU64>,
}

impl TcpStream for FakeTcpStream {
    fn read(&mut self, deadline: Option<NetInstant>, buf: &mut [u8]) -> Result<usize, NetError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(NetError::Disconnected);
        }
        let now = self.now.load(Ordering::SeqCst);
        if let Some(dl) = deadline {
            if now > dl {
                return Err(NetError::TimedOut);
            }
        }
        let mut rx = self.rx.lock().map_err(|_| NetError::Internal("poisoned mutex"))?;
        if rx.is_empty() {
            return Err(NetError::WouldBlock);
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

    fn write(&mut self, deadline: Option<NetInstant>, buf: &[u8]) -> Result<usize, NetError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(NetError::Disconnected);
        }
        let now = self.now.load(Ordering::SeqCst);
        if let Some(dl) = deadline {
            if now > dl {
                return Err(NetError::TimedOut);
            }
        }
        validate_tcp_write_len(buf.len())?;
        let mut tx = self.tx.lock().map_err(|_| NetError::Internal("poisoned mutex"))?;
        for &b in buf {
            tx.push_back(b);
        }
        Ok(buf.len())
    }

    fn close(&mut self) {
        self.closed.store(true, Ordering::SeqCst);
    }
}

impl Drop for FakeTcpStream {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::SeqCst);
    }
}

/// Convenience address helper for tests.
pub fn loopback(port: u16) -> NetSocketAddrV4 {
    NetSocketAddrV4 { ip: NetIpAddrV4([127, 0, 0, 1]), port }
}
