// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Kernel-backed IPC implementation for OS/no_std builds (IPC v1 syscalls)
//! OWNERS: @runtime
//! PUBLIC API: KernelClient, KernelServer, set_default_target, supports_service_routing
//! DEPENDS_ON: nexus-abi (ipc_send_v1/ipc_recv_v1/ipc_recv_v2), alloc, core
//! INVARIANTS:
//!   - No unsafe code (delegates to nexus-abi wrappers)
//!   - Wait maps to kernel IPC v1 NONBLOCK or a park without deadline (no clock, RFC-0093 §7)
//!   - Service routing is limited to capabilities pre-distributed by init-lite (RFC-0005)
//! ADR: docs/adr/0003-ipc-runtime-architecture.md

extern crate alloc;

use alloc::vec::Vec;

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

/// Nonce mismatches tolerated while draining answers meant for a superseded ask.
const ROUTE_NONCE_MISMATCH_BUDGET: u32 = 32;

/// Resolves `target` through init's responder — ONE nonce-correlated ask.
///
/// Routing v1 (nonce-less, plus a 32-frame "drain stale replies" prologue) is gone: without a
/// nonce an answer could be consumed by the wrong waiter, which is why windowd once bound its
/// own inbox as gpud and packagefsd silently fell back to a RAM seed. The nonce makes a
/// mismatched answer detectable, so the drain — and every guard built on top of it — is
/// unnecessary.
fn resolve_route(target: &str) -> Result<(u32, u32)> {
    use crate::budget::{route_with_nonce, NonceMismatchBudget, RouteRetryOutcome};
    let name = target.as_bytes();
    if name.is_empty() || name.len() > nexus_abi::routing::MAX_SERVICE_NAME_LEN {
        return Err(IpcError::Unsupported);
    }
    match route_with_nonce(name, NonceMismatchBudget::new(ROUTE_NONCE_MISMATCH_BUDGET)) {
        RouteRetryOutcome::Success { send_slot, recv_slot } => Ok((send_slot, recv_slot)),
        RouteRetryOutcome::TargetStale => Err(IpcError::Timeout),
        RouteRetryOutcome::NonceMismatchBudgetExceeded | RouteRetryOutcome::Rejected { .. } => {
            Err(IpcError::Unsupported)
        }
        RouteRetryOutcome::Ipc(err) => Err(err),
    }
}

/// The kernel flags for a wait policy. The deadline argument of every syscall below is the
/// literal `0` ("no deadline"): a blocking call parks until the frame arrives or the peer dies.
fn wait_flags(wait: Wait) -> u32 {
    match wait {
        Wait::NonBlocking => nexus_abi::IPC_SYS_NONBLOCK,
        Wait::Blocking => 0,
    }
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
        // RFC-0079: last-sender EOF surfaces as a peer disconnect.
        nexus_abi::IpcError::PeerClosed => IpcError::Disconnected,
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
        let (send_slot, recv_slot) = resolve_route(target)?;
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

    /// Receives a response into a caller-provided buffer, returning the frame length.
    ///
    /// Allocation-free counterpart to [`Client::recv`] (which returns a freshly
    /// allocated `Vec<u8>` per call). Preferred in hot loops on services backed by
    /// a non-freeing bump allocator — e.g. windowd draining gpud present-acks every
    /// frame — where a per-call `Vec` would monotonically consume the heap.
    pub fn recv_into(&self, wait: Wait, out: &mut [u8]) -> Result<usize> {
        self.recv_into_flags(wait, out, false)
    }

    /// Like [`recv_into`](Self::recv_into) but OPTS INTO last-sender EOF
    /// (RFC-0079): once this channel has had a sender and its last SEND cap
    /// closes, a blocking recv returns [`IpcError::Disconnected`] instead of
    /// blocking forever. For dedicated per-connection channels (the app-host
    /// event channel) whose owner should terminate when the peer goes away.
    pub fn recv_into_eof(&self, wait: Wait, out: &mut [u8]) -> Result<usize> {
        self.recv_into_flags(wait, out, true)
    }

    fn recv_into_flags(&self, wait: Wait, out: &mut [u8], eof: bool) -> Result<usize> {
        let flags = wait_flags(wait);
        let mut sys_flags = flags | nexus_abi::IPC_SYS_TRUNCATE;
        if eof {
            sys_flags |= nexus_abi::IPC_SYS_EOF;
        }
        let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let n = nexus_abi::ipc_recv_v1(self.recv_slot, &mut hdr, out, sys_flags, 0)
            .map_err(|e| map_recv_err(e, wait))?;
        Ok(n as usize)
    }
}

impl Client for KernelClient {
    fn send(&self, frame: &[u8], wait: Wait) -> Result<()> {
        let flags = wait_flags(wait);
        // Send has no truncate flag.
        let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, frame.len() as u32);
        nexus_abi::ipc_send_v1(self.send_slot, &hdr, frame, flags, 0)
            .map(|_| ())
            .map_err(|e| map_send_err(e, wait))
    }

    fn recv(&self, wait: Wait) -> Result<Vec<u8>> {
        let flags = wait_flags(wait);
        let sys_flags = flags | nexus_abi::IPC_SYS_TRUNCATE;
        let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 512];
        let n = nexus_abi::ipc_recv_v1(self.recv_slot, &mut hdr, &mut buf, sys_flags, 0)
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

/// One parked reply and the buffer it lives in (TASK-0054C P5c).
///
/// A server answers the PREVIOUS request together with the wait for the next
/// one, so the answer has to outlive the iteration that produced it. One buffer
/// for the service's lifetime, because the os-lite heap never frees. Nothing
/// parked means the last requester moved no capability — P2-c's rule, carried
/// through the merge rather than re-stated at every call site.
pub struct PendingReply {
    cap: Option<ReplyCap>,
    buf: Vec<u8>,
    len: usize,
}

