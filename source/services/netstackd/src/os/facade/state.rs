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

/// Loopback TCP listener slot (in-process pairing).
pub(crate) enum Listener {
    Tcp(OsTcpListener),
    Loop { port: u16, pending: Option<StreamId> },
}

/// TCP or loopback byte stream tracked by the facade.
pub(crate) enum Stream {
    /// Outbound connector stream (created via OP_CONNECT).
    TcpDial(OsTcpStream),
    /// Inbound accepted stream (created via OP_ACCEPT on listener socket).
    TcpAccepted(OsTcpStream),
    Loop {
        peer: StreamId,
        rx: LoopBuf,
    },
}

/// UDP loopback buffer bound to a port.
pub(crate) struct LoopUdp {
    pub rx: LoopBuf,
    pub port: u16,
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
            _not_send_sync: PhantomData,
        }
    }
}
