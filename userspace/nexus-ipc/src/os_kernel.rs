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

// Routing queries can be policy-gated inside init-lite (policyd roundtrip). Keep this comfortably
// above the policyd control-plane deadline to avoid flaky bring-up under QEMU.
const ROUTE_QUERY_TIMEOUT: Duration = Duration::from_secs(8);

fn query_route(target: &str, wait: Wait) -> Result<(u32, u32)> {
    let name = target.as_bytes();
    if name.is_empty() || name.len() > nexus_abi::routing::MAX_SERVICE_NAME_LEN {
        return Err(IpcError::Unsupported);
    }
    // Drain stale responses on the per-service control reply channel.
    //
    // Routing uses a simple request/response frame without a nonce. If a previous ROUTE_RSP is
    // still queued (e.g. due to bring-up scheduling jitter), we'd otherwise consume the wrong
    // response and mis-route future IPC (a boot-killer for CAP_MOVE flows).
    for _ in 0..32 {
        let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 32];
        match nexus_abi::ipc_recv_v1(
            CTRL_RECV_SLOT,
            &mut hdr,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(_) => continue,
            Err(nexus_abi::IpcError::QueueEmpty) => break,
            Err(_) => break,
        }
    }
    let mut req = [0u8; 5 + nexus_abi::routing::MAX_SERVICE_NAME_LEN];
    let req_len =
        nexus_abi::routing::encode_route_get(name, &mut req).ok_or(IpcError::Unsupported)?;

    // Routing v1 has no nonce; avoid long blocking waits. Use NONBLOCK syscalls and an explicit,
    // short per-attempt budget (caller-level retries handle longer waits).
    let start_ns = nexus_abi::nsec().map_err(|_| IpcError::Unsupported)?;
    let per_attempt_ns: u64 = match wait {
        Wait::NonBlocking => 0,
        Wait::Blocking => duration_to_ns(Duration::from_millis(100)),
        Wait::Timeout(d) => {
            core::cmp::min(duration_to_ns(d), duration_to_ns(Duration::from_millis(100)))
        }
    };
    let deadline_ns = start_ns.saturating_add(per_attempt_ns);

    let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, req_len as u32);
    if matches!(wait, Wait::NonBlocking) {
        nexus_abi::ipc_send_v1(
            CTRL_SEND_SLOT,
            &hdr,
            &req[..req_len],
            nexus_abi::IPC_SYS_NONBLOCK,
            0,
        )
        .map(|_| ())
        .map_err(|e| map_send_err(e, wait))?;
    } else {
        let clock = crate::budget::OsClock;
        crate::budget::raw::send_budgeted(&clock, CTRL_SEND_SLOT, &hdr, &req[..req_len], deadline_ns)
            .map_err(|e| match e {
                IpcError::Timeout => IpcError::Timeout,
                other => other,
            })?;
    }

    let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
    let mut buf = [0u8; 32];
    let n = if matches!(wait, Wait::NonBlocking) {
        nexus_abi::ipc_recv_v1(
            CTRL_RECV_SLOT,
            &mut rh,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        )
        .map(|n| n as usize)
        .map_err(|e| map_recv_err(e, wait))?
    } else {
        let clock = crate::budget::OsClock;
        crate::budget::raw::recv_budgeted(&clock, CTRL_RECV_SLOT, &mut rh, &mut buf, deadline_ns)
            .map_err(|e| match e {
                IpcError::Timeout => IpcError::Timeout,
                other => other,
            })?
    };
    let (status, send_slot, recv_slot) =
        nexus_abi::routing::decode_route_rsp(&buf[..n]).ok_or(IpcError::Unsupported)?;
    if status != nexus_abi::routing::STATUS_OK {
        return Err(IpcError::Unsupported);
    }
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
    d.as_secs().saturating_mul(1_000_000_000).saturating_add(d.subsec_nanos() as u64)
}

fn map_send_err(err: nexus_abi::IpcError, wait: Wait) -> IpcError {
    match err {
        nexus_abi::IpcError::QueueFull if matches!(wait, Wait::NonBlocking) => IpcError::WouldBlock,
        nexus_abi::IpcError::TimedOut => IpcError::Timeout,
        nexus_abi::IpcError::NoSpace => IpcError::NoSpace,
        other => IpcError::Kernel(other),
    }
}

