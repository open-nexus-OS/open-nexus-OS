// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Kernel-backed IPC implementation for OS/no_std builds (IPC v1 syscalls)
//! OWNERS: @runtime
//! PUBLIC API: KernelClient, KernelServer, set_default_target, supports_service_routing
//! DEPENDS_ON: nexus-abi (ipc_send_v1/ipc_recv_v1 + nsec), alloc, core
//! INVARIANTS:
//!   - No unsafe code (delegates to nexus-abi wrappers)
//!   - Wait mapping uses kernel IPC v1 (NONBLOCK + deadline semantics)
//!   - Service routing is limited to capabilities pre-distributed by init-lite (RFC-0005)
//! ADR: docs/adr/0003-ipc-runtime-architecture.md

extern crate alloc;

use alloc::vec::Vec;
use core::time::Duration;

use crate::{Client, IpcError, Result, Server, Wait};

/// Sets the default service target for the current context.
///
pub fn set_default_target(name: &str) {
    let _ = name;
}

/// Returns whether kernel-backed IPC runtime can route to named services across processes.
///
/// IMPORTANT: This returns true once init-lite distributes per-service endpoint caps into
/// deterministic slots and the kernel backend knows how to map service names to those slots.
pub fn supports_service_routing() -> bool {
    true
}

const CTRL_SEND_SLOT: u32 = 1; // init-lite transfers control REQ (child SEND) into slot 1.
const CTRL_RECV_SLOT: u32 = 2; // init-lite transfers control RSP (child RECV) into slot 2.
const ROUTE_GET: u8 = 0x40;
const ROUTE_RSP: u8 = 0x41;

fn query_route(target: &str, wait: Wait) -> Result<(u32, u32)> {
    let name = target.as_bytes();
    if name.is_empty() || name.len() > 48 {
        return Err(IpcError::Unsupported);
    }
    let mut req = Vec::with_capacity(2 + name.len());
    req.push(ROUTE_GET);
    req.push(name.len() as u8);
    req.extend_from_slice(name);

    let (flags, deadline_ns) = wait_to_sys(wait)?;
    let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, req.len() as u32);
    nexus_abi::ipc_send_v1(CTRL_SEND_SLOT, &hdr, &req, flags, deadline_ns)
        .map_err(|e| map_send_err(e, wait))?;

    let (flags, deadline_ns) = wait_to_sys(wait)?;
    let sys_flags = flags | nexus_abi::IPC_SYS_TRUNCATE;
    let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
    let mut buf = [0u8; 32];
    let n = nexus_abi::ipc_recv_v1(CTRL_RECV_SLOT, &mut rh, &mut buf, sys_flags, deadline_ns)
        .map_err(|e| map_recv_err(e, wait))? as usize;
    if n < 10 || buf[0] != ROUTE_RSP {
        return Err(IpcError::Unsupported);
    }
    let status = buf[1];
    if status != 0 {
        return Err(IpcError::Unsupported);
    }
    let send_slot = u32::from_le_bytes([buf[2], buf[3], buf[4], buf[5]]);
    let recv_slot = u32::from_le_bytes([buf[6], buf[7], buf[8], buf[9]]);
    Ok((send_slot, recv_slot))
}

fn wait_to_sys(wait: Wait) -> core::result::Result<(u32, u64), IpcError> {
    let (flags, deadline_ns) = match wait {
        Wait::NonBlocking => (nexus_abi::IPC_SYS_NONBLOCK, 0),
        Wait::Blocking => (0, 0),
        Wait::Timeout(d) => {
            let now = nexus_abi::nsec().map_err(|_| IpcError::Unsupported)?;
            (0, now.saturating_add(duration_to_ns(d)))
        }
    };
    Ok((flags, deadline_ns))
}

fn duration_to_ns(d: Duration) -> u64 {
    d.as_secs()
        .saturating_mul(1_000_000_000)
        .saturating_add(d.subsec_nanos() as u64)
}

