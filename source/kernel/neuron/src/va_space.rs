// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the per-address-space VA allocation POLICY (RFC-0085) — which
//! virtual addresses are taken, which are free, and where the next mapping
//! goes. The kernel picks addresses; userspace receives them. Before this,
//! userspace invented VAs (gpud hand-carved ~450 MiB across two
//! never-freeing bump allocators; six components copied the same MMIO
//! window constant) and the page table was the only record — no hole
//! finding, no unmap, overlaps discovered by the kernel refusing a PTE
//! tens of megabytes into a loop.
//!
//! NOT target-gated (pure usize logic) so the tests run on host — same
//! rationale as `image_allocs`/`vmo_ro`. This module is the HOST-PROVEN
//! half; the syscall layer (`syscall/api/vm_map.rs`) is a thin shell that
//! feeds it capability-derived facts and executes its decisions against
//! the page table.
//!
//! OWNERS: @kernel-mm-team
//! STATUS: Functional (RFC-0085 Phase 1)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests below — the proofs that would catch a wrong
//!   design (determinism golden sequence, first-fit reuse, phase-aligned
//!   holes, exhaustion leaves state untouched, bounded fuzz invariant)
//! INVARIANTS: regions pairwise disjoint, va-sorted, in-window for
//!   kernel-chosen kinds; fixed capacity — a `vm_map` that cannot be
//!   RECORDED is REFUSED (never map without a record); Fixed records that
//!   do not fit are COUNTED, never silently forgotten.

/// Regions tracked per address space. Post-migration worst case (gpud) is
/// ~33: MMIO + 6 queue/pool windows + 8 2D resources + 12 virgl backings +
/// framebuffer + ~5 exec-time Fixed records. 64 = 2x headroom, 2.5 KiB
/// embedded per address space, no kernel-heap churn.
pub const MAX_REGIONS: usize = 64;

const PAGE: usize = 4096;
/// One page, exported for the tracked-map shell.
pub const PAGE_LEN: usize = PAGE;

/// What a region is backed by — determines who may unmap it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegionKind {
    /// `vm_map`-created VMO mapping (userspace may `vm_unmap`).
    Vmo,
    /// `mmio_map_auto`-created device window (userspace may `vm_unmap`).
    Mmio,
    /// Kernel-placed: ELF segments, stack, meta/info pages, legacy fixed-VA
    /// maps. NOT unmappable from userspace — the loader's memory is kernel
    /// property.
    Fixed,
}

/// One tracked mapping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VaRegion {
    pub va: usize,
    pub len: usize,
    /// Backing physical base — the `vmo_destroy` guard needs to answer
    /// "does this address space still map that physical range?".
    pub pa: usize,
    /// PageFlags bits as plain usize (keeps the module pure).
    pub flags: usize,
    pub kind: RegionKind,
}

/// Why an operation was refused. Every variant maps to exactly one errno at
/// the syscall boundary (ADR-0054: no wildcard arms).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VaError {
    /// No hole of the requested size/alignment left in the window (ENOMEM).
    WindowExhausted,
    /// The region table is full (ENOSPC). State is untouched.
    TableFull,
    /// Unmap: nothing recorded at this va (ENOENT).
    NotFound,
    /// Unmap: a region exists at this va but with a different length —
    /// v1 unmaps exact whole regions only (EINVAL).
    LenMismatch,
    /// Unmap: the region is `Fixed` — kernel property (EPERM).
    FixedRegion,
    /// Malformed input: zero length, unaligned, or arithmetic overflow
    /// (EINVAL).
    BadInput,
    /// A caller-chosen VA overlaps a tracked region or lies inside the
    /// kernel-managed window (EEXIST / EPERM at the boundary).
    Occupied,
}

/// The per-address-space region table + the managed window it allocates in.
///
/// The window bounds are FIELDS (not consts) so the policy is testable with
/// small windows on the host; the kernel constructs it with
/// `mm::USER_VM_WINDOW`.
#[derive(Clone, Copy)]
pub struct VaSpace {
    regions: [VaRegion; MAX_REGIONS],
    len: usize,
    /// Fixed records that did not fit — counted, never forgotten.
    untracked: usize,
    /// Peak region count (observability: exhaustion must be visible before
    /// it bites).
    peak: usize,
    window_base: usize,
    window_len: usize,
}

const EMPTY: VaRegion = VaRegion { va: 0, len: 0, pa: 0, flags: 0, kind: RegionKind::Fixed };

impl VaSpace {
    #[must_use]
    pub const fn new(window_base: usize, window_len: usize) -> Self {
        Self {
            regions: [EMPTY; MAX_REGIONS],
            len: 0,
            untracked: 0,
            peak: 0,
            window_base,
            window_len,
        }
    }

    #[must_use]
    pub fn region_count(&self) -> usize {
        self.len
    }

    #[must_use]
    pub fn peak_regions(&self) -> usize {
        self.peak
    }

    #[must_use]
    pub fn untracked_fixed(&self) -> usize {
        self.untracked
    }

