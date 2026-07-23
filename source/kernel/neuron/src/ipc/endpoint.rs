// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: one IPC endpoint's queue + waiter lists — the per-endpoint state
//! the `Router` (parent) manages. Split out of `ipc/mod.rs` (RFC-0079,
//! module-size ratchet). Fields/methods are `pub(super)` so only the Router
//! touches them. The `had_sender` latch drives RFC-0079 last-sender EOF.
//! OWNERS: @kernel-ipc-team
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: exercised via Router host tests in `ipc/mod.rs`

use super::{IpcError, Message, WaiterId};
use alloc::collections::VecDeque;
use alloc::vec::Vec;

#[derive(Default)]
pub(super) struct Endpoint {
    pub(super) queue: VecDeque<Message>,
    pub(super) depth: usize,
    pub(super) queued_bytes: usize,
    pub(super) max_queued_bytes: usize,
    pub(super) owner: Option<WaiterId>,
    pub(super) alive: bool,
    pub(super) recv_waiters: VecDeque<WaiterId>,
    pub(super) send_waiters: VecDeque<WaiterId>,
    /// RFC-0079: monotonic latch — set once a sender has been OBSERVED (a
    /// successful send, or a recv-block scan that found a live SEND cap).
    /// Never cleared. Last-sender EOF requires this true, so an endpoint that
    /// never had a sender can never wrongly EOF (a server blocking at boot).
    pub(super) had_sender: bool,
}

impl Endpoint {
    pub(super) fn with_depth(depth: usize, owner: Option<WaiterId>) -> Self {
        // Byte-based DoS hardening: in addition to queue depth, cap the total bytes that can be
        // buffered in an endpoint. This keeps memory use bounded even if messages are large.
        //
        // NOTE: Payloads are already bounded at syscall entry (MAX_FRAME_BYTES); this compounds
        // that bound over the queue depth.
        const MAX_FRAME_BYTES: usize = 8 * 1024;
        let max_queued_bytes = depth.saturating_mul(MAX_FRAME_BYTES);
        Self {
            queue: VecDeque::new(),
            depth,
            queued_bytes: 0,
            max_queued_bytes,
            owner,
            alive: true,
            recv_waiters: VecDeque::new(),
            send_waiters: VecDeque::new(),
            had_sender: false,
        }
    }

    pub(super) fn push(&mut self, msg: Message) -> core::result::Result<(), (IpcError, Message)> {
        if !self.alive {
            return Err((IpcError::NoSuchEndpoint, msg));
        }
        if self.queue.len() >= self.depth {
            return Err((IpcError::QueueFull, msg));
        }
        let len = msg.payload.len();
        if self.queued_bytes.saturating_add(len) > self.max_queued_bytes {
            return Err((IpcError::NoSpace, msg));
        }
        self.queue.push_back(msg);
        self.queued_bytes = self.queued_bytes.saturating_add(len);
        Ok(())
    }

    pub(super) fn pop(&mut self) -> Result<Message, IpcError> {
        if !self.alive {
            return Err(IpcError::NoSuchEndpoint);
        }
        let msg = self.queue.pop_front().ok_or(IpcError::QueueEmpty)?;
        self.queued_bytes = self.queued_bytes.saturating_sub(msg.payload.len());
        Ok(msg)
    }

    pub(super) fn push_front(&mut self, msg: Message) -> Result<(), IpcError> {
        if !self.alive {
            return Err(IpcError::NoSuchEndpoint);
        }
        if self.queue.len() >= self.depth {
            return Err(IpcError::QueueFull);
        }
        let len = msg.payload.len();
        if self.queued_bytes.saturating_add(len) > self.max_queued_bytes {
            return Err(IpcError::NoSpace);
        }
        self.queue.push_front(msg);
        self.queued_bytes = self.queued_bytes.saturating_add(len);
        Ok(())
    }

    pub(super) fn register_recv_waiter(&mut self, pid: WaiterId) {
        if !self.alive {
            return;
        }
        if self.recv_waiters.iter().any(|p| *p == pid) {
            return;
        }
        self.recv_waiters.push_back(pid);
    }

    pub(super) fn register_send_waiter(&mut self, pid: WaiterId) {
        if !self.alive {
            return;
        }
        if self.send_waiters.iter().any(|p| *p == pid) {
            return;
        }
        self.send_waiters.push_back(pid);
    }

    pub(super) fn pop_recv_waiter(&mut self) -> Option<WaiterId> {
        self.recv_waiters.pop_front()
    }

    pub(super) fn pop_send_waiter(&mut self) -> Option<WaiterId> {
        self.send_waiters.pop_front()
    }

    pub(super) fn remove_recv_waiter(&mut self, pid: WaiterId) -> bool {
        let before = self.recv_waiters.len();
        self.recv_waiters.retain(|p| *p != pid);
        before != self.recv_waiters.len()
    }

    pub(super) fn remove_send_waiter(&mut self, pid: WaiterId) -> bool {
        let before = self.send_waiters.len();
        self.send_waiters.retain(|p| *p != pid);
        before != self.send_waiters.len()
    }

    pub(super) fn close_if_owned_by(
        &mut self,
        owner: WaiterId,
    ) -> Option<(Vec<WaiterId>, Vec<WaiterId>)> {
        if !self.alive || self.owner != Some(owner) {
            return None;
        }
        self.alive = false;
        self.queue.clear();
        self.queued_bytes = 0;
        let recv: Vec<WaiterId> = self.recv_waiters.drain(..).collect();
        let send: Vec<WaiterId> = self.send_waiters.drain(..).collect();
        Some((recv, send))
    }
}
