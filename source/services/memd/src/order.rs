// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the orders' bytes — the lifecycle authority's `OP_MEM_KILL` and `OP_MEM_SQUEEZE`
//! (abilitymgr `protocol.rs`, RFC-0100 §3/§5, RFC-0111 §5, ADR-0086/0087). The kill:
//! `[A,M,1,OP_MEM_KILL, pid:u32le]`, answered `[A,M,1,OP_MEM_KILL|0x80, status, pid:u32le]`;
//! abilitymgr refuses a foreground instance and orders the parent (execd `OP_KILL`). The squeeze
//! (6f-1): `[A,M,1,OP_MEM_SQUEEZE, pid:u32le]`, answered with the parent's totals
//! `[A,M,1,OP_MEM_SQUEEZE|0x80, status, pid, pages, zero, kept, taken, stored: u32le]`;
//! abilitymgr refuses an instance that is not suspended and orders the parent (execd
//! `OP_SQUEEZE`). Both are taken from THIS service's kernel sender id alone. The carrier is a
//! trait so the host tests drive the same policy with a recording fake and the OS loop with the
//! declared route.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Internal (string-identical to abilitymgr's `protocol.rs` / `mem_squeeze.rs`)
//! TEST_COVERAGE: tests/contract.rs (golden bytes), abilitymgr's `mem_kill` / `mem_squeeze`
//!   tests (the decode)

/// abilitymgr's envelope.
pub const ABILITYMGR_MAGIC: [u8; 2] = [b'A', b'M'];
/// abilitymgr's wire version.
pub const ABILITYMGR_VERSION: u8 = 1;
/// abilitymgr's `OP_MEM_KILL`.
pub const OP_MEM_KILL: u8 = 7;
/// The reply's op (the request's with the response flag).
pub const OP_MEM_KILL_RESPONSE: u8 = OP_MEM_KILL | 0x80;
/// abilitymgr's `OP_MEM_SQUEEZE` (TASK-0333 6f-1, ADR-0087).
pub const OP_MEM_SQUEEZE: u8 = 8;
/// The squeeze reply's op.
pub const OP_MEM_SQUEEZE_RESPONSE: u8 = OP_MEM_SQUEEZE | 0x80;
/// abilitymgr's `STATUS_OK`.
pub const STATUS_OK: u8 = 0;
/// The order frame's length (both orders).
pub const ORDER_LEN: usize = 8;
/// The kill reply's length.
pub const REPLY_LEN: usize = 9;
/// The squeeze reply's length.
pub const SQUEEZE_REPLY_LEN: usize = 9 + 5 * 4;

fn order(op: u8, pid: u32) -> [u8; ORDER_LEN] {
    let p = pid.to_le_bytes();
    [ABILITYMGR_MAGIC[0], ABILITYMGR_MAGIC[1], ABILITYMGR_VERSION, op, p[0], p[1], p[2], p[3]]
}

/// `[A,M,1,OP_MEM_KILL, pid:u32le]`.
#[must_use]
pub fn encode_mem_kill(pid: u32) -> [u8; ORDER_LEN] {
    order(OP_MEM_KILL, pid)
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

/// What a squeeze took, as the parent counted it (the batches of `task_squeeze`, added up).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Squeezed {
    /// Pages recorded compressed.
    pub pages: u32,
    /// Pages of zeroes dropped.
    pub zero: u32,
    /// Committed pages kept as they are.
    pub kept: u32,
    /// Frames the stores took.
    pub taken: u32,
    /// Bytes of the records written.
    pub stored: u32,
}

impl Squeezed {
    /// Frames given back net: every recorded and every dropped page freed one, the stores took
    /// `taken`.
    #[must_use]
    pub fn net_frames(&self) -> i64 {
        i64::from(self.pages) + i64::from(self.zero) - i64::from(self.taken)
    }

