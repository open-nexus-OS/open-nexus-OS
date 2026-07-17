// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Bounded, recycling node arena for the interaction net. Ports are
//! packed `AtomicU64`s so workers share `&Arena` safely (an interaction only
//! writes ports whose previous peer it just consumed — disjoint by protocol;
//! atomics make that data-race-free by construction, no unsafe). The free
//! list recycles (bump-only would exhaust under net reduction, which frees
//! as much as it allocates) and sits behind a spin lock (TASK-0277).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: host tests in reduce.rs (alloc/free balance, exhaustion)

use alloc::vec::Vec;
use core::sync::atomic::{AtomicI64, AtomicU64, AtomicU8, Ordering};

use nexus_sync::SpinLock;

/// Packed port reference: `node_index * 8 + slot`. Slot 0 is the principal
/// port; slots 1..=2 are auxiliary.
pub type Port = u64;

/// "Not connected" sentinel.
pub const NIL: Port = u64::MAX;

#[inline]
#[must_use]
pub const fn port(node: u32, slot: u8) -> Port {
    (node as u64) * 8 + slot as u64
}

#[inline]
#[must_use]
pub const fn port_node(p: Port) -> u32 {
    (p / 8) as u32
}

#[inline]
#[must_use]
pub const fn port_slot(p: Port) -> u8 {
    (p % 8) as u8
}

/// Agent kinds. `Free` marks recycled slots; `Root` is the net's external
/// interface (never part of a redex — a redex needs BOTH kinds >= Era).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NodeKind {
    Free = 0,
    Root = 1,
    /// Eraser (arity 0).
    Era = 2,
    /// Constructor (arity 2). Same-kind pairs annihilate, CON-DUP commutes.
    Con = 3,
    /// Duplicator (arity 2).
    Dup = 4,
    /// Number leaf (arity 0, payload = value).
    Num = 5,
    /// Binary add awaiting its FIRST operand on the principal port
    /// (aux1 = second operand, aux2 = result).
    Add2 = 6,
    /// Add with the first operand captured in the payload (aux1 = result).
    Add1 = 7,
}

impl NodeKind {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Root,
            2 => Self::Era,
            3 => Self::Con,
            4 => Self::Dup,
            5 => Self::Num,
            6 => Self::Add2,
            7 => Self::Add1,
            _ => Self::Free,
        }
    }
}

/// An active pair: two nodes connected principal-to-principal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Redex(pub u32, pub u32);

/// Errors — every rejection is explicit and bounded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InetError {
    /// The bounded arena is out of nodes (reject, never OOM).
    ArenaExhausted,
    /// The net reached a principal pair with no interaction rule.
    Stuck,
    /// The round budget elapsed before the normal form was reached.
    Diverged,
}

struct Node {
    kind: AtomicU8,
    payload: AtomicI64,
    ports: [AtomicU64; 3],
}

impl Node {
    fn new_free() -> Self {
        Self {
            kind: AtomicU8::new(NodeKind::Free as u8),
            payload: AtomicI64::new(0),
            ports: [AtomicU64::new(NIL), AtomicU64::new(NIL), AtomicU64::new(NIL)],
        }
    }
}

/// The bounded net store. Shared by reference across workers: all node state
/// is atomic, the free list is spin-locked.
pub struct Arena {
    nodes: Vec<Node>,
    free: SpinLock<Vec<u32>>,
}

impl Arena {
    /// A fresh arena with `capacity` nodes, all free.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let mut nodes = Vec::with_capacity(capacity);
        let mut free = Vec::with_capacity(capacity);
        for i in 0..capacity {
            nodes.push(Node::new_free());
            // Pop order = ascending indices (deterministic single-thread builds).
            free.push((capacity - 1 - i) as u32);
        }
        Self { nodes, free: SpinLock::new(free) }
    }

    /// Allocate a node of `kind` (ports NIL). Bounded: `ArenaExhausted` when
    /// the free list is empty — the caller rejects the job, nothing panics.
    pub fn alloc(&self, kind: NodeKind, payload: i64) -> Result<u32, InetError> {
        let idx = self.free.lock().pop().ok_or(InetError::ArenaExhausted)?;
        let n = &self.nodes[idx as usize];
        n.kind.store(kind as u8, Ordering::Release);
        n.payload.store(payload, Ordering::Release);
        for p in &n.ports {
            p.store(NIL, Ordering::Release);
        }
        Ok(idx)
    }

    /// Recycle a node (kind → Free, back on the free list).
    pub fn free_node(&self, idx: u32) {
        let n = &self.nodes[idx as usize];
        n.kind.store(NodeKind::Free as u8, Ordering::Release);
        for p in &n.ports {
            p.store(NIL, Ordering::Release);
        }
        self.free.lock().push(idx);
    }

    #[must_use]
    pub fn kind(&self, idx: u32) -> NodeKind {
        NodeKind::from_u8(self.nodes[idx as usize].kind.load(Ordering::Acquire))
    }

    /// Repurpose a node in place (used by rules that transmute, e.g.
    /// Add2 → Add1 — cheaper than free + alloc and keeps rounds alloc-free
    /// for the tree workloads).
    pub fn set_kind_payload(&self, idx: u32, kind: NodeKind, payload: i64) {
        let n = &self.nodes[idx as usize];
        n.payload.store(payload, Ordering::Release);
        n.kind.store(kind as u8, Ordering::Release);
    }

    #[must_use]
    pub fn payload(&self, idx: u32) -> i64 {
        self.nodes[idx as usize].payload.load(Ordering::Acquire)
    }

    /// The current peer of `p` (NIL if unconnected).
    #[must_use]
    pub fn peer(&self, p: Port) -> Port {
        self.nodes[port_node(p) as usize].ports[port_slot(p) as usize].load(Ordering::Acquire)
    }

    /// Connect two ports bidirectionally. Returns `Some(Redex)` when the link
    /// creates an active pair (both principal ports of real agents) — the
    /// caller queues it (per-round out list).
    pub fn link(&self, a: Port, b: Port) -> Option<Redex> {
        self.nodes[port_node(a) as usize].ports[port_slot(a) as usize]
            .store(b, Ordering::Release);
        self.nodes[port_node(b) as usize].ports[port_slot(b) as usize]
            .store(a, Ordering::Release);
        if port_slot(a) == 0 && port_slot(b) == 0 {
            let (ka, kb) = (self.kind(port_node(a)), self.kind(port_node(b)));
            if ka as u8 >= NodeKind::Era as u8 && kb as u8 >= NodeKind::Era as u8 {
                return Some(Redex(port_node(a), port_node(b)));
            }
        }
        None
    }

    /// Free-list length (diagnostics + leak checks in tests).
    #[must_use]
    pub fn free_len(&self) -> usize {
        self.free.lock().len()
    }
}
