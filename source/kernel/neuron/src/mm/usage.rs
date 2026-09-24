// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: collects `accounting::MmStats` from the owners of the truth
//! (RFC-0098 C4, TASK-0286 P5): the frame pool, the page-table allocator, the
//! VMO table and the region table of every address space. Nothing here counts
//! on its own — every number is read where it already lives.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the record and its checks are host-tested in
//!   `crate::accounting`; this read is proven by `KSELFTEST: mm frames (…)` and
//!   `metricsd: mm snapshot ok (…)` on every QEMU boot
//! INVARIANTS: resident bytes are VMO and kernel-placed regions (a device window
//!   is not memory); a shared VMO counts in every space that maps it (RSS).

use super::address_space::{AddressSpace, AddressSpaceManager, AsHandle};
use crate::accounting::MmStats;
use crate::va_space::RegionKind;

/// Resident and contiguous-DMA bytes of one address space.
#[derive(Clone, Copy, Debug, Default)]
pub struct SpaceUsage {
    pub rss: u64,
    pub dma: u64,
}

/// What every address space together holds.
#[derive(Clone, Copy, Debug, Default)]
pub struct SpacesSummary {
    pub spaces: u64,
    pub rss_sum: u64,
    pub rss_max: u64,
}

/// One space's resident bytes, from its region table.
pub fn space_usage(space: &AddressSpace) -> SpaceUsage {
    let mut usage = SpaceUsage::default();
    for region in space.va_space().regions() {
        match region.kind {
            RegionKind::Mmio => {}
            RegionKind::Fixed => usage.rss += region.len as u64,
            RegionKind::Vmo => {
                usage.rss += region.len as u64;
                if super::vmo::is_contiguous(region.vmo) {
                    usage.dma += region.len as u64;
                }
            }
        }
    }
    usage
}

/// The record for a caller whose space is `own` (none: the kernel's view), and
/// the summary over every space.
pub fn snapshot(spaces: &AddressSpaceManager, own: Option<AsHandle>) -> (MmStats, SpacesSummary) {
    let mut stats = MmStats::default();
    if let Some(pool) = super::frame_pool::stats() {
        stats.banks = pool.banks as u64;
        stats.total = pool.total as u64;
        stats.free = pool.free as u64;
        stats.reserved = pool.reserved as u64;
        stats.excluded = pool.excluded as u64;
        stats.allocs = pool.allocs;
        stats.frees = pool.frees;
        stats.exhausted = pool.exhausted;
    }
    stats.pt_frames = super::page_table::PageTable::allocation_stats().live as u64;
    let (vmos, vmo_bytes, dma_bytes) = super::vmo::stats();
    (stats.vmos, stats.vmo_bytes, stats.dma_bytes) =
        (vmos as u64, vmo_bytes as u64, dma_bytes as u64);
    if let Some(space) = own.and_then(|h| spaces.get(h).ok()) {
        let usage = space_usage(space);
        (stats.own_rss_bytes, stats.own_dma_bytes) = (usage.rss, usage.dma);
    }
    let mut summary = SpacesSummary::default();
    for space in spaces.iter() {
        let rss = space_usage(space).rss;
        summary.spaces += 1;
        summary.rss_sum += rss;
        summary.rss_max = summary.rss_max.max(rss);
    }
    (stats, summary)
}
