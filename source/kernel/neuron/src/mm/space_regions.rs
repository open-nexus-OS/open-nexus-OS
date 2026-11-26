// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: a region and its object's count change together (RFC-0102, TASK-0330 P3). Every
//! write to an address space's region table goes through here: a region that names a memory
//! object is recorded only with the space counted at the object (`cap::SpaceMaps`, the token
//! from `vmo::mapping` — refused for an object that is gone, unlocked for purging, or sealed
//! against this writer), and forgotten with the count given back. `AddressSpace` hands out
//! its region table for writing to this module only, so no path can record a region the
//! object does not know of — the object's frames are never freed under a mapping.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the policy host-proven (`va_space_tests`, `cap/tests_prop.rs`); QEMU —
//!   `KSELFTEST: vm map ok / vm unmap ok / vm map reject ok`, `KSELFTEST: exec image …`
//! INVARIANTS: count, then record; forget, then give the count back; a refusal of either
//!   leaves neither behind

use super::address_space::AddressSpace;
use super::page_table::PageFlags;
use super::vmo::mapping::{self, MapRefusal};
use crate::cap::SpaceMaps;
use crate::va_space::{VaError, VaRegion};

/// A region was not recorded: the region table refused it, or its object did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordError {
    Va(VaError),
    Object(MapRefusal),
}

fn writable(flags: usize) -> bool {
    flags & PageFlags::WRITE.bits() != 0
}

fn count(maps: &mut SpaceMaps, id: u32, writable: bool) -> Result<(), RecordError> {
    if id == 0 {
        return Ok(());
    }
    maps.add(
        id,
        writable,
        || mapping::begin(id, writable),
        |token| mapping::make_writable(id, token),
        MapRefusal::Heap,
    )
    .map_err(RecordError::Object)
}

/// Records `region`; one that names an object counts this space at it first.
pub fn record(space: &mut AddressSpace, region: VaRegion) -> Result<(), RecordError> {
    let (regions, maps) = space.regions_mut();
    count(maps, region.vmo, writable(region.flags))?;
    regions.insert(region).map_err(|err| {
        maps.remove(region.vmo, writable(region.flags));
        RecordError::Va(err)
    })
}

/// Records kernel-placed memory backed by the lazy object `vmo` (`VaSpace::record_lazy`).
pub fn record_lazy(
    space: &mut AddressSpace,
    va: usize,
    len: usize,
    flags: usize,
    vmo: u32,
) -> Result<(), RecordError> {
    let (regions, maps) = space.regions_mut();
    count(maps, vmo, writable(flags))?;
    regions.record_lazy(va, len, flags, vmo).map_err(|err| {
        maps.remove(vmo, writable(flags));
        RecordError::Va(err)
    })
}

/// Marks the exact region `(va, len)` as going (`VaSpace::begin_unmap`): still recorded, still
/// counted at its object — its frames stay until the forget.
pub fn begin_unmap(space: &mut AddressSpace, va: usize, len: usize) -> Result<VaRegion, VaError> {
    space.regions_mut().0.begin_unmap(va, len)
}

/// Forgets the exact region `(va, len)` and gives its object's count back.
pub fn forget(space: &mut AddressSpace, va: usize, len: usize) -> Result<VaRegion, VaError> {
    let (regions, maps) = space.regions_mut();
    let region = regions.remove_exact(va, len)?;
    maps.remove(region.vmo, writable(region.flags));
    Ok(region)
}
