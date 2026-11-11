// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(test)]
//! CONTEXT: Host tests of the capability table and of the counts a held capability keeps at
//! its object (RFC-0102, TASK-0330 P5)
//! OWNERS: @kernel-cap-team
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 15 tests
//! NOTE: Tests only; no kernel logic. Deterministic: the random walk runs from a fixed seed.
//!
//! TEST_SCOPE:
//!   - Slot ops: set/get, rights derivation (subset only), the low and the high end, growth
//!   - The counts of an endpoint and of a memory object against a census of every table and
//!     every message, after each of 10 000 random operations
//!   - The doors a counted capability comes into being by, and the ones that refuse
//!
//! TEST_SCENARIOS:
//!   - set_and_get_roundtrip / derive_keeps_the_kind_and_masks_the_rights
//!   - test_reject_derive_beyond_the_rights_held
//!   - allocate_high_never_collides_with_low
//!   - a_full_table_grows_and_the_slots_keep_their_ends
//!   - test_reject_a_growth_while_the_pool_refuses
//!   - the_counts_equal_a_census_after_every_one_of_10_000_random_operations
//!   - the_owner_of_an_endpoint_is_never_its_own_peer
//!   - test_reject_unowned_endpoint_counts_every_sender
//!   - a_move_changes_no_count / a_refused_install_counts_the_capability_off
//!   - test_reject_a_counted_kind_made_from_a_number
//!   - test_reject_dup_beyond_the_rights_held
//!   - test_reject_a_writable_capability_from_a_read_only_alias
//!   - test_reject_a_header_while_the_pool_refuses

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;

use super::{CapError, CapTable, Capability, CapabilityKind, Held, HeldCounts, Rights};

/// xorshift64*: the walk's only source of choice, from a fixed seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn rights(&mut self) -> Rights {
        Rights::from_bits_truncate(self.next() as u32)
    }
}

fn plain(kind: CapabilityKind, rights: Rights) -> Held {
    Held::plain(Capability { kind, rights }).expect("a plain kind")
}

fn factory() -> Held {
    plain(CapabilityKind::EndpointFactory, Rights::MANAGE)
}

fn plain_kinds() -> [CapabilityKind; 7] {
    [
        CapabilityKind::EndpointFactory,
        CapabilityKind::DeviceMmio {
            base: 0x7000_0000,
            len: 0x1000,
            irq: 7,
            dma_noncoherent: false,
            dev: 3,
        },
        CapabilityKind::Irq(9),
        CapabilityKind::Timer(4),
        CapabilityKind::Waitset(5),
        CapabilityKind::Fence(6),
        CapabilityKind::SchedSetAffinity,
    ]
}

#[test]
fn set_and_get_roundtrip() {
    let mut rng = Rng(0x5eed_0001);
    for kind in plain_kinds() {
        for _ in 0..32 {
            let (slot, rights) = (rng.below(32), rng.rights());
            let mut table = CapTable::new();
            table.set(slot, plain(kind, rights)).unwrap();
            assert_eq!(table.get(slot), Ok(Capability { kind, rights }));
            assert_eq!(table.peek(slot), Ok(Some(Capability { kind, rights })));
        }
    }
    let mut table = CapTable::with_capacity(4);
    assert_eq!(table.set(4, factory()), Err(CapError::InvalidSlot));
    assert_eq!(table.set_if_empty(1, factory()), Ok(()));
    assert_eq!(table.set_if_empty(1, factory()), Err(CapError::PermissionDenied));
    assert_eq!(table.peek(2), Ok(None));
    assert_eq!(table.peek(4), Err(CapError::InvalidSlot));
}

#[test]
fn derive_keeps_the_kind_and_masks_the_rights() {
    let mut rng = Rng(0x5eed_0002);
    for kind in plain_kinds() {
        for _ in 0..64 {
            let (base, mask) = (rng.rights(), rng.rights());
            let mut table = CapTable::new();
            table.set(0, plain(kind, base)).unwrap();
            assert_eq!(table.derive(0, base & mask), Ok(Capability { kind, rights: base & mask }));
        }
    }
}

