// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the kill order's bytes — the lifecycle authority's `OP_MEM_KILL` (abilitymgr
//! `protocol.rs`, RFC-0100 §3 / RFC-0111 §5): `[A,M,1,OP_MEM_KILL, pid:u32le]`, answered
//! `[A,M,1,OP_MEM_KILL|0x80, status, pid:u32le]`. abilitymgr takes the order from THIS
//! service's kernel sender id alone, refuses a foreground instance and orders the parent
//! (execd `OP_KILL`). The carrier is a trait so the host tests drive the same policy with a
//! recording fake and the OS loop with the declared route.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Internal (string-identical to abilitymgr's `protocol.rs`)
//! TEST_COVERAGE: tests/contract.rs (golden bytes), abilitymgr's `mem_kill` tests (the decode)

/// abilitymgr's envelope.
pub const ABILITYMGR_MAGIC: [u8; 2] = [b'A', b'M'];
/// abilitymgr's wire version.
pub const ABILITYMGR_VERSION: u8 = 1;
/// abilitymgr's `OP_MEM_KILL`.
pub const OP_MEM_KILL: u8 = 7;
/// The reply's op (the request's with the response flag).
pub const OP_MEM_KILL_RESPONSE: u8 = OP_MEM_KILL | 0x80;
/// abilitymgr's `STATUS_OK`.
pub const STATUS_OK: u8 = 0;
/// The order frame's length.
pub const ORDER_LEN: usize = 8;
/// The reply frame's length.
pub const REPLY_LEN: usize = 9;

/// `[A,M,1,OP_MEM_KILL, pid:u32le]`.
#[must_use]
pub fn encode_mem_kill(pid: u32) -> [u8; ORDER_LEN] {
    let p = pid.to_le_bytes();
    [
        ABILITYMGR_MAGIC[0],
        ABILITYMGR_MAGIC[1],
        ABILITYMGR_VERSION,
        OP_MEM_KILL,
        p[0],
        p[1],
        p[2],
        p[3],
    ]
}

/// `true` when `frame` is the authority's reply saying the kill of `pid` was ordered.
#[must_use]
pub fn mem_kill_carried_out(frame: &[u8], pid: u32) -> bool {
    frame.len() == REPLY_LEN
        && frame[..2] == ABILITYMGR_MAGIC
        && frame[2] == ABILITYMGR_VERSION
        && frame[3] == OP_MEM_KILL_RESPONSE
        && frame[4] == STATUS_OK
        && frame[5..9] == pid.to_le_bytes()
}

/// Who carries a kill order to the lifecycle authority: the declared route on the OS, a
/// recording fake in the tests. `true` when the authority reports the kill ordered.
pub trait KillOrder {
    /// Orders the kill of `pid`.
    fn kill(&mut self, pid: u32) -> bool;
}

/// A carrier that records every order and answers a fixed verdict (tests, the host build).
#[derive(Debug, Default)]
pub struct Recording {
    /// The pids ordered killed, in order.
    pub ordered: [u32; 8],
    /// How many were ordered (the first `n` of `ordered` are valid).
    pub n: usize,
    /// The verdict every order gets.
    pub verdict: bool,
}

impl Recording {
    /// A carrier that reports every order carried out.
    #[must_use]
    pub const fn accepting() -> Self {
        Self { ordered: [0; 8], n: 0, verdict: true }
    }

    /// A carrier that reports every order refused.
    #[must_use]
    pub const fn refusing() -> Self {
        Self { ordered: [0; 8], n: 0, verdict: false }
    }

    /// The orders recorded so far.
    #[must_use]
    pub fn orders(&self) -> &[u32] {
        &self.ordered[..self.n.min(self.ordered.len())]
    }
}

impl KillOrder for Recording {
    fn kill(&mut self, pid: u32) -> bool {
        if self.n < self.ordered.len() {
            self.ordered[self.n] = pid;
        }
        self.n += 1;
        self.verdict
    }
}
