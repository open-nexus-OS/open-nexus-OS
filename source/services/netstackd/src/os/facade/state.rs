// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Shared netstackd IPC facade runtime tables and debug flags
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered by netstackd host tests + QEMU netstackd markers
//! ADR: docs/adr/0005-dsoftbus-architecture.md

extern crate alloc;

use alloc::vec::Vec;
use core::marker::PhantomData;

use nexus_net::NetSocketAddrV4;
use nexus_net_os::{OsTcpListener, OsTcpStream, OsUdpSocket};

use crate::os::ipc::handles::StreamId;
use crate::os::loopback::LoopBuf;

/// Hairpinned connections waiting for `accept` on one listener (RFC-0092
/// facade prerequisite): bounded, FIFO, never grows.
pub(crate) const LOOP_PENDING_CAPACITY: usize = 4;

/// FIFO of loop-stream ids parked on a listener until the owner accepts.
#[derive(Clone, Copy, Default)]
pub(crate) struct PendingQueue {
    slots: [Option<StreamId>; LOOP_PENDING_CAPACITY],
}

impl PendingQueue {
    pub(crate) const fn new() -> Self {
        Self { slots: [None; LOOP_PENDING_CAPACITY] }
    }

    pub(crate) fn has_room(&self) -> bool {
        self.slots.iter().any(Option::is_none)
    }

    /// `false` when full (the caller refuses the connect).
    pub(crate) fn push(&mut self, id: StreamId) -> bool {
        match self.slots.iter_mut().find(|s| s.is_none()) {
            Some(slot) => {
                *slot = Some(id);
                true
            }
            None => false,
        }
    }

    /// Oldest first.
    pub(crate) fn pop(&mut self) -> Option<StreamId> {
        let first = self.slots.iter().position(Option::is_some)?;
        let id = self.slots[first].take();
        self.slots.copy_within(first + 1.., first);
        self.slots[LOOP_PENDING_CAPACITY - 1] = None;
        id
    }
}

/// A facade listener: a real NIC-facing smoltcp listener (`Tcp`, also the
/// hairpin target for local connects to the interface address) or a
/// loopback-only one (`Loop`, `127/8` binds — never reachable from the NIC).
pub(crate) enum Listener {
    Tcp { sock: OsTcpListener, port: u16, pending: PendingQueue },
    Loop { port: u16, pending: PendingQueue },
}

impl Listener {
    pub(crate) fn port(&self) -> u16 {
        match self {
            Self::Tcp { port, .. } | Self::Loop { port, .. } => *port,
        }
    }

    pub(crate) fn pending_mut(&mut self) -> &mut PendingQueue {
        match self {
            Self::Tcp { pending, .. } | Self::Loop { pending, .. } => pending,
        }
    }
}

/// TCP or loopback byte stream tracked by the facade.
pub(crate) enum Stream {
    /// Outbound connector stream (created via OP_CONNECT).
    TcpDial(OsTcpStream),
    /// Inbound accepted stream (created via OP_ACCEPT on listener socket).
    TcpAccepted(OsTcpStream),
    /// One end of an in-facade pair (loopback / hairpin): bytes written here
    /// land in the peer's `rx`; `remote` is what `OP_PEER_ADDR` reports;
    /// `peer_closed` turns an empty read into end-of-stream.
    Loop { peer: StreamId, rx: LoopBuf, remote: ([u8; 4], u16), peer_closed: bool },
}

/// Reuses a released stream slot (ids of closed streams are dead) or grows
/// the table — keeps the bump-allocated table from doubling per boot.
pub(crate) fn alloc_stream_slot(streams: &mut Vec<Option<Stream>>) -> usize {
    match streams.iter().position(Option::is_none) {
        Some(i) => i,
        None => {
            streams.push(None);
            streams.len() - 1
        }
    }
}

/// UDP loopback buffer bound to a port.
pub(crate) struct LoopUdp {
    pub rx: LoopBuf,
    pub port: u16,
    pub last_from_port: u16,
}

/// Kernel UDP socket or in-memory loopback UDP.
pub(crate) enum UdpSock {
    Udp(OsUdpSocket),
    Loop(LoopUdp),
}

/// Mutable facade state split out of the IPC loop for Phase-1 de-monolith.
///
/// Ownership Model:
/// - `run_facade_loop` owns a single `FacadeState` instance for the full daemon lifetime.
/// - Each IPC turn constructs a `FacadeContext` that hands exclusive `&mut` access to handlers.
/// - Handle IDs from requests are decoded to typed IDs (`ListenerId`, `StreamId`, `UdpId`) before
///   indexing these tables.
/// - This structure is intentionally single-thread confined and should not be shared.
pub(crate) struct FacadeState {
    pub listeners: Vec<Option<Listener>>,
    pub streams: Vec<Option<Stream>>,
    pub pending_dial: Option<(NetSocketAddrV4, OsTcpStream)>,
    pub udps: Vec<Option<UdpSock>>,
    /// Debug help for TASK-0005: log the first non-loopback TCP connect target we see.
    pub dbg_connect_target_printed: bool,
    pub dbg_loopback_connect_logged: bool,
    pub dbg_udp_bind_logged: bool,
    pub dbg_connect_kick_ok_logged: bool,
    pub dbg_connect_kick_would_block_logged: bool,
    pub dbg_connect_pending_set_logged: bool,
    pub dbg_connect_pending_reused_logged: bool,
    pub dbg_connect_pending_stale_logged: bool,
    pub dbg_connect_status_would_block_logged: bool,
    pub dbg_connect_status_io_logged: bool,
    pub dbg_connect_req_count: u32,
    pub dbg_accept_status_ok_logged: bool,
    pub dbg_accept_status_would_block_logged: bool,
    pub dbg_accept_status_io_logged: bool,
    pub dbg_listen_loopback_logged: bool,
    pub dbg_listen_tcp_logged: bool,
    /// policyd + `@reply` slots for the RFC-0091 seam (resolved once at
    /// facade start; `None` = policyd unreachable ⇒ governed ops refuse).
    pub policy: Option<nexus_ipc::policyd::PolicySlots>,
    /// Admitted (sender, class, addr_class, port, addr) tuples — profiles are
    /// static per boot, so an admit never changes; refusals are NOT cached
    /// (policyd must see every one for audit/learn). Bounded ring.
    pub admit_cache: crate::os::facade::authz::AdmitCache,
    pub _not_send_sync: PhantomData<*const ()>,
}

impl FacadeState {
    pub(crate) fn new() -> Self {
        Self {
            listeners: Vec::with_capacity(4),
            streams: Vec::with_capacity(4),
            pending_dial: None,
            udps: Vec::with_capacity(4),
            dbg_connect_target_printed: false,
            dbg_loopback_connect_logged: false,
            dbg_udp_bind_logged: false,
            dbg_connect_kick_ok_logged: false,
            dbg_connect_kick_would_block_logged: false,
            dbg_connect_pending_set_logged: false,
            dbg_connect_pending_reused_logged: false,
            dbg_connect_pending_stale_logged: false,
            dbg_connect_status_would_block_logged: false,
            dbg_connect_status_io_logged: false,
            dbg_connect_req_count: 0,
            dbg_accept_status_ok_logged: false,
            dbg_accept_status_would_block_logged: false,
            dbg_accept_status_io_logged: false,
            dbg_listen_loopback_logged: false,
            dbg_listen_tcp_logged: false,
            policy: None,
            admit_cache: crate::os::facade::authz::AdmitCache::new(),
            _not_send_sync: PhantomData,
        }
    }
}