#[test]
fn test_reject_derive_beyond_the_rights_held() {
    let mut rng = Rng(0x5eed_0003);
    for _ in 0..256 {
        let (base, extra) = (rng.rights(), rng.rights());
        if base.contains(extra) {
            continue;
        }
        let mut table = CapTable::new();
        table.set(1, plain(CapabilityKind::Timer(1), base)).unwrap();
        assert_eq!(table.derive(1, base | extra), Err(CapError::PermissionDenied));
        assert!(matches!(table.dup(1, base | extra), Err(CapError::PermissionDenied)));
    }
}

/// RFC-0069 slot discipline: allocate packs low-first, allocate_high packs high-first, and the
/// two never hand out the same slot while free slots remain on both ends — the invariant that
/// keeps init's deterministic child wiring and a service's own early object creation apart.
#[test]
fn allocate_high_never_collides_with_low() {
    for low in 1usize..8 {
        for high in 1usize..8 {
            let mut table = CapTable::new();
            let low_slots: Vec<usize> =
                (0..low).map(|_| table.allocate(factory()).unwrap()).collect();
            let high_slots: Vec<usize> =
                (0..high).map(|_| table.allocate_high(factory()).unwrap()).collect();
            assert!(low_slots.iter().enumerate().all(|(i, slot)| *slot == i), "lows pack from 0");
            assert!(high_slots.windows(2).all(|pair| pair[1] == pair[0] - 1), "highs descend");
            assert!(high_slots.iter().all(|slot| *slot > low_slots[low - 1]));
        }
    }
}

/// TASK-0352 P7 (ADR-0093): the 257th capability lands in slot 256 — the table doubled — and a
/// task's own object after that lands at the new top.
#[test]
fn a_full_table_grows_and_the_slots_keep_their_ends() {
    let mut table = CapTable::new();
    for expected in 0..256 {
        assert_eq!(table.allocate(factory()), Ok(expected));
    }
    assert_eq!(table.capacity(), 256);
    assert_eq!(table.allocate(factory()), Ok(256), "the 257th grows the table");
    assert_eq!(table.capacity(), 512);
    assert_eq!(table.allocate_high(factory()), Ok(511), "the task's own object: the new top");
    assert_eq!(table.get(255).map(|c| c.rights), Ok(Rights::MANAGE), "the old slots are kept");
    let mut small = CapTable::with_capacity(2);
    assert_eq!((small.allocate_high(factory()), small.allocate_high(factory())), (Ok(1), Ok(0)));
    assert_eq!(small.allocate_high(factory()), Ok(9), "2 + at least 8: the new top");
}

/// While the pool refuses, a full table stays full: `NoSpace`, and nothing is half grown.
#[test]
fn test_reject_a_growth_while_the_pool_refuses() {
    let mut table = CapTable::with_capacity(4);
    for _ in 0..4 {
        table.allocate(factory()).unwrap();
    }
    let refuses = crate::kheap::refusal::PoolRefuses::new();
    assert_eq!(table.allocate(factory()), Err(CapError::NoSpace));
    assert_eq!(table.allocate_high(factory()), Err(CapError::NoSpace));
    assert_eq!(table.capacity(), 4, "no half growth");
    drop(refuses);
    assert_eq!(table.allocate(factory()), Ok(4));
}

// ---- the counts (RFC-0102 "a held capability is counted at its object") ----

const TABLES: usize = 6;
const ENDPOINTS: usize = 5;
const VMOS: usize = 4;

/// The world the walk runs in: the tables of six tasks, five endpoints (four owned by a task,
/// one by nobody), four memory objects, and the messages in flight.
struct World {
    tables: Vec<CapTable>,
    endpoints: Vec<Arc<HeldCounts>>,
    vmos: Vec<Arc<HeldCounts>>,
    flight: Vec<Held>,
}

fn pid_of(table: usize) -> u32 {
    table as u32 + 1
}

