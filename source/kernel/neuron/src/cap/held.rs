// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: A held capability is counted at its object (RFC-0102, TASK-0330 P5). An endpoint
//! and a memory object each own a header (`HeldCounts`): who the object is and how many
//! capabilities to it are held — all, those with the send right, those with the receive right,
//! and the send ones that sit in the owner's own table. A `Held` is one held capability: its
//! descriptor and a reference to the header; it is never copied, its drop counts it off, and a
//! move changes no count. So no path asks every table who holds a capability to an object —
//! the walk a blocking receive and a capability close ran under the kernel lock, the largest
//! holders of TASK-0330 P0's measurement — and no observer sees a count below the truth while
//! a capability travels.
//! OWNERS: @kernel-cap-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: host — `cap/tests_prop.rs` (the counts against a census over 10 000 random
//!   operations, the `test_reject_*` of the doors)
//! INVARIANTS:
//!   - The kind and the rights of a `Held` are fixed when it comes into being: the fields are
//!     private to this file and nothing here writes them. A second one (`dup`) has the same
//!     kind and a subset of the rights; a read-only alias never becomes a writable one.
//!   - A counted kind (endpoint, memory object) comes into being only at its header (`mint`)
//!     or from a `Held` of the same object; `plain` refuses it — never from a number.
//!   - A header's counts equal the `Held` that exist.
//!
//! RFC: docs/rfcs/RFC-0102-smp-lock-decomposition-lock-classes-ranks-and-budgets.md

use alloc::sync::Arc;
use core::fmt;
use core::sync::atomic::{AtomicU32, Ordering};

use super::{CapError, Capability, CapabilityKind, EndpointId, Rights};

/// The object a header counts the held capabilities of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Counted {
    /// A kernel endpoint.
    Endpoint(EndpointId),
    /// A memory object of `len` bytes.
    Vmo { id: u32, len: usize },
}

/// The header an endpoint or a memory object owns: its identity and the capabilities held to
/// it.
pub struct HeldCounts {
    object: Counted,
    /// The endpoint's owner (its creator's pid): never its own peer. `None` for a memory
    /// object and for an endpoint the kernel made for nobody.
    owner: Option<u32>,
    any: AtomicU32,
    send: AtomicU32,
    recv: AtomicU32,
    /// Send capabilities sitting in the owner's own table — the base it clones a reply
    /// capability from.
    owner_send: AtomicU32,
}

impl HeldCounts {
    /// The bytes one header takes from the kernel heap (the header and its reference counts).
    const HEAP_BYTES: usize = core::mem::size_of::<Self>() + 2 * core::mem::size_of::<usize>();

    fn new(object: Counted, owner: Option<u32>) -> Option<Arc<Self>> {
        // ADR-0091: above the heap's reserve, or the object is not made.
        if !crate::kheap::admits(Self::HEAP_BYTES) {
            return None;
        }
        Some(Arc::new(Self {
            object,
            owner,
            any: AtomicU32::new(0),
            send: AtomicU32::new(0),
            recv: AtomicU32::new(0),
            owner_send: AtomicU32::new(0),
        }))
    }

    /// The header of endpoint `id`, owned by `owner`. `None`: the kernel heap refuses it.
    pub fn endpoint(id: EndpointId, owner: Option<u32>) -> Option<Arc<Self>> {
        Self::new(Counted::Endpoint(id), owner)
    }

    /// The header of the memory object `id` of `len` bytes. `None`: the heap refuses it.
    pub fn vmo(id: u32, len: usize) -> Option<Arc<Self>> {
        Self::new(Counted::Vmo { id, len }, None)
    }

    /// A new capability to the object with `rights` — the object's own door: only the
    /// registry that holds the header (the router, the memory-object table) mints from it.
    pub fn mint(self: &Arc<Self>, rights: Rights) -> Held {
        let kind = match self.object {
            Counted::Endpoint(id) => CapabilityKind::Endpoint(id),
            Counted::Vmo { id, len } => CapabilityKind::Vmo { id, len },
        };
        Held::counted(Capability { kind, rights }, self.clone())
    }

    /// A new READ-ONLY capability to the memory object with `rights` (RFC-0080) — the door of
    /// an object nobody ever holds writably: the kernel's own (the device tree, the console
    /// ring, the boot image's files). `None` for an endpoint: it has no read-only kind.
    pub fn mint_read_only(self: &Arc<Self>, rights: Rights) -> Option<Held> {
        match self.object {
            Counted::Vmo { id, len } => Some(Held::counted(
                Capability { kind: CapabilityKind::VmoRo { id, len }, rights },
                self.clone(),
            )),
            Counted::Endpoint(_) => None,
        }
    }

    /// Capabilities held to the object, in a table or in a message.
    pub fn any(&self) -> usize {
        self.any.load(Ordering::Acquire) as usize
    }