impl Default for PendingReply {
    fn default() -> Self {
        Self::new()
    }
}

impl PendingReply {
    /// A loop's parked-reply slot, sized by the transport cap so it can never
    /// truncate an answer the kernel would have carried.
    pub fn new() -> Self {
        Self { cap: None, buf: alloc::vec![0u8; nexus_abi::IPC_PAYLOAD_MAX], len: 0 }
    }

    /// Park `bytes` to be sent with the next receive.
    pub fn park(&mut self, cap: ReplyCap, bytes: &[u8]) {
        self.len = core::cmp::min(bytes.len(), self.buf.len());
        self.buf[..self.len].copy_from_slice(&bytes[..self.len]);
        self.cap = Some(cap);
    }
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
        let (send_slot, recv_slot) = resolve_route(service)?;
        Self::new_with_slots(recv_slot, send_slot)
    }

    /// Returns the raw capability slots backing this server (recv_slot, send_slot).
    ///
    /// This is intended for low-level bring-up services that want to avoid heap allocations.
    pub fn slots(&self) -> (u32, u32) {
        (self.recv_slot, self.send_slot)
    }

    /// The server loop's ONE step: answer the previous request, if one is parked,
    /// and wait for the next — in a single trap (TASK-0054C P5c).
    ///
    /// Eight service loops had spelled this out identically; a shape repeated
    /// eight times is a shape with one home. A parked reply means the previous
    /// requester moved a capability; nothing parked means it did not, and then
    /// this is an ordinary receive — which is how P2-c's rule ("a server answers
    /// exactly the senders that moved a reply capability") survives the merge.
    /// `wait` applies to the RECEIVE half only; a parked reply always goes out
    /// (its trap carries the receive with it, so there is nothing to defer).
    pub fn serve_next(
        &self,
        pending: &mut PendingReply,
        wait: Wait,
        out: &mut [u8],
    ) -> Result<(nexus_abi::MsgHeader, usize, u64, Option<ReplyCap>)> {
        match pending.cap.take() {
            Some(cap) => {
                self.reply_and_recv_with_header_into(cap, &pending.buf[..pending.len], out)
            }
            None => self.recv_request_with_header_into(wait, out),
        }
    }

    /// Answer the current request and wait for the next one in ONE trap
    /// (TASK-0054C P5b-2, syscall 59).
    ///
    /// Today a served request costs THREE kernel entries: `send_on_cap` for the
    /// reply, `cap_close` for the one-shot capability, and the next receive.
    /// This does all three. `reply` is consumed exactly as
    /// [`ReplyCap::reply_and_close`] consumes it, and the return shape is the
    /// one [`recv_request_with_header_into`](Self::recv_request_with_header_into)
    /// gives, so a loop swaps one call for the other.
    pub fn reply_and_recv_with_header_into(
        &self,
        reply: ReplyCap,
        response: &[u8],
        out: &mut [u8],
    ) -> Result<(nexus_abi::MsgHeader, usize, u64, Option<ReplyCap>)> {
        let reply_hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, response.len() as u32);
        let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut sid: u64 = 0;
        let n = nexus_abi::ipc_reply_recv(
            reply.slot,
            &reply_hdr,
            response,
            self.recv_slot,
            &mut hdr,
            out,
            &mut sid,
        )
        .map_err(|e| map_recv_err(e, Wait::Blocking))?;
        let n = core::cmp::min(n, out.len());
        let next = if (hdr.flags & nexus_abi::ipc_hdr::CAP_MOVE) != 0 {
            Some(ReplyCap { slot: hdr.src })
        } else {
            None
        };
        Ok((hdr, n, sid, next))
    }

    /// Like [`recv_request_with_meta_into`](Self::recv_request_with_meta_into) but also hands
    /// back the message HEADER, whose `dst` carries the kernel-attested sender PID.
    ///
    /// The allocation-free counterpart of the deleted `recv_with_header_meta`: samgrd echoes
    /// that PID (`OP_SENDER_PID`) and execd serves on it, and both were paying a `Vec` per
    /// request on a heap that never frees to get it (TASK-0054C P5b).
    pub fn recv_request_with_header_into(
        &self,
        wait: Wait,
        out: &mut [u8],
    ) -> Result<(nexus_abi::MsgHeader, usize, u64, Option<ReplyCap>)> {
        let flags = wait_flags(wait);
        let sys_flags = flags | nexus_abi::IPC_SYS_TRUNCATE;
        let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut sid: u64 = 0;
        let n = nexus_abi::ipc_recv_v2(self.recv_slot, &mut hdr, out, &mut sid, sys_flags, 0)
            .map_err(|e| map_recv_err(e, wait))? as usize;
        let n = core::cmp::min(n, out.len());
        let reply = if (hdr.flags & nexus_abi::ipc_hdr::CAP_MOVE) != 0 {
            Some(ReplyCap { slot: hdr.src })
        } else {
            None
        };
        Ok((hdr, n, sid, reply))
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
        let flags = wait_flags(wait);
        let sys_flags = flags | nexus_abi::IPC_SYS_TRUNCATE;
        let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut sid: u64 = 0;
        let n = nexus_abi::ipc_recv_v2(self.recv_slot, &mut hdr, out, &mut sid, sys_flags, 0)
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
        let flags = wait_flags(wait);
        let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, frame.len() as u32);
        nexus_abi::ipc_send_v1(cap_slot, &hdr, frame, flags, 0)
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