fn owner_of(endpoint: usize) -> Option<u32> {
    (endpoint < ENDPOINTS - 1).then(|| pid_of(endpoint % TABLES))
}

impl World {
    fn new() -> Self {
        Self {
            tables: (0..TABLES).map(|t| CapTable::new_for(pid_of(t))).collect(),
            endpoints: (0..ENDPOINTS)
                .map(|e| HeldCounts::endpoint(e as u32, owner_of(e)).expect("a header"))
                .collect(),
            vmos: (0..VMOS)
                .map(|v| HeldCounts::vmo(v as u32 + 1, 0x1000 * (v + 1)).expect("a header"))
                .collect(),
            flight: Vec::new(),
        }
    }

    fn occupied(&self, table: usize) -> Vec<usize> {
        let caps = &self.tables[table];
        (0..caps.capacity()).filter(|&slot| matches!(caps.peek(slot), Ok(Some(_)))).collect()
    }

    /// A slot of `table` that holds something, chosen by the walk.
    fn pick(&self, rng: &mut Rng, table: usize) -> Option<usize> {
        let occupied = self.occupied(table);
        (!occupied.is_empty()).then(|| occupied[rng.below(occupied.len())])
    }

    /// One random operation. Every way a capability comes into being, moves and goes.
    fn step(&mut self, rng: &mut Rng) {
        let (from, to) = (rng.below(TABLES), rng.below(TABLES));
        match rng.below(16) {
            // An object's registry mints a capability into a table.
            0 | 1 => {
                let held = self.endpoints[rng.below(ENDPOINTS)].mint(rng.rights());
                let _ = self.tables[to].allocate(held);
            }
            2 => {
                let held = self.vmos[rng.below(VMOS)].mint(rng.rights());
                let _ = self.tables[to].allocate_high(held);
            }
            // A clone or a transfer: a second capability with a subset of the rights.
            3 | 4 => {
                if let Some(slot) = self.pick(rng, from) {
                    let rights = self.tables[from].get(slot).unwrap().rights & rng.rights();
                    let held = self.tables[from].dup(slot, rights).unwrap();
                    let _ = self.tables[to].allocate(held);
                }
            }
            // A read-only alias of a memory object (refused for anything else).
            5 => {
                if let Some(slot) = self.pick(rng, from) {
                    let rights = self.tables[from].get(slot).unwrap().rights & rng.rights();
                    if let Ok(alias) = self.tables[from].held(slot).unwrap().read_only_alias(rights)
                    {
                        let _ = self.tables[to].allocate(alias);
                    }
                }
            }
            // A close.
            6..=9 => {
                if let Some(slot) = self.pick(rng, from) {
                    self.tables[from].take(slot).unwrap();
                }
            }
            // A capability moves into a message.
            10 | 11 => {
                if let Some(slot) = self.pick(rng, from) {
                    self.flight.push(self.tables[from].take_held(slot).unwrap());
                }
            }
            // A message is received: its capability lands in the receiver's table.
            12 | 13 => {
                if !self.flight.is_empty() {
                    let held = self.flight.swap_remove(rng.below(self.flight.len()));
                    if let Err((_, back)) = self.tables[to].try_allocate(held) {
                        self.flight.push(back);
                    }
                }
            }
            // A queue is closed with a message in it; or a slot is overwritten.
            14 => {
                if !self.flight.is_empty() && rng.below(2) == 0 {
                    drop(self.flight.swap_remove(rng.below(self.flight.len())));
                } else {
                    let held = self.endpoints[rng.below(ENDPOINTS)].mint(rng.rights());
                    let _ = self.tables[to].set(rng.below(16), held);
                }
            }
            // A plain capability; rarely, a task exits and its table goes at once.
            _ => {
                if rng.below(40) == 0 {
                    self.tables[from] = CapTable::new_for(pid_of(from));
                } else {
                    let _ = self.tables[to].allocate(plain(CapabilityKind::Timer(1), rng.rights()));
                }
            }
        }
    }

