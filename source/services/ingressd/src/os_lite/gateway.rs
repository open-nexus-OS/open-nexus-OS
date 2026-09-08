// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The data plane per open exposure: the NIC-facing listener the
//! gateway bound through the facade (`0.0.0.0:<port>` — the ONE `address =
//! "any"` bind the grammar allows), the accept-side admission (peer address
//! from `OP_PEER_ADDR` → CIDR allow-list → token bucket, each refusal a
//! labelled marker + counter), the loopback backend dial, and the bounded
//! relay that pumps bytes both ways within a per-turn budget. One relay
//! table (≤ 16 links) is allocated per open exposure and lives for the
//! boot (bump allocator).
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! TEST_COVERAGE: QEMU (`ingressd: port open|deny`, `SELFTEST: ingress allow|cidr deny|rate ok`)

use alloc::boxed::Box;

use nexus_metrics::deny_counter::DenyCounter;

use crate::forward::{Link, Relay};
use crate::intent::{Registry, MAX_OPEN_EXPOSURES};
use crate::table::{ExposeEntry, Proto};
use crate::wire::Reason;

use super::netclient::{self, NetErr};
use super::print::Line;
use super::stream::NetStream;

/// Accept polling cadence per exposure (an accept RPC costs the facade a
/// bounded stack poll even when nothing is pending).
const ACCEPT_POLL_NS: u64 = 20_000_000;
/// Bytes moved per link direction per turn.
const PUMP_BUDGET: usize = 2 * netclient::MAX_IO_BYTES;
/// Backend dial retries on `WouldBlock` before the peer is closed.
const BACKEND_DIAL_RETRIES: u32 = 8;
/// Loopback backends only (RFC-0092 §3).
const BACKEND_IP: [u8; 4] = [127, 0, 0, 1];

struct Exposure {
    listener: u32,
    relay: Relay<NetStream, NetStream>,
    next_accept_ns: u64,
    backend_down_logged: bool,
}

/// One slot per table entry (the registry's index space) + the refusal
/// counter (`ingress_denies_total{subject}`, flushed to metricsd ≤ 1/s).
pub(crate) struct Gateway {
    slots: [Option<Box<Exposure>>; MAX_OPEN_EXPOSURES],
    denies: DenyCounter,
}

impl Gateway {
    pub(crate) const fn new() -> Self {
        Self {
            slots: [const { None }; MAX_OPEN_EXPOSURES],
            denies: DenyCounter::new("ingress_denies_total"),
        }
    }

    /// Records one refusal attributed to `subject` and prints its marker.
    pub(crate) fn deny(&mut self, subject: u64, reason: Reason, now_ns: u64) {
        self.denies.note(subject, now_ns);
        Line::prefixed("deny (reason=").push_str(reason.label()).push(b")").emit();
    }

    pub(crate) fn is_open(&self, idx: usize) -> bool {
        self.slots.get(idx).is_some_and(Option::is_some)
    }

    /// Binds the NIC-facing listener for `entry` (idempotent).
    pub(crate) fn open(&mut self, idx: usize, entry: &ExposeEntry) -> Result<(), NetErr> {
        if self.is_open(idx) {
            return Ok(());
        }
        let Some(slot) = self.slots.get_mut(idx) else { return Err(NetErr::Io) };
        // The UDP data plane is TASK-0323: a UDP intent is refused honestly
        // (`port open FAIL`, `STATUS_DENY reason=limit`) — never a TCP
        // listener wearing a udp label.
        if entry.proto != Proto::Tcp {
            return Err(NetErr::Io);
        }
        let listener = netclient::listen([0, 0, 0, 0], entry.port)?;
        *slot = Some(Box::new(Exposure {
            listener,
            relay: Relay::new(),
            next_accept_ns: 0,
            backend_down_logged: false,
        }));
        Ok(())
    }

    /// Releases the exposure's links (streams close at the facade). The
    /// listener itself stays bound until the facade grows a listener-close
    /// op — accepts on it are no longer served, so peers see the close.
    pub(crate) fn close(&mut self, idx: usize) {
        if let Some(slot) = self.slots.get_mut(idx) {
            *slot = None;
        }
    }

    /// One turn: accept pending peers (cadence-limited) and pump every link.
    pub(crate) fn service<const N: usize>(&mut self, reg: &mut Registry<'_, N>, now_ns: u64) {
        for idx in 0..MAX_OPEN_EXPOSURES {
            let Some(entry) = reg.table().get(idx).copied() else { break };
            let Some(exp) = self.slots[idx].as_deref_mut() else { continue };
            let mut refused = None;
            if now_ns >= exp.next_accept_ns {
                exp.next_accept_ns = now_ns.saturating_add(ACCEPT_POLL_NS);
                if let Ok(sid) = netclient::accept(exp.listener) {
                    refused = admit(exp, reg, idx, &entry, sid, now_ns).err();
                }
            }
            exp.relay.pump_all(PUMP_BUDGET);
            if let Some(reason) = refused {
                self.deny(entry.subject_id, reason, now_ns);
            }
        }
    }
}

/// Accept-side admission of one peer stream; `Err(reason)` = refused (the
/// peer stream closes when it drops).
fn admit<const N: usize>(
    exp: &mut Exposure,
    reg: &mut Registry<'_, N>,
    idx: usize,
    entry: &ExposeEntry,
    sid: u32,
    now_ns: u64,
) -> Result<(), Reason> {
    let peer = NetStream::new(sid);
    // An unattributable peer is refused.
    let (ip, _port) = netclient::peer_addr(peer.id()).map_err(|_| Reason::Identity)?;
    reg.admit_peer(idx, ip, now_ns)?;
    if !exp.relay.has_room() {
        return Err(Reason::Limit);
    }
    let mut dial = Err(NetErr::WouldBlock);
    for _ in 0..BACKEND_DIAL_RETRIES {
        dial = netclient::connect(BACKEND_IP, entry.backend);
        if !matches!(dial, Err(NetErr::WouldBlock)) {
            break;
        }
        let _ = nexus_abi::yield_();
    }
    match dial {
        Ok(bid) => {
            let _ = exp.relay.attach(Link::new(peer, NetStream::new(bid)));
            Ok(())
        }
        Err(_) => {
            // Backend down: the accepted connection closes, the exposure
            // stays open (RFC-0092 failure model). Logged once per exposure.
            if !exp.backend_down_logged {
                exp.backend_down_logged = true;
                Line::prefixed("backend down (port=")
                    .push_dec(u64::from(entry.port))
                    .push(b", backend=")
                    .push_dec(u64::from(entry.backend))
                    .push(b")")
                    .emit();
            }
            Ok(())
        }
    }
}