    /// `(va, len)` of the i-th region in va-order (observability + the fuzz
    /// invariant walk). Out of range returns `(0, 0)`.
    #[must_use]
    pub fn region_at(&self, index: usize) -> (usize, usize) {
        self.regions[..self.len].get(index).map_or((0, 0), |r| (r.va, r.len))
    }

    fn window_end(&self) -> usize {
        self.window_base + self.window_len
    }

    /// The regions overlapping `[va, va+len)`, as an index into the table.
    fn first_overlap(&self, va: usize, len: usize) -> Option<usize> {
        let end = va.checked_add(len)?;
        self.regions[..self.len].iter().position(|r| va < r.va + r.len && r.va < end)
    }

    /// Largest free hole in the window (observability for ENOMEM logs).
    #[must_use]
    pub fn largest_free_hole(&self) -> usize {
        let mut cursor = self.window_base;
        let mut best = 0;
        for r in self.regions[..self.len].iter().filter(|r| r.va >= self.window_base) {
            if r.va > cursor {
                best = best.max(r.va - cursor);
            }
            cursor = cursor.max(r.va + r.len);
        }
        best.max(self.window_end().saturating_sub(cursor))
    }

    /// Lowest `va` in the window with `va ≡ phase (mod align)` such that
    /// `[va, va+len)` is free. First-fit: deterministic, O(regions).
    ///
    /// `align` must be a power of two; `phase < align`. This is the whole
    /// superpage story from the allocator's side: for a ≥2 MiB mapping the
    /// caller passes `align = 2 MiB, phase = pa % 2 MiB`, so the interior
    /// spans of the chosen va are 2 MiB-congruent with the physical pages
    /// and promote to superpage leaves.
    #[must_use]
    pub fn find_hole(&self, len: usize, align: usize, phase: usize) -> Option<usize> {
        if len == 0 || !align.is_power_of_two() || phase >= align || len % PAGE != 0 {
            return None;
        }
        // Candidate holes: before each in-window region, and after the last.
        // Regions are va-sorted, so walk gaps in order and take the first fit.
        let mut cursor = self.window_base;
        for r in self.regions[..self.len].iter().filter(|r| r.va + r.len > self.window_base) {
            if let Some(va) = fit(cursor, r.va.min(self.window_end()), len, align, phase) {
                return Some(va);
            }
            cursor = cursor.max(r.va + r.len);
        }
        fit(cursor, self.window_end(), len, align, phase)
    }

    /// Records a kernel-chosen mapping (`Vmo`/`Mmio`). The caller obtained
    /// `va` from [`find_hole`]; this re-checks every invariant anyway —
    /// refuse, don't trust (the two calls may be separated by other logic).
    pub fn insert(&mut self, region: VaRegion) -> Result<(), VaError> {
        if region.len == 0
            || region.va % PAGE != 0
            || region.len % PAGE != 0
            || region.va.checked_add(region.len).is_none()
        {
            return Err(VaError::BadInput);
        }
        if region.kind != RegionKind::Fixed {
            let inside =
                region.va >= self.window_base && region.va + region.len <= self.window_end();
            if !inside {
                return Err(VaError::BadInput);
            }
        }
        if self.first_overlap(region.va, region.len).is_some() {
            return Err(VaError::Occupied);
        }
        if self.len == MAX_REGIONS {
            return Err(VaError::TableFull);
        }
        let at = self.regions[..self.len].partition_point(|r| r.va < region.va);
        self.regions.copy_within(at..self.len, at + 1);
        self.regions[at] = region;
        self.len += 1;
        self.peak = self.peak.max(self.len);
        Ok(())
    }

    /// Records a kernel-placed fixed mapping (exec segments, stack, legacy
    /// fixed-VA maps — all OUTSIDE the window). Overflow COUNTS instead of
    /// failing: refusing an exec because a bookkeeping table is full would
    /// invert priorities, and fixed records never feed the hole-finder, so
    /// an untracked one cannot corrupt allocation. Coalesces with a
    /// va-adjacent, pa-contiguous record of equal flags+kind so legacy
    /// per-page map loops occupy one slot, not thousands.
    pub fn record_fixed(&mut self, va: usize, len: usize, pa: usize, flags: usize) {
        if len == 0 || va % PAGE != 0 || len % PAGE != 0 || va.checked_add(len).is_none() {
            self.untracked = self.untracked.saturating_add(1);
            return;
        }
        // Coalesce: the immediate left neighbour ends exactly at `va` with
        // contiguous pa and identical flags.
        let at = self.regions[..self.len].partition_point(|r| r.va < va);
        if at > 0 {
            let left = &mut self.regions[at - 1];
            if left.kind == RegionKind::Fixed
                && left.flags == flags
                && left.va + left.len == va
                && left.pa + left.len == pa
            {
                left.len += len;
                return;
            }
        }
        let region = VaRegion { va, len, pa, flags, kind: RegionKind::Fixed };
        if self.insert(region).is_err() {
            self.untracked = self.untracked.saturating_add(1);
        }
    }