    /// Who holds what, counted the way the kernel did before the counts: every slot of every
    /// table and every message. Per endpoint `(foreign senders, receivers, all)`, per memory
    /// object all.
    fn census(&self) -> (BTreeMap<u32, (usize, usize, usize)>, BTreeMap<u32, usize>) {
        let mut endpoints: BTreeMap<u32, (usize, usize, usize)> = BTreeMap::new();
        let mut vmos: BTreeMap<u32, usize> = BTreeMap::new();
        let mut count = |cap: Capability, holder: Option<u32>| match cap.kind {
            CapabilityKind::Endpoint(id) => {
                let entry = endpoints.entry(id).or_default();
                // The owner is never its own peer; a capability in flight always is one.
                let foreign = holder.is_none() || holder != owner_of(id as usize);
                entry.0 += usize::from(cap.rights.contains(Rights::SEND) && foreign);
                entry.1 += usize::from(cap.rights.contains(Rights::RECV));
                entry.2 += 1;
            }
            CapabilityKind::Vmo { id, .. } | CapabilityKind::VmoRo { id, .. } => {
                *vmos.entry(id).or_default() += 1;
            }
            _ => {}
        };
        for (t, table) in self.tables.iter().enumerate() {
            for slot in 0..table.capacity() {
                if let Ok(Some(cap)) = table.peek(slot) {
                    count(cap, Some(pid_of(t)));
                }
            }
        }
        for held in &self.flight {
            count(held.cap(), None);
        }
        (endpoints, vmos)
    }

    fn assert_counts_equal_the_census(&self, step: usize) {
        let (endpoints, vmos) = self.census();
        for (e, counts) in self.endpoints.iter().enumerate() {
            let expected = endpoints.get(&(e as u32)).copied().unwrap_or_default();
            let got = (counts.foreign_senders(), counts.receivers(), counts.any());
            assert_eq!(got, expected, "endpoint {e} after step {step}");
        }
        for (v, counts) in self.vmos.iter().enumerate() {
            let expected = vmos.get(&(v as u32 + 1)).copied().unwrap_or_default();
            assert_eq!(counts.any(), expected, "memory object {} after step {step}", v + 1);
        }
    }
}

#[test]
fn the_counts_equal_a_census_after_every_one_of_10_000_random_operations() {
    let mut world = World::new();
    let mut rng = Rng(0x0330_0005_c0ff_ee01);
    for step in 0..10_000 {
        world.step(&mut rng);
        world.assert_counts_equal_the_census(step);
    }
    // The walk reached every door: something is held, something is in flight.
    assert!(world.endpoints.iter().any(|c| c.any() > 0) && world.vmos.iter().any(|c| c.any() > 0));
    // Every table and every message gone: nothing is held any more.
    world.tables.clear();
    world.flight.clear();
    for counts in world.endpoints.iter().chain(world.vmos.iter()) {
        assert_eq!((counts.any(), counts.foreign_senders(), counts.receivers()), (0, 0, 0));
    }
}

#[test]
fn the_owner_of_an_endpoint_is_never_its_own_peer() {
    let counts = HeldCounts::endpoint(7, Some(1)).unwrap();
    let (mut owner, mut peer) = (CapTable::new_for(1), CapTable::new_for(2));
    // The owner's base: send and receive, in its own table.
    let base = owner.allocate(counts.mint(Rights::SEND | Rights::RECV)).unwrap();
    assert_eq!((counts.foreign_senders(), counts.receivers(), counts.any()), (0, 1, 1));
    // One reply capability per exchange: cloned, then moved to the peer.
    let reply = owner.allocate(owner.dup(base, Rights::SEND).unwrap()).unwrap();
    assert_eq!(counts.foreign_senders(), 0, "still in the owner's table");
    let travelling = owner.take_held(reply).unwrap();
    assert_eq!(counts.foreign_senders(), 1, "in flight: the peer's to be");
    let slot = peer.allocate(travelling).unwrap();
    assert_eq!((counts.foreign_senders(), counts.any()), (1, 2));
    // The peer closes it: the last foreign sender is gone, the owner's base remains.
    assert_eq!(peer.take(slot).map(|cap| cap.rights), Ok(Rights::SEND));
    assert_eq!((counts.foreign_senders(), counts.receivers(), counts.any()), (0, 1, 1));
    // A send capability that comes home is the owner's own again.
    let home = peer.allocate(counts.mint(Rights::SEND)).unwrap();
    assert_eq!(counts.foreign_senders(), 1);
    let back = peer.take_held(home).unwrap();
    owner.allocate(back).unwrap();
    assert_eq!((counts.foreign_senders(), counts.any()), (0, 2));
    // The owner exits: its table goes at once.
    drop(owner);
    assert_eq!((counts.foreign_senders(), counts.receivers(), counts.any()), (0, 0, 0));
}

