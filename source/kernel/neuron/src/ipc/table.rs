// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: where the router keeps its endpoints (TASK-0330 P2, ADR-0102): a block table of
//! slots that never move, each endpoint behind its own lock (class `ipc`), so two pairs of
//! services on two harts touch no common state but two counters. The queued-byte total is an
//! atomic that a send reserves from and gives back on a refusal; an owner's total is one
//! counter its endpoints share. Creation and reuse of a slot hold the registry lock; finding an
//! endpoint by its id takes none.
//! OWNERS: @kernel-ipc-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the router's host tests (`ipc/mod.rs`: accounting against a census after
//!   2000 random operations), `sync::block_table`, `sync::ranked`
//! INVARIANTS: a `Slot` is an endpoint under its lock; no slot's lock is held while another
//!   slot's is waited for; the counters never go below what the queues hold.
//! ADR: docs/adr/0102-a-thread-blocks-in-two-steps-and-a-waker-never-waits-ipc-leaves-the-kernel-lock.md

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicUsize, Ordering};

use super::endpoint::Endpoint;
use super::{EndpointId, Router, WaiterId};
use crate::cap::Held;
use crate::sync::block_table::BlockTable;
use crate::sync::lock_rank::Class;
use crate::sync::ranked::{RankedGuard, RankedLock};

/// Blocks the table may grow to: 65 536 endpoints.
const BLOCKS: usize = 1024;

/// One slot of the table: `None` until an endpoint was created in it.
pub(super) type Cell = RankedLock<Option<Endpoint>>;

/// The owners' queued-byte counters, by owner — the registry lock's content.
pub(super) type Owners = BTreeMap<WaiterId, Arc<AtomicUsize>>;

/// An endpoint under its lock.
pub(super) struct Slot<'a>(RankedGuard<'a, Option<Endpoint>>);

impl Deref for Slot<'_> {
    type Target = Endpoint;
    fn deref(&self) -> &Endpoint {
        match self.0.as_ref() {
            Some(endpoint) => endpoint,
            // `Router::ep` hands out a slot only where an endpoint is.
            None => unreachable!("an empty slot was handed out"),
        }
    }
}

impl DerefMut for Slot<'_> {
    fn deref_mut(&mut self) -> &mut Endpoint {
        match self.0.as_mut() {
            Some(endpoint) => endpoint,
            None => unreachable!("an empty slot was handed out"),
        }
    }
}

/// An empty table.
pub(super) fn new_slots() -> BlockTable<Cell> {
    BlockTable::with_blocks(BLOCKS)
}

impl Router {
    /// Endpoint `id` under its lock; `None` where none was ever created.
    pub(super) fn ep(&self, id: EndpointId) -> Option<Slot<'_>> {
        let guard = self.slots.get(id as usize)?.lock();
        guard.is_some().then_some(Slot(guard))
    }

    /// The ids ever created (dead ones included).
    pub(super) fn slot_count(&self) -> usize {
        self.slots.len()
    }

    /// Runs `f` on every endpoint ever created, one at a time under its lock.
    pub(super) fn each(&self, mut f: impl FnMut(EndpointId, &mut Endpoint)) {
        for id in 0..self.slots.len() as EndpointId {
            if let Some(mut ep) = self.ep(id) {
                f(id, &mut ep);
            }
        }
    }

    /// Puts `endpoint` into slot `id` — a new slot, or a dead one being reused. The caller holds
    /// the registry lock.
    pub(super) fn install(&self, id: EndpointId, endpoint: Endpoint) {
        if let Some(cell) = self.slots.get(id as usize) {
            *cell.lock() = Some(endpoint);
        }
    }

    /// Adds an empty slot. The caller holds the registry lock.
    pub(super) fn grow(&self) -> Option<EndpointId> {
        self.slots
            .grow(crate::kheap::admits, || RankedLock::new(Class::Ipc, None))
            .ok()
            .map(|index| index as EndpointId)
    }

    /// `owner`'s byte counter, made at its first endpoint. `Err`: the heap refuses one.
    pub(super) fn owner_counter(
        owners: &mut Owners,
        owner: Option<WaiterId>,
    ) -> Result<Option<Arc<AtomicUsize>>, ()> {
        let Some(owner) = owner else { return Ok(None) };
        if let Some(counter) = owners.get(&owner) {
            return Ok(Some(counter.clone()));
        }
        // ADR-0091: above the heap's reserve, or the endpoint is not made.
        if !crate::kheap::admits(4 * core::mem::size_of::<usize>()) {
            return Err(());
        }
        let counter = Arc::new(AtomicUsize::new(0));
        owners.insert(owner, counter.clone());
        Ok(Some(counter))
    }

    /// Reserves `len` queued bytes for a message into `ep` from the global and the owner's
    /// budget; false (and nothing taken) where either is spent. The caller holds `ep`'s lock.
    pub(super) fn reserve(&self, ep: &Endpoint, len: usize) -> bool {
        let total = self.queued_bytes_total.fetch_add(len, Ordering::AcqRel);
        if total.saturating_add(len) > self.max_queued_bytes_total {
            self.queued_bytes_total.fetch_sub(len, Ordering::AcqRel);
            return false;
        }
        if let Some(owner) = &ep.owner_bytes {
            let owned = owner.fetch_add(len, Ordering::AcqRel);
            if owned.saturating_add(len) > self.max_queued_bytes_per_owner {
                owner.fetch_sub(len, Ordering::AcqRel);
                self.queued_bytes_total.fetch_sub(len, Ordering::AcqRel);
                return false;
            }
        }
        true
    }

    /// Gives `len` queued bytes of `ep` back to both budgets. The caller holds `ep`'s lock.
    pub(super) fn release(&self, ep: &Endpoint, len: usize) {
        self.queued_bytes_total.fetch_sub(len, Ordering::AcqRel);
        if let Some(owner) = &ep.owner_bytes {
            owner.fetch_sub(len, Ordering::AcqRel);
        }
    }

    /// Capabilities a close took out of dead queues: kept for the caller's release loop
    /// (`take_in_flight_dropped`). Called with no endpoint held.
    pub(super) fn retire(&self, mut dropped: Vec<Held>) {
        if !dropped.is_empty() {
            self.in_flight_dropped.lock().append(&mut dropped);
        }
    }
}