fn map_send_err(err: nexus_abi::IpcError, wait: Wait) -> IpcError {
    match err {
        nexus_abi::IpcError::QueueFull if matches!(wait, Wait::NonBlocking) => IpcError::WouldBlock,
        nexus_abi::IpcError::TimedOut => IpcError::Timeout,
        other => IpcError::Kernel(other),
    }
}

fn map_recv_err(err: nexus_abi::IpcError, wait: Wait) -> IpcError {
    match err {
        nexus_abi::IpcError::QueueEmpty if matches!(wait, Wait::NonBlocking) => IpcError::WouldBlock,
        nexus_abi::IpcError::TimedOut => IpcError::Timeout,
        other => IpcError::Kernel(other),
    }
}

/// Client backed by kernel IPC v1 syscalls.
pub struct KernelClient {
    send_slot: u32,
    recv_slot: u32,
}

impl KernelClient {
    /// Creates a new client bound to the bootstrap endpoint (slot 0).
    pub fn new() -> Result<Self> {
        Ok(Self {
            send_slot: 0,
            recv_slot: 0,
        })
    }

    /// Creates a client for a specific target.
    pub fn new_for(target: &str) -> Result<Self> {
        let (send_slot, recv_slot) = query_route(target, Wait::Timeout(Duration::from_millis(100)))?;
        Ok(Self { send_slot, recv_slot })
    }

    /// Creates a client using explicit capability slot numbers for send/recv.
    pub fn new_with_slots(send_slot: u32, recv_slot: u32) -> Result<Self> {
        Ok(Self { send_slot, recv_slot })
    }
}

impl Client for KernelClient {
    fn send(&self, frame: &[u8], wait: Wait) -> Result<()> {
        let (flags, deadline_ns) = wait_to_sys(wait)?;
        // Send has no truncate flag.
        let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, frame.len() as u32);
        nexus_abi::ipc_send_v1(self.send_slot, &hdr, frame, flags, deadline_ns)
            .map(|_| ())
            .map_err(|e| map_send_err(e, wait))
    }

    fn recv(&self, wait: Wait) -> Result<Vec<u8>> {
        let (flags, deadline_ns) = wait_to_sys(wait)?;
        let sys_flags = flags | nexus_abi::IPC_SYS_TRUNCATE;
        let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 512];
        let n = nexus_abi::ipc_recv_v1(self.recv_slot, &mut hdr, &mut buf, sys_flags, deadline_ns)
            .map_err(|e| map_recv_err(e, wait))?;
        let n = n as usize;
        let mut out = Vec::with_capacity(n);
        out.extend_from_slice(&buf[..n]);
        Ok(out)
    }
}

/// Server backed by kernel IPC v1 syscalls.
///
/// NOTE: Reply routing is not implemented yet; treat this as a minimal request receiver only.
pub struct KernelServer {
    recv_slot: u32,
    send_slot: u32,
}

impl KernelServer {
    /// Creates a server handle for kernel IPC.
    ///
    /// NOTE: Defaults to bootstrap endpoint (slot 0), which is only useful for selftests.
    pub fn new() -> Result<Self> {
        Ok(Self {
            recv_slot: 0,
            send_slot: 0,
        })
    }

    /// Creates a server using explicit capability slot numbers for recv/send.
    pub fn new_with_slots(recv_slot: u32, send_slot: u32) -> Result<Self> {
        Ok(Self { recv_slot, send_slot })
    }

    /// Creates a server bound to a named service target.
    pub fn new_for(service: &str) -> Result<Self> {
        // Routing reply is (send_slot, recv_slot) from the caller's perspective.
        let (send_slot, recv_slot) = query_route(service, Wait::Timeout(Duration::from_millis(100)))?;
        Self::new_with_slots(recv_slot, send_slot)
    }
}

impl Server for KernelServer {
    fn recv(&self, wait: Wait) -> Result<Vec<u8>> {
        let client = KernelClient::new_with_slots(self.send_slot, self.recv_slot)?;
        client.recv(wait)
    }

    fn send(&self, frame: &[u8], wait: Wait) -> Result<()> {
        let client = KernelClient::new_with_slots(self.send_slot, self.recv_slot)?;
        client.send(frame, wait)
    }
}