/// No owner recorded: every send capability keeps the channel connected (fail-safe — never a
/// spurious end on an endpoint the kernel cannot attribute); a receiver remains while a
/// receive capability is held, in a table or in flight.
#[test]
fn test_reject_unowned_endpoint_counts_every_sender() {
    let counts = HeldCounts::endpoint(5, None).unwrap();
    let mut table = CapTable::new_for(7);
    let slot = table.allocate(counts.mint(Rights::SEND | Rights::RECV)).unwrap();
    assert_eq!((counts.foreign_senders(), counts.receivers()), (1, 1));
    let travelling = table.take_held(slot).unwrap();
    assert_eq!((counts.foreign_senders(), counts.receivers()), (1, 1), "in flight it is held");
    drop(travelling);
    assert_eq!((counts.foreign_senders(), counts.receivers(), counts.any()), (0, 0, 0));
}

#[test]
fn a_move_changes_no_count() {
    let counts = HeldCounts::endpoint(3, None).unwrap();
    let (mut sender, mut receiver) = (CapTable::new_for(1), CapTable::new_for(2));
    let slot = sender.allocate(counts.mint(Rights::SEND | Rights::RECV)).unwrap();
    let before = (counts.foreign_senders(), counts.receivers(), counts.any());
    let travelling = sender.take_held(slot).unwrap();
    assert_eq!((counts.foreign_senders(), counts.receivers(), counts.any()), before);
    let descriptor = travelling.cap();
    let landed = receiver.allocate(travelling).unwrap();
    assert_eq!((counts.foreign_senders(), counts.receivers(), counts.any()), before);
    assert_eq!(receiver.get(landed), Ok(descriptor), "the same kind and rights it left with");
}

#[test]
fn a_refused_install_counts_the_capability_off() {
    let counts = HeldCounts::vmo(9, 0x4000).unwrap();
    let mut table = CapTable::with_capacity(1);
    table.allocate(counts.mint(Rights::MAP)).unwrap();
    let refuses = crate::kheap::refusal::PoolRefuses::new();
    // A fresh capability the full table refuses is dropped.
    assert_eq!(table.allocate(counts.mint(Rights::MAP)), Err(CapError::NoSpace));
    assert_eq!(table.set(5, counts.mint(Rights::MAP)), Err(CapError::InvalidSlot));
    assert_eq!(table.set_if_empty(0, counts.mint(Rights::MAP)), Err(CapError::PermissionDenied));
    assert_eq!(counts.any(), 1);
    // One that travelled is handed back: it stays with its message.
    let (err, back) = table.try_allocate(counts.mint(Rights::MAP)).unwrap_err();
    assert_eq!((err, counts.any()), (CapError::NoSpace, 2));
    drop((back, refuses));
    // An overwrite drops what the slot held.
    table.set(0, counts.mint(Rights::MAP)).unwrap();
    assert_eq!(counts.any(), 1);
}

/// A counted kind comes into being at its object or from a capability to it — never from a
/// number someone names.
#[test]
fn test_reject_a_counted_kind_made_from_a_number() {
    for kind in [
        CapabilityKind::Endpoint(3),
        CapabilityKind::Vmo { id: 1, len: 0x1000 },
        CapabilityKind::VmoRo { id: 1, len: 0x1000 },
    ] {
        let forged = Held::plain(Capability { kind, rights: Rights::all() });
        assert!(matches!(forged, Err(CapError::PermissionDenied)), "{kind:?}");
    }
}

