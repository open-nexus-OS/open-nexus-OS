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

/// Resident, contiguous-DMA and reserved bytes of one address space.
#[derive(Clone, Copy, Debug, Default)]
pub struct SpaceUsage {
    /// Committed bytes: every page of an eager region, the touched pages of a lazy one.
    pub rss: u64,
    pub dma: u64,
    /// Bytes of lazy regions not yet touched (ADR-0077).
    pub reserved: u64,
}

/// What every address space together holds.
#[derive(Clone, Copy, Debug, Default)]
pub struct SpacesSummary {
    pub spaces: u64,
    pub rss_sum: u64,
    pub rss_max: u64,
}

/// How many spaces map each object (TASK-0333 6e, ADR-0085): `(vmo id, mappers)`, read once
/// per snapshot from every region table — an object with two or more is shared.
pub struct Mappers(alloc::vec::Vec<(u32, u32)>);

impl Mappers {
    /// Count over every space.
    pub fn census(spaces: &AddressSpaceManager) -> Self {
        let mut counts: alloc::vec::Vec<(u32, u32)> = alloc::vec::Vec::new();
        for space in spaces.iter() {
            // One space mapping an object twice is still one mapper.
            let mut seen: alloc::vec::Vec<u32> = alloc::vec::Vec::new();
            for region in space.va_space().regions() {
                if region.vmo == 0 || seen.contains(&region.vmo) {
                    continue;
                }
                seen.push(region.vmo);
                match counts.iter_mut().find(|(id, _)| *id == region.vmo) {
                    Some((_, n)) => *n += 1,
                    None => counts.push((region.vmo, 1)),
                }
            }
        }
        Mappers(counts)
    }

    /// Whether another space maps `vmo` too.
    pub fn is_shared(&self, vmo: u32) -> bool {
        self.0.iter().any(|(id, n)| *id == vmo && *n >= 2)
    }
}

/// The resident bytes of `space` over objects another space maps too.
pub fn shared_bytes(space: &AddressSpace, mappers: &Mappers) -> u64 {
    let mut shared = 0u64;
    for region in space.va_space().regions() {
        if region.kind == RegionKind::Mmio || region.vmo == 0 || !mappers.is_shared(region.vmo) {
            continue;
        }
        shared += if region.lazy {
            super::vmo::committed_in(region.vmo, region.offset, region.len) as u64
        } else {
            region.len as u64
        };
    }
    shared
}

/// One space's resident bytes, from its region table.
pub fn space_usage(space: &AddressSpace) -> SpaceUsage {
    let mut usage = SpaceUsage::default();
    for region in space.va_space().regions() {
        match region.kind {
            RegionKind::Mmio => {}
            // A region over a lazy object costs what it touched (ADR-0077).
            _ if region.lazy => {
                let committed = super::vmo::committed_in(region.vmo, region.offset, region.len);
                usage.rss += committed as u64;
                usage.reserved += region.len.saturating_sub(committed) as u64;
            }
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

/// Each followed process's committed high-water (`crate::growth`).
static MARKS: spin::Mutex<crate::growth::Marks> = spin::Mutex::new(crate::growth::Marks::new());

/// At a reap (TASK-0333 6b-3): every live space measured against the high-water last reported
/// for its process; one that reached a new high by a step is named — `KINIT: mm grew (pid= sid=
/// rss= by=)`. The first sight of a process sets its mark silently.
pub fn report_growth(tasks: &crate::task::TaskTable, spaces: &AddressSpaceManager) {
    let mut marks = MARKS.lock();
    for index in 0..tasks.len() {
        let Some(task) = tasks.task(crate::types::Pid::from_raw(index as u32)) else { continue };
        let Some(space) = task.address_space().and_then(|h| spaces.get(h).ok()) else {
            continue;
        };
        let (rss, sid) = (space_usage(space).rss, task.service_id());
        if let Some(by) = marks.note(index as u32, sid, rss) {
            log_info!(target: "mm", "KINIT: mm grew (pid={} sid={:#x} rss={} by={})", index, sid, rss, by);
        }
    }
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
    let (vmos, vmo_bytes, dma_bytes, committed) = super::vmo::stats();
    (stats.vmos, stats.vmo_bytes, stats.dma_bytes, stats.vmo_committed) =
        (vmos as u64, vmo_bytes as u64, dma_bytes as u64, committed as u64);
    if let Some(space) = own.and_then(|h| spaces.get(h).ok()) {
        let usage = space_usage(space);
        (stats.own_rss_bytes, stats.own_dma_bytes, stats.own_reserved_bytes) =
            (usage.rss, usage.dma, usage.reserved);
        stats.own_shared_bytes = shared_bytes(space, &Mappers::census(spaces));
    }
    // ADR-0085: the level and its watermarks, from the one tracker over the pool.
    if let Some((level, marks)) = super::pressure_endpoint::state() {
        stats.pressure_level = level as u64;
        (stats.watermark_warn, stats.watermark_critical, stats.watermark_emergency) =
            (marks.warn, marks.critical, marks.emergency);
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