    /// The records' bytes against the pages they replace, in percent (0 when nothing was
    /// recorded).
    #[must_use]
    pub fn ratio_percent(&self) -> u64 {
        match u64::from(self.pages) * 4096 {
            0 => 0,
            whole => u64::from(self.stored) * 100 / whole,
        }
    }
}

/// `[A,M,1,OP_MEM_SQUEEZE, pid:u32le]`.
#[must_use]
pub fn encode_mem_squeeze(pid: u32) -> [u8; ORDER_LEN] {
    order(OP_MEM_SQUEEZE, pid)
}

/// The parent's totals when `frame` is the authority's reply saying the squeeze of `pid` was
/// carried out; `None` for a refusal, another pid's answer or anything else.
#[must_use]
pub fn mem_squeeze_reply(frame: &[u8], pid: u32) -> Option<Squeezed> {
    if frame.len() != SQUEEZE_REPLY_LEN
        || frame[..2] != ABILITYMGR_MAGIC
        || frame[2] != ABILITYMGR_VERSION
        || frame[3] != OP_MEM_SQUEEZE_RESPONSE
        || frame[4] != STATUS_OK
    {
        return None;
    }
    let word = |i: usize| {
        u32::from_le_bytes([frame[5 + i * 4], frame[6 + i * 4], frame[7 + i * 4], frame[8 + i * 4]])
    };
    (word(0) == pid).then(|| Squeezed {
        pages: word(1),
        zero: word(2),
        kept: word(3),
        taken: word(4),
        stored: word(5),
    })
}

/// Who carries the authority's orders to the lifecycle authority: the declared route on the OS,
/// a recording fake in the tests.
pub trait Orders {
    /// Orders the kill of `pid`; `true` when the authority reports the kill ordered.
    fn kill(&mut self, pid: u32) -> bool;
    /// Orders the squeeze of `pid` (ADR-0087); the parent's totals, `None` when refused.
    fn squeeze(&mut self, pid: u32) -> Option<Squeezed>;
}

/// A carrier that records every order and answers a fixed verdict (tests, the host build).
#[derive(Debug, Default)]
pub struct Recording {
    /// The pids ordered killed, in order.
    pub ordered: [u32; 8],
    /// How many were ordered (the first `n` of `ordered` are valid).
    pub n: usize,
    /// The pids ordered squeezed, in order.
    pub squeezes: [u32; 8],
    /// How many were ordered squeezed.
    pub ns: usize,
    /// The verdict every kill order gets.
    pub verdict: bool,
    /// The answer every squeeze order gets.
    pub squeezed: Option<Squeezed>,
}

impl Recording {
    /// A carrier that reports every order carried out (a squeeze: 100 pages into 10 frames).
    #[must_use]
    pub const fn accepting() -> Self {
        Self {
            ordered: [0; 8],
            n: 0,
            squeezes: [0; 8],
            ns: 0,
            verdict: true,
            squeezed: Some(Squeezed { pages: 100, zero: 20, kept: 5, taken: 10, stored: 40_000 }),
        }
    }

    /// A carrier that reports every order refused.
    #[must_use]
    pub const fn refusing() -> Self {
        Self { ordered: [0; 8], n: 0, squeezes: [0; 8], ns: 0, verdict: false, squeezed: None }
    }

    /// The kill orders recorded so far.
    #[must_use]
    pub fn orders(&self) -> &[u32] {
        &self.ordered[..self.n.min(self.ordered.len())]
    }

    /// The squeeze orders recorded so far.
    #[must_use]
    pub fn squeeze_orders(&self) -> &[u32] {
        &self.squeezes[..self.ns.min(self.squeezes.len())]
    }
}

impl Orders for Recording {
    fn kill(&mut self, pid: u32) -> bool {
        if self.n < self.ordered.len() {
            self.ordered[self.n] = pid;
        }
        self.n += 1;
        self.verdict
    }

    fn squeeze(&mut self, pid: u32) -> Option<Squeezed> {
        if self.ns < self.squeezes.len() {
            self.squeezes[self.ns] = pid;
        }
        self.ns += 1;
        self.squeezed
    }
}
