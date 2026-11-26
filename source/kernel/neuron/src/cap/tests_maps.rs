// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(test)]
//! CONTEXT: Host tests of the count a memory object keeps of the address spaces that map it
//! (RFC-0102, TASK-0330 P3): `Mapped` at the header, `SpaceMaps` per space
//! OWNERS: @kernel-cap-team
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 4 tests
//! NOTE: Tests only; no kernel logic. The two doors a space asks (`first`, `upgrade`) are the
//!   memory-object table's in the kernel (`mm/vmo/mapping.rs`); here they answer from the test.
//!
//! TEST_SCENARIOS:
//!   - a_memory_object_counts_the_spaces_that_map_it
//!   - test_reject_a_mapping_of_an_endpoint
//!   - test_reject_a_mapper_or_a_writer_the_object_refuses
//!   - test_reject_a_mapping_while_the_pool_refuses

use alloc::sync::Arc;

use super::{HeldCounts, Mapped, SpaceMaps};

/// What the object table answers a space with: a token of `counts`, and a writer allowed.
fn doors(
    counts: &Arc<HeldCounts>,
    writable: bool,
) -> (
    impl FnOnce() -> Result<Mapped, &'static str> + '_,
    impl FnOnce(&mut Mapped) -> Result<(), &'static str>,
) {
    (
        move || counts.map(writable).ok_or("no object"),
        |token: &mut Mapped| {
            token.set_writable(true);
            Ok(())
        },
    )
}

fn add(maps: &mut SpaceMaps, counts: &Arc<HeldCounts>, id: u32, writable: bool) {
    let (first, upgrade) = doors(counts, writable);
    maps.add(id, writable, first, upgrade, "heap").expect("recorded");
}

/// An object counts the address spaces that map it, not their regions, and those that hold a
/// writable region of it; the last region of a space counts the space off, a space that goes
/// takes its counts with it.
#[test]
fn a_memory_object_counts_the_spaces_that_map_it() {
    let object = HeldCounts::vmo(7, 0x4000).unwrap();
    let other = HeldCounts::vmo(8, 0x1000).unwrap();
    let (mut a, mut b) = (SpaceMaps::new(), SpaceMaps::new());
    assert_eq!((object.mapped(), object.mapped_writable()), (0, 0));

    add(&mut a, &object, 7, false);
    add(&mut a, &object, 7, false);
    assert_eq!((object.mapped(), object.mapped_writable()), (1, 0), "two regions, one space");
    add(&mut a, &object, 7, true);
    add(&mut b, &object, 7, false);
    add(&mut b, &other, 8, true);
    assert_eq!((object.mapped(), object.mapped_writable()), (2, 1));
    assert_eq!((other.mapped(), other.mapped_writable()), (1, 1));

    a.remove(7, true);
    assert_eq!((object.mapped(), object.mapped_writable()), (2, 0), "its last writable region");
    a.remove(7, false);
    assert_eq!(object.mapped(), 2, "a region of the space remains");
    a.remove(7, false);
    assert_eq!(object.mapped(), 1, "the space's last region");
    a.remove(7, false);
    assert_eq!(object.mapped(), 1, "a region the space does not have changes nothing");

    drop(b);
    assert_eq!((object.mapped(), object.mapped_writable()), (0, 0));
    assert_eq!((other.mapped(), other.mapped_writable()), (0, 0));
    assert_eq!(object.any(), 0, "a mapping is no capability");
}

/// Nothing maps an endpoint.
#[test]
fn test_reject_a_mapping_of_an_endpoint() {
    let endpoint = HeldCounts::endpoint(3, None).unwrap();
    assert!(endpoint.map(false).is_none() && endpoint.map(true).is_none());
    assert_eq!((endpoint.mapped(), endpoint.mapped_writable()), (0, 0));
}

/// A mapping the object refuses leaves nothing behind: no first region of an object that
/// refuses the space, no writable region of one that refuses a writer (a sealed object) —
/// the space keeps what it had.
#[test]
fn test_reject_a_mapper_or_a_writer_the_object_refuses() {
    let object = HeldCounts::vmo(7, 0x4000).unwrap();
    let mut maps = SpaceMaps::new();
    let refused: Result<(), &str> = maps.add(7, false, || Err("unlocked"), |_| Ok(()), "heap");
    assert_eq!(refused, Err("unlocked"));
    assert_eq!(object.mapped(), 0);

    add(&mut maps, &object, 7, false);
    let (first, _) = doors(&object, true);
    let sealed = maps.add(7, true, first, |_| Err("sealed"), "heap");
    assert_eq!(sealed, Err("sealed"));
    assert_eq!((object.mapped(), object.mapped_writable()), (1, 0), "still its one reader");
    // The refused region was not counted: one remove gives the space's token back.
    maps.remove(7, false);
    assert_eq!(object.mapped(), 0);
}

/// A space whose record of one more object the heap refuses maps it not (ADR-0091): no token
/// is taken for an entry that cannot be kept.
#[test]
fn test_reject_a_mapping_while_the_pool_refuses() {
    let object = HeldCounts::vmo(7, 0x4000).unwrap();
    let mut maps = SpaceMaps::new();
    let refuses = crate::kheap::refusal::PoolRefuses::new();
    let (first, upgrade) = doors(&object, true);
    assert_eq!(maps.add(7, true, first, upgrade, "heap"), Err("heap"));
    assert_eq!((object.mapped(), object.mapped_writable()), (0, 0));
    drop(refuses);
    add(&mut maps, &object, 7, true);
    assert_eq!((object.mapped(), object.mapped_writable()), (1, 1));
}
