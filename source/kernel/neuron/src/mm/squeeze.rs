// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: one batch of a squeeze (RFC-0100 §5, TASK-0333 6f-1, ADR-0087) over a parked child's
//! address space — the space side of `task_squeeze` (69). The child's lazy regions are walked in
//! address order from the caller's cursor; a page of an object the rule allows (the syscall shell
//! decides `allowed`: `vmo_lifetime::squeeze_allowed` over the child's own objects) is handed to
//! `vmo::squeeze::squeeze_page`. A batch examines at most [`BATCH_PAGES`] committed pages and
//! scans at most [`SCAN_PAGES`] page words, so the kernel lock is never held for a whole process
//! (16 pages ≈ 0.8 ms on the board's core, 6f-0's measurement). The PTE of every page that gave
//! its frame up is cleared, ONE shootdown runs with the kernel lock held (the memory paths' form;
//! idle harts flush at their next dispatch), and only then do the frames go back to the pool —
//! no hart can reach a frame the pool handed on.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional (TASK-0333 6f-1)
//! API_STABILITY: Internal (the record is `nexus_abi::SqueezeBatch` v1)
//! TEST_COVERAGE: `KSELFTEST: task squeeze ok (…)` on every lane and the board (the PTEs cleared,
//!   the pool's frames back, every page back byte-exact); `SELFTEST: exec child squeeze ok (…)`
//! INVARIANTS: called with the kernel lock held, the child parked; a frame is freed only after
//!   the shootdown that follows its PTE's clearing; user memory never starts at address 0 (the
//!   cursor's `0` is "done").

use alloc::vec::Vec;

use super::address_space::AddressSpace;
use super::frame_pool;
use super::page_table::PAGE_SIZE;
use super::vmo::squeeze::{squeeze_page, PageOutcome};
use crate::frames::Block;
use crate::lazy::page_index;
use crate::lazy::store::Spare;
use crate::va_space::VaRegion;

/// Committed pages one batch examines at most.
pub const BATCH_PAGES: usize = 16;
/// Page words one batch scans at most (absent and squeezed pages cost a look each).
pub const SCAN_PAGES: usize = 4096;
/// The batch record's layout version.
pub const BATCH_VERSION: u32 = 1;
/// The record's 64-bit fields.
pub const BATCH_FIELDS: u32 = 5;
/// The record's bytes.
pub const BATCH_BYTES: usize = 8 + BATCH_FIELDS as usize * 8;

/// What one batch did — the record `task_squeeze` writes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Batch {
    /// Pages recorded in their object's store.
    pub squeezed: u64,
    /// Pages of zeroes uncommitted.
    pub zero: u64,
    /// Committed pages kept as they are (their block over a record, or no frame for the store).
    pub kept: u64,
    /// Frames the stores took.
    pub taken: u64,
    /// Bytes of the records written (length fields and blocks).
    pub stored: u64,
}

impl Batch {
    /// The record: version, field count, the fields in order, little-endian.
    #[must_use]
    pub fn encode(&self) -> [u8; BATCH_BYTES] {
        let mut out = [0u8; BATCH_BYTES];
        out[0..4].copy_from_slice(&BATCH_VERSION.to_le_bytes());
        out[4..8].copy_from_slice(&BATCH_FIELDS.to_le_bytes());
        let fields = [self.squeezed, self.zero, self.kept, self.taken, self.stored];
        for (i, value) in fields.iter().enumerate() {
            out[8 + i * 8..16 + i * 8].copy_from_slice(&value.to_le_bytes());
        }
        out
    }
}

/// One batch from `from_va` over the lazy regions of `space` whose object `allowed` names.
/// Answers the address the next batch starts at, `0` when the walk reached the end.
pub fn run(space: &mut AddressSpace, from_va: usize, allowed: &[u32], batch: &mut Batch) -> usize {
    let regions: Vec<VaRegion> = space
        .va_space()
        .regions()
        .iter()
        .filter(|r| r.lazy && allowed.contains(&r.vmo) && r.va + r.len > from_va)
        .copied()
        .collect();
    let mut spare = Spare::new();
    let mut freed: Vec<u64> = Vec::with_capacity(BATCH_PAGES);
    let (mut examined, mut scanned, mut next) = (0usize, 0usize, 0usize);
    'walk: for region in &regions {
        let mut va = from_va.max(region.va);
        while va < region.va + region.len {
            if examined == BATCH_PAGES || scanned == SCAN_PAGES {
                next = va;
                break 'walk;
            }
            scanned += 1;
            // A record lands in at most two store pages: two frames ready, taken before the
            // object table's lock (what is left goes back below).
            while spare.missing() > 0 {
                let Ok(block) = frame_pool::alloc(0) else { break };
                if spare.put(block.base).is_err() {
                    frame_pool::free(block);
                    break;
                }
            }
            let held = spare.held();
            let index = page_index(region.va, region.offset, va);
            let outcome = squeeze_page(region.vmo, index, &mut spare);
            batch.taken += (held - spare.held()) as u64;
            let frame = match outcome {
                PageOutcome::Untouched => None,
                PageOutcome::Kept => {
                    batch.kept += 1;
                    None
                }
                PageOutcome::Zero(frame) => {
                    batch.zero += 1;
                    Some(frame)
                }
                PageOutcome::Squeezed { frame, bytes } => {
                    batch.squeezed += 1;
                    batch.stored += bytes as u64;
                    Some(frame)
                }
            };
            if outcome != PageOutcome::Untouched {
                examined += 1;
            }
            if let Some(frame) = frame {
                // The PTE now (a page committed but never mapped here has none); the frame after
                // the shootdown.
                let _ = space.page_table_mut().unmap_leaf(va);
                freed.push(frame);
            }
            va += PAGE_SIZE;
        }
    }
    if !freed.is_empty() {
        crate::smp::tlb::shootdown_all();
    }
    for pa in freed.into_iter().chain(spare.drain()) {
        frame_pool::free(Block { base: pa, order: 0 });
    }
    next
}