    /// Those with the receive right.
    pub fn receivers(&self) -> usize {
        self.recv.load(Ordering::Acquire) as usize
    }

    /// Those with the send right that are not the owner's own: in another table, or in a
    /// message (whoever dequeues it will hold it — never the owner's base).
    pub fn foreign_senders(&self) -> usize {
        let send = self.send.load(Ordering::Acquire);
        send.saturating_sub(self.owner_send.load(Ordering::Acquire)) as usize
    }

    fn count(&self, rights: Rights, up: bool) {
        let step = |counter: &AtomicU32| {
            if up {
                counter.fetch_add(1, Ordering::AcqRel);
            } else {
                counter.fetch_sub(1, Ordering::AcqRel);
            }
        };
        step(&self.any);
        if rights.contains(Rights::SEND) {
            step(&self.send);
        }
        if rights.contains(Rights::RECV) {
            step(&self.recv);
        }
    }
}

impl fmt::Debug for HeldCounts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HeldCounts")
            .field("object", &self.object)
            .field("any", &self.any())
            .field("foreign_senders", &self.foreign_senders())
            .field("receivers", &self.receivers())
            .finish()
    }
}

/// One held capability. Neither `Copy` nor `Clone`: a second one is a `dup`, and its drop is
/// the drop of a capability. Its kind and rights never change.
pub struct Held {
    cap: Capability,
    counts: Option<Arc<HeldCounts>>,
    /// This send capability sits in its endpoint's owner's table (the header's fourth count).
    in_owner_table: bool,
}

impl Held {
    fn counted(cap: Capability, counts: Arc<HeldCounts>) -> Self {
        counts.count(cap.rights, true);
        Self { cap, counts: Some(counts), in_owner_table: false }
    }

    /// A capability of a kind that carries no counts (a timer, a waitset, a fence, a device
    /// window, an interrupt, a factory, a scheduling right). An endpoint or a memory object
    /// is refused: a counted kind is never made from a number.
    pub fn plain(cap: Capability) -> Result<Self, CapError> {
        match cap.kind {
            CapabilityKind::Endpoint(_)
            | CapabilityKind::Vmo { .. }
            | CapabilityKind::VmoRo { .. } => Err(CapError::PermissionDenied),
            _ => Ok(Self { cap, counts: None, in_owner_table: false }),
        }
    }

    /// The descriptor: kind and rights, a plain value that holds nothing.
    pub fn cap(&self) -> Capability {
        self.cap
    }

    /// The header of the object this capability is counted at; `None` for a plain kind.
    pub fn counts(&self) -> Option<&Arc<HeldCounts>> {
        self.counts.as_ref()
    }

    /// A second capability to the same object, of the same kind, with `rights` — a subset of
    /// this one's.
    pub fn dup(&self, rights: Rights) -> Result<Self, CapError> {
        if !self.cap.rights.contains(rights) {
            return Err(CapError::PermissionDenied);
        }
        let cap = Capability { kind: self.cap.kind, rights };
        Ok(match &self.counts {
            Some(counts) => Self::counted(cap, counts.clone()),
            None => Self { cap, counts: None, in_owner_table: false },
        })
    }

    /// A read-only alias of the memory object this capability holds (RFC-0080), with `rights`
    /// — a subset of this one's. Only a writable capability to a memory object has one.
    pub fn read_only_alias(&self, rights: Rights) -> Result<Self, CapError> {
        if !self.cap.rights.contains(rights) {
            return Err(CapError::PermissionDenied);
        }
        match (self.cap.kind, &self.counts) {
            (CapabilityKind::Vmo { id, len }, Some(counts)) => Ok(Self::counted(
                Capability { kind: CapabilityKind::VmoRo { id, len }, rights },
                counts.clone(),
            )),
            _ => Err(CapError::PermissionDenied),
        }
    }

    /// The capability enters the table of `holder`: a send capability in its endpoint's
    /// owner's table is the owner's own, not a peer's.
    pub(super) fn enter_table(&mut self, holder: Option<u32>) {
        let Some(counts) = &self.counts else { return };
        if !self.in_owner_table
            && holder.is_some()
            && counts.owner == holder
            && matches!(counts.object, Counted::Endpoint(_))
            && self.cap.rights.contains(Rights::SEND)
        {
            counts.owner_send.fetch_add(1, Ordering::AcqRel);
            self.in_owner_table = true;
        }
    }

    /// The capability leaves its table (to travel, or to be dropped).
    pub(super) fn leave_table(&mut self) {
        if self.in_owner_table {
            if let Some(counts) = &self.counts {
                counts.owner_send.fetch_sub(1, Ordering::AcqRel);
            }
            self.in_owner_table = false;
        }
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        self.leave_table();
        if let Some(counts) = &self.counts {
            counts.count(self.cap.rights, false);
        }
    }
}

impl fmt::Debug for Held {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.cap.fmt(f)
    }
}
