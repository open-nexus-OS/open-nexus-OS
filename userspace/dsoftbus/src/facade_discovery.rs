// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Discovery backend over the nexus-net sockets facade (host-first, deterministic)
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered by dsoftbus integration tests (facade discovery)
//!
//! TEST_SCENARIOS (implemented):
//!   - `userspace/dsoftbus/tests/facade_discovery.rs`: announce + watch yields deterministic announcement
//!   - `userspace/dsoftbus/tests/facade_discovery.rs`: watch seeds cache deterministically
//!   - `userspace/dsoftbus/tests/facade_discovery.rs`: multi-announce yields multiple peers deterministically
//!
//! ADR: docs/adr/0005-dsoftbus-architecture.md

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;

use identity::DeviceId;
use nexus_net::{NetError, NetInstant, NetSocketAddrV4, NetStack, UdpSocket};

use crate::discovery_packet::{decode_announce_v1, encode_announce_v1, AnnounceV1};
use crate::{Announcement, Discovery, DiscoveryError};

fn to_v4(addr: SocketAddr) -> Result<NetSocketAddrV4, DiscoveryError> {
    match addr.ip() {
        std::net::IpAddr::V4(v4) => Ok(NetSocketAddrV4::new(v4.octets(), addr.port())),
        std::net::IpAddr::V6(_) => Err(DiscoveryError::Registry("ipv6 unsupported".into())),
    }
}

/// Discovery backend that sends/receives the announce packet over a UDP socket from a `nexus-net`
/// backend (e.g. `FakeNet` in host tests).
pub struct FacadeDiscovery<N>
where
    N: NetStack + Send + 'static,
    N::Udp: Send + 'static,
{
    net: Arc<Mutex<N>>,
    socket: Arc<Mutex<N::Udp>>,
    bus: NetSocketAddrV4,
    tick: Arc<AtomicU64>,
    cache: Arc<Mutex<HashMap<String, Announcement>>>,
}

impl<N> FacadeDiscovery<N>
where
    N: NetStack + Send + 'static,
    N::Udp: Send + 'static,
{
    pub fn new(net: N, bind: SocketAddr, bus: SocketAddr) -> Result<Self, DiscoveryError> {
        let mut net = net;
        let local = to_v4(bind)?;
        let socket = net
            .udp_bind(local)
            .map_err(|e| DiscoveryError::Registry(format!("udp_bind: {e}")))?;
        Ok(Self {
            net: Arc::new(Mutex::new(net)),
            socket: Arc::new(Mutex::new(socket)),
            bus: to_v4(bus)?,
            tick: Arc::new(AtomicU64::new(0)),
            cache: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    fn poll_tick(&self) -> NetInstant {
        let now = self.tick.fetch_add(1, Ordering::SeqCst).saturating_add(1);
        self.net.lock().poll(now);
        now
    }
}

pub struct FacadeAnnouncementStream<N: NetStack> {
    discovery: FacadeDiscovery<N>,
    remaining_ticks: u64,
    seeded: VecDeque<Announcement>,
}

impl<N> Iterator for FacadeAnnouncementStream<N>
where
    N: NetStack + Send + 'static,
    N::Udp: Send + 'static,
{
    type Item = Announcement;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(ann) = self.seeded.pop_front() {
            return Some(ann);
        }
        let mut buf = vec![0u8; nexus_net::MAX_UDP_DATAGRAM_BYTES];
        while self.remaining_ticks > 0 {
            self.remaining_ticks -= 1;
            self.discovery.poll_tick();

            match self.discovery.socket.lock().recv_from(&mut buf) {
                Ok((n, _from)) => {
                    let bytes = &buf[..n];
                    let pkt = match decode_announce_v1(bytes) {
                        Ok(p) => p,
                        Err(_) => continue, // malformed packets are ignored deterministically
                    };

                    let device_id = match DeviceId::from_hex_sha256(&pkt.device_id) {
                        Ok(id) => id,
                        Err(_) => continue,
                    };

                    let ann = Announcement::new(device_id, pkt.services, pkt.port, pkt.noise_static);
                    self.discovery
                        .cache
                        .lock()
                        .insert(ann.device_id().as_str().to_string(), ann.clone());
                    return Some(ann);
                }
                Err(NetError::WouldBlock) => {
                    std::thread::yield_now();
                    continue;
                }
                Err(NetError::TimedOut) => return None,
                Err(_) => return None,
            }
        }
        None
    }
}

impl<N> Discovery for FacadeDiscovery<N>
where
    N: NetStack + Send + 'static,
    N::Udp: Send + 'static,
{
    type Error = DiscoveryError;
    type Stream = FacadeAnnouncementStream<N>;

    fn announce(&self, announcement: Announcement) -> Result<(), Self::Error> {
        let pkt = AnnounceV1 {
            device_id: announcement.device_id().as_str().to_string(),
            port: announcement.port(),
            noise_static: *announcement.noise_static(),
            services: announcement.services().to_vec(),
        };
        let bytes = encode_announce_v1(&pkt).map_err(|e| DiscoveryError::Registry(e.to_string()))?;
        self.poll_tick();
        self.socket
            .lock()
            .send_to(&bytes, self.bus)
            .map_err(|e| DiscoveryError::Registry(format!("udp_send_to: {e}")))?;
        Ok(())
    }

    fn get(&self, device: &DeviceId) -> Result<Option<Announcement>, Self::Error> {
        Ok(self.cache.lock().get(device.as_str()).cloned())
    }

    fn watch(&self) -> Result<Self::Stream, Self::Error> {
        // Seed the iterator with the current cache contents in stable order.
        let mut seed: Vec<Announcement> = self.cache.lock().values().cloned().collect();
        seed.sort_by(|a, b| a.device_id().as_str().cmp(b.device_id().as_str()));

        Ok(FacadeAnnouncementStream {
            discovery: FacadeDiscovery {
                net: Arc::clone(&self.net),
                socket: Arc::clone(&self.socket),
                bus: self.bus,
                tick: Arc::clone(&self.tick),
                cache: Arc::clone(&self.cache),
            },
            remaining_ticks: 500,
            seeded: VecDeque::from(seed),
        })
    }
}