/// A second capability never has a right the first lacks, and a refusal counts nothing.
#[test]
fn test_reject_dup_beyond_the_rights_held() {
    let counts = HeldCounts::endpoint(4, None).unwrap();
    let send_only = counts.mint(Rights::SEND);
    for more in [Rights::RECV, Rights::MANAGE, Rights::SEND | Rights::RECV, Rights::all()] {
        assert!(matches!(send_only.dup(more), Err(CapError::PermissionDenied)), "{more:?}");
    }
    assert_eq!((counts.foreign_senders(), counts.receivers(), counts.any()), (1, 0, 1));
    let same = send_only.dup(Rights::SEND).unwrap();
    assert_eq!(same.cap(), send_only.cap(), "the same kind, the rights asked for");
    let none = send_only.dup(Rights::empty()).unwrap();
    assert_eq!(
        (none.cap().rights, counts.foreign_senders(), counts.any()),
        (Rights::empty(), 2, 3)
    );
}

/// A read-only alias stays one: its copies are aliases, it has no alias of its own to climb
/// by, and no door turns it into the writable capability.
#[test]
fn test_reject_a_writable_capability_from_a_read_only_alias() {
    let counts = HeldCounts::vmo(2, 0x2000).unwrap();
    let writable = counts.mint(Rights::MAP);
    let alias = writable.read_only_alias(Rights::MAP).unwrap();
    let read_only = CapabilityKind::VmoRo { id: 2, len: 0x2000 };
    assert_eq!(alias.cap(), Capability { kind: read_only, rights: Rights::MAP });
    assert_eq!(alias.dup(Rights::MAP).unwrap().cap().kind, read_only);
    assert!(matches!(alias.read_only_alias(Rights::MAP), Err(CapError::PermissionDenied)));
    assert!(matches!(
        writable.read_only_alias(Rights::MAP | Rights::MANAGE),
        Err(CapError::PermissionDenied)
    ));
    let endpoint = HeldCounts::endpoint(1, None).unwrap().mint(Rights::all());
    assert!(matches!(endpoint.read_only_alias(Rights::MAP), Err(CapError::PermissionDenied)));
    assert!(matches!(factory().read_only_alias(Rights::MANAGE), Err(CapError::PermissionDenied)));
    assert_eq!(counts.any(), 2, "the writable one and its alias");
}

/// The kernel's own objects are handed out read-only from the start: the read-only mint is
/// counted like any capability, never climbs to a writable one, and an endpoint has none.
#[test]
fn test_reject_a_read_only_mint_that_climbs_or_names_an_endpoint() {
    let counts = HeldCounts::vmo(3, 0x1000).unwrap();
    let alias = counts.mint_read_only(Rights::MAP).unwrap();
    let read_only = CapabilityKind::VmoRo { id: 3, len: 0x1000 };
    assert_eq!(alias.cap(), Capability { kind: read_only, rights: Rights::MAP });
    assert_eq!(counts.any(), 1);
    assert_eq!(alias.dup(Rights::MAP).unwrap().cap().kind, read_only);
    assert!(matches!(alias.read_only_alias(Rights::MAP), Err(CapError::PermissionDenied)));
    assert!(matches!(alias.dup(Rights::MAP | Rights::MANAGE), Err(CapError::PermissionDenied)));
    assert!(HeldCounts::endpoint(1, None).unwrap().mint_read_only(Rights::MAP).is_none());
    drop(alias);
    assert_eq!(counts.any(), 0);
}

/// An object whose header the heap refuses is not made (ADR-0091).
#[test]
fn test_reject_a_header_while_the_pool_refuses() {
    let _refuses = crate::kheap::refusal::PoolRefuses::new();
    assert!(HeldCounts::endpoint(1, Some(1)).is_none());
    assert!(HeldCounts::vmo(1, 0x1000).is_none());
}