fn map_recv_err(err: nexus_abi::IpcError, wait: Wait) -> IpcError {
    match err {
        nexus_abi::IpcError::QueueEmpty if matches!(wait, Wait::NonBlocking) => {
            IpcError::WouldBlock
        }
        nexus_abi::IpcError::TimedOut => IpcError::Timeout,
        nexus_abi::IpcError::NoSpace => IpcError::NoSpace,
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
        Ok(Self { send_slot: 0, recv_slot: 0 })
    }

    /// Creates a client for a specific target.
    pub fn new_for(target: &str) -> Result<Self> {
        let (send_slot, recv_slot) = query_route(target, Wait::Timeout(ROUTE_QUERY_TIMEOUT))?;
        Ok(Self { send_slot, recv_slot })
    }

    /// Creates a client using explicit capability slot numbers for send/recv.
    pub fn new_with_slots(send_slot: u32, recv_slot: u32) -> Result<Self> {
        Ok(Self { send_slot, recv_slot })
    }

    /// Returns the raw capability slots backing this client (send_slot, recv_slot).
    ///
    /// This is intended for low-level bring-up tests.
    pub fn slots(&self) -> (u32, u32) {
        (self.send_slot, self.recv_slot)
    }

    /// Sends a frame and moves one capability alongside the message.
    ///
    /// `cap_slot_to_move` is a cap slot in the caller that will be consumed by the kernel and
    /// delivered to the receiver.
    pub fn send_with_cap_move(&self, frame: &[u8], cap_slot_to_move: u32) -> Result<()> {
        self.send_with_cap_move_wait(frame, cap_slot_to_move, Wait::NonBlocking)
    }

    /// Sends a frame and moves one capability alongside the message, with the given wait policy.
    pub fn send_with_cap_move_wait(
        &self,
        frame: &[u8],
        cap_slot_to_move: u32,
        wait: Wait,
    ) -> Result<()> {
        let (flags, deadline_ns) = wait_to_sys(wait)?;
        let hdr = nexus_abi::MsgHeader::new(
            cap_slot_to_move,
            0,
            0,
            nexus_abi::ipc_hdr::CAP_MOVE,
            frame.len() as u32,
        );
        nexus_abi::ipc_send_v1(self.send_slot, &hdr, frame, flags, deadline_ns)
            .map(|_| ())
            .map_err(|e| map_send_err(e, wait))
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
pub struct KernelServer {
    recv_slot: u32,
    send_slot: u32,
}

/// Reply capability passed via CAP_MOVE (one-shot).
pub struct ReplyCap {
    slot: u32,
}

impl ReplyCap {
    /// Returns the underlying capability slot number in the receiver.
    pub fn slot(&self) -> u32 {
        self.slot
    }

    /// Closes the reply capability without sending.
    pub fn close(self) {
        let _ = nexus_abi::cap_close(self.slot);
    }

    /// Sends `frame` on the reply cap and then closes it (one-shot).
    pub fn reply_and_close(self, frame: &[u8]) -> Result<()> {
        KernelServer::send_on_cap_wait(self.slot, frame, Wait::NonBlocking)?;
        let _ = nexus_abi::cap_close(self.slot);
        Ok(())
    }

    /// Sends `frame` on the reply cap (with wait policy) and then closes it (one-shot).
    pub fn reply_and_close_wait(self, frame: &[u8], wait: Wait) -> Result<()> {
        KernelServer::send_on_cap_wait(self.slot, frame, wait)?;
        let _ = nexus_abi::cap_close(self.slot);
        Ok(())
    }
}

impl KernelServer {
    /// Creates a server handle for kernel IPC.
    ///
    /// NOTE: Defaults to bootstrap endpoint (slot 0), which is only useful for selftests.
    pub fn new() -> Result<Self> {
        Ok(Self { recv_slot: 0, send_slot: 0 })
    }

    /// Creates a server using explicit capability slot numbers for recv/send.
    pub fn new_with_slots(recv_slot: u32, send_slot: u32) -> Result<Self> {
        Ok(Self { recv_slot, send_slot })
    }

    /// Creates a server bound to a named service target.
    pub fn new_for(service: &str) -> Result<Self> {
        // Routing reply is (send_slot, recv_slot) from the caller's perspective.
        let (send_slot, recv_slot) = query_route(service, Wait::Timeout(ROUTE_QUERY_TIMEOUT))?;
        Self::new_with_slots(recv_slot, send_slot)
    }

    /// Returns the raw capability slots backing this server (recv_slot, send_slot).
    ///
    /// This is intended for low-level bring-up services that want to avoid heap allocations.
    pub fn slots(&self) -> (u32, u32) {
        (self.recv_slot, self.send_slot)
    }

    /// Receives a frame and returns it alongside the raw kernel IPC header.
    ///
    /// If the sender used CAP_MOVE, the returned header's `src` contains the allocated cap slot
    /// in the receiver.
    pub fn recv_with_header(&self, wait: Wait) -> Result<(nexus_abi::MsgHeader, Vec<u8>)> {
        let (flags, deadline_ns) = wait_to_sys(wait)?;
        let sys_flags = flags | nexus_abi::IPC_SYS_TRUNCATE;
        let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 512];
        let n = nexus_abi::ipc_recv_v1(self.recv_slot, &mut hdr, &mut buf, sys_flags, deadline_ns)
            .map_err(|e| map_recv_err(e, wait))?;
        let n = n as usize;
        let mut out = Vec::with_capacity(n);
        out.extend_from_slice(&buf[..n]);
        Ok((hdr, out))
    }

    /// Receives a frame and returns it alongside the raw kernel IPC header and sender service id.
    ///
    /// The sender service id is derived by the kernel at `exec_v2` time and attached to each
    /// message at send-time (cannot be spoofed by the sender).
    pub fn recv_with_header_meta(
        &self,
        wait: Wait,
    ) -> Result<(nexus_abi::MsgHeader, u64, Vec<u8>)> {
        let (flags, deadline_ns) = wait_to_sys(wait)?;
        let sys_flags = flags | nexus_abi::IPC_SYS_TRUNCATE;
        let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut sid: u64 = 0;
        let mut buf = [0u8; 512];
        let n = nexus_abi::ipc_recv_v2(
            self.recv_slot,
            &mut hdr,
            &mut buf,
            &mut sid,
            sys_flags,
            deadline_ns,
        )
        .map_err(|e| map_recv_err(e, wait))?;
        let n = n as usize;
        let mut out = Vec::with_capacity(n);
        out.extend_from_slice(&buf[..n]);
        Ok((hdr, sid, out))
    }

    /// Receives a request and (optionally) a one-shot reply capability moved with the message.
    pub fn recv_request(&self, wait: Wait) -> Result<(Vec<u8>, Option<ReplyCap>)> {
        let (hdr, frame) = self.recv_with_header(wait)?;
        if (hdr.flags & nexus_abi::ipc_hdr::CAP_MOVE) != 0 {
            Ok((frame, Some(ReplyCap { slot: hdr.src })))
        } else {
            Ok((frame, None))
        }
    }

    /// Receives a request and returns the kernel-derived sender service id alongside the frame.
    ///
    /// If the sender used CAP_MOVE, a one-shot reply capability is returned (to be replied on and
    /// closed by the callee).
    pub fn recv_request_with_meta(&self, wait: Wait) -> Result<(Vec<u8>, u64, Option<ReplyCap>)> {
        let (hdr, sid, frame) = self.recv_with_header_meta(wait)?;
        if (hdr.flags & nexus_abi::ipc_hdr::CAP_MOVE) != 0 {
            Ok((frame, sid, Some(ReplyCap { slot: hdr.src })))
        } else {
            Ok((frame, sid, None))
        }
    }

    /// Receives a request into a caller-provided buffer and returns:
    /// `(frame_len, sender_service_id, reply_cap_if_cap_move)`.
    ///
    /// This is the preferred API for os-lite services that use a bump allocator: it avoids
    /// per-message heap allocations (which would otherwise monotonically consume heap).
    pub fn recv_request_with_meta_into(
        &self,
        wait: Wait,
        out: &mut [u8],
    ) -> Result<(usize, u64, Option<ReplyCap>)> {
        let (flags, deadline_ns) = wait_to_sys(wait)?;
        let sys_flags = flags | nexus_abi::IPC_SYS_TRUNCATE;
        let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut sid: u64 = 0;
        let n = nexus_abi::ipc_recv_v2(
            self.recv_slot,
            &mut hdr,
            out,
            &mut sid,
            sys_flags,
            deadline_ns,
        )
        .map_err(|e| map_recv_err(e, wait))? as usize;
        let n = core::cmp::min(n, out.len());
        let reply = if (hdr.flags & nexus_abi::ipc_hdr::CAP_MOVE) != 0 {
            Some(ReplyCap { slot: hdr.src })
        } else {
            None
        };
        Ok((n, sid, reply))
    }

    /// Sends a frame on an arbitrary endpoint capability slot (e.g. one received via CAP_MOVE).
    pub fn send_on_cap(cap_slot: u32, frame: &[u8]) -> Result<()> {
        Self::send_on_cap_wait(cap_slot, frame, Wait::NonBlocking)
    }

    /// Sends a frame on an arbitrary endpoint capability slot (e.g. one received via CAP_MOVE),
    /// using the given wait policy.
    pub fn send_on_cap_wait(cap_slot: u32, frame: &[u8], wait: Wait) -> Result<()> {
        let (flags, deadline_ns) = wait_to_sys(wait)?;
        let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, frame.len() as u32);
        nexus_abi::ipc_send_v1(cap_slot, &hdr, frame, flags, deadline_ns)
            .map(|_| ())
            .map_err(|e| map_send_err(e, wait))
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