    /// Read-only twin of [`Self::remove_exact`]: validates that exactly
    /// `(va, len)` names an unmappable region and returns it, leaving the
    /// table untouched. The unmap shell peeks, clears PTEs and shoots down
    /// the TLB, and only THEN forgets the region — a region must never
    /// disappear from the record while its PTEs are still live.
    pub fn peek_exact(&self, va: usize, len: usize) -> Result<VaRegion, VaError> {
        if len == 0 || va % PAGE != 0 || len % PAGE != 0 {
            return Err(VaError::BadInput);
        }
        let at = self.regions[..self.len].partition_point(|r| r.va < va);
        let Some(region) = self.regions[..self.len].get(at).copied().filter(|r| r.va == va) else {
            return Err(VaError::NotFound);
        };
        if region.kind == RegionKind::Fixed {
            return Err(VaError::FixedRegion);
        }
        if region.len != len {
            return Err(VaError::LenMismatch);
        }
        Ok(region)
    }

    /// Unmap bookkeeping: removes the region at exactly `(va, len)`.
    /// Returns the removed region so the shell can clear its PTEs.
    pub fn remove_exact(&mut self, va: usize, len: usize) -> Result<VaRegion, VaError> {
        let region = self.peek_exact(va, len)?;
        let at = self.regions[..self.len].partition_point(|r| r.va < va);
        self.regions.copy_within(at + 1..self.len, at);
        self.len -= 1;
        Ok(region)
    }

    /// Does any live region map part of the physical range `[pa, pa+len)`?
    /// The `vmo_destroy` guard: destroying a VMO this address space still
    /// maps would leave live PTEs onto recycled arena pages.
    #[must_use]
    pub fn any_backed_by(&self, pa: usize, len: usize) -> bool {
        let Some(end) = pa.checked_add(len) else {
            return false;
        };
        self.regions[..self.len]
            .iter()
            .any(|r| r.kind != RegionKind::Fixed && pa < r.pa + r.len && r.pa < end)
    }

    /// Is `[va, va+len)` free of tracked regions AND outside the managed
    /// window? The legacy fixed-VA syscall's admission check: it may map
    /// anywhere EXCEPT into the kernel's window (that is the invariant that
    /// keeps the hole-finder sound without consulting the page table).
    #[must_use]
    pub fn admits_fixed(&self, va: usize, len: usize) -> Result<(), VaError> {
        let Some(end) = va.checked_add(len) else {
            return Err(VaError::BadInput);
        };
        if end > self.window_base && va < self.window_end() {
            return Err(VaError::Occupied);
        }
        if self.first_overlap(va, len).is_some() {
            return Err(VaError::Occupied);
        }
        Ok(())
    }
}

/// First `va >= start` with `va ≡ phase (mod align)` and `va + len <= end`.
fn fit(start: usize, end: usize, len: usize, align: usize, phase: usize) -> Option<usize> {
    if start >= end {
        return None;
    }
    let rem = (start.wrapping_sub(phase)) & (align - 1);
    let va = if rem == 0 { start } else { start.checked_add(align - rem)? };
    let stop = va.checked_add(len)?;
    (stop <= end).then_some(va)
}

/// The 2 MiB superpage size.
pub const SUPERPAGE: usize = 2 * 1024 * 1024;

/// One chunk of a mapping plan.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MapChunk {
    pub va: usize,
    pub pa: usize,
    pub len: usize,
    pub superpage: bool,
}

/// Tiles `[va, va+len)` into head 4K pages, interior 2 MiB superpages, and
/// tail 4K pages — maximal superpage coverage when `va ≡ pa (mod 2 MiB)`
/// (which the phase-aware [`VaSpace::find_hole`] guarantees for kernel-chosen
/// VAs). When the congruence does not hold, the whole range tiles as 4K.
///
/// Pure and bounded: at most 3 logical chunks; the executor iterates pages
/// within them.
#[must_use]
pub fn superpage_plan(va: usize, pa: usize, len: usize) -> [Option<MapChunk>; 3] {
    let mut out = [None, None, None];
    if len == 0 {
        return out;
    }
    let congruent = (va & (SUPERPAGE - 1)) == (pa & (SUPERPAGE - 1));
    let head_end = (va + SUPERPAGE - 1) & !(SUPERPAGE - 1);
    let body_end = (va + len) & !(SUPERPAGE - 1);
    if !congruent || body_end <= head_end || body_end - head_end < SUPERPAGE {
        out[0] = Some(MapChunk { va, pa, len, superpage: false });
        return out;
    }
    let mut idx = 0;
    if head_end > va {
        let head = head_end - va;
        out[idx] = Some(MapChunk { va, pa, len: head, superpage: false });
        idx += 1;
    }
    out[idx] = Some(MapChunk {
        va: head_end,
        pa: pa + (head_end - va),
        len: body_end - head_end,
        superpage: true,
    });
    idx += 1;
    if va + len > body_end {
        out[idx] = Some(MapChunk {
            va: body_end,
            pa: pa + (body_end - va),
            len: va + len - body_end,
            superpage: false,
        });
    }
    out
}
