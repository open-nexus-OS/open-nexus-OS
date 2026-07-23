// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the kernel-managed user VMO arena (`VMO_POOL`/`VmoPool`) split out
//! of `vmo.rs` (module-size ratchet). A bump allocator over the fixed
//! `USER_VMO_ARENA` window with a bounded free-list (task #124) and a P2
//! idle-hart zero-frontier so `vmo_create` never memsets on the hot path.
//! `vmo.rs` re-exports the four public symbols so `exec`/`task_image`/`tests`
//! keep reaching them via `super::vmo::…`.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: neuron host tests + QEMU marker gates (just test-os / ci-os-smp)
//! ADR: docs/adr/0016-kernel-libs-architecture.md

use super::*;

pub(super) static VMO_POOL: Mutex<VmoPool> = Mutex::new(VmoPool::new());

/// Bounded free-range table (task #124): freed one-shot VMOs (boot-splash
/// backing, staging buffers) are reused instead of growing the bump frontier.
pub(super) const VMO_FREE_SLOTS: usize = 16;

pub(super) struct VmoPool {
    base: usize,
    next: usize,
    pub(super) limit: usize,
    peak_next: usize,
    /// Freed ranges available for reuse: `(base, len)`, `len == 0` marks an
    /// empty entry. Bounded — when full, a freed range is leaked (graceful
    /// bump-only degradation), never corrupted.
    free_list: [(usize, usize); VMO_FREE_SLOTS],
    /// P2: freed-but-not-yet-zeroed ranges. `free()` parks ranges here; the
    /// idle harts zero them (`idle_zero_step`) and promote them to
    /// `free_list`, so `allocate`'s reuse path NEVER memsets (the recycled
    /// 4MB boot-splash VMO was the measured ~100ms BKL hold).
    dirty_list: [(usize, usize); VMO_FREE_SLOTS],
    /// Bytes dropped because the free list was full (observability).
    leaked: usize,
    /// Zero-frontier (P2): everything in [next, zeroed_until) is already
    /// zeroed by the idle harts (`idle_zero_step`), so the common
    /// `vmo_create` path skips its memset entirely — the measured 82-90ms
    /// BKL holds were exactly this zeroing running inside the syscall.
    zeroed_until: usize,
}

/// Snapshot of the kernel-managed user VMO arena.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct VmoPoolStats {
    pub(super) base: usize,
    pub(super) used: usize,
    pub(super) remaining: usize,
    pub(super) peak_used: usize,
}

impl VmoPool {
    pub(super) const fn new() -> Self {
        Self {
            base: 0,
            next: 0,
            limit: 0,
            peak_next: 0,
            free_list: [(0, 0); VMO_FREE_SLOTS],
            dirty_list: [(0, 0); VMO_FREE_SLOTS],
            leaked: 0,
            zeroed_until: 0,
        }
    }

    #[cfg(test)]
    pub(super) fn with_window(base: usize, len: usize) -> Self {
        Self {
            base,
            next: base,
            limit: base.saturating_add(len),
            peak_next: base,
            free_list: [(0, 0); VMO_FREE_SLOTS],
            dirty_list: [(0, 0); VMO_FREE_SLOTS],
            leaked: 0,
        }
    }

    pub(super) fn ensure_initialized(&mut self) {
        if self.base != 0 {
            return;
        }
        let start = align_up_addr(USER_VMO_ARENA_BASE);
        let limit = start.saturating_add(USER_VMO_ARENA_LEN);
        self.base = start;
        self.next = start;
        self.zeroed_until = start;
        self.limit = limit;
        self.peak_next = start;
    }

    /// P2 phased reserve: clean free-list first (no zero needed), else bump
    /// WITHOUT zeroing — the trap handler zeroes with the BKL dropped. The
    /// range is unreachable until `vmo_create_finish` installs the cap.
    /// Returns (base, len, needs_zero).
    pub(super) fn allocate_nozero(&mut self, len: usize) -> Result<(usize, usize, bool), Error> {
        self.ensure_initialized();
        if len == 0 {
            return Err(Error::Capability(CapError::PermissionDenied));
        }
        let aligned = align_len(len).ok_or(Error::Capability(CapError::PermissionDenied))?;
        let mut best: Option<usize> = None;
        for (index, &(_, entry_len)) in self.free_list.iter().enumerate() {
            if entry_len >= aligned && best.is_none_or(|prev| entry_len < self.free_list[prev].1) {
                best = Some(index);
            }
        }
        if let Some(index) = best {
            let (entry_base, entry_len) = self.free_list[index];
            self.free_list[index] = if entry_len > aligned {
                (entry_base + aligned, entry_len - aligned)
            } else {
                (0, 0)
            };
            return Ok((entry_base, aligned, false));
        }
        let next =
            self.next.checked_add(aligned).ok_or(Error::Capability(CapError::PermissionDenied))?;
        if next > self.limit {
            let pressure = self.stats();
            log_error!(
                "VMO-POOL exhausted: want=0x{:x} used=0x{:x} remaining=0x{:x} peak=0x{:x}",
                aligned,
                pressure.used,
                pressure.remaining,
                pressure.peak_used
            );
            return Err(Error::Capability(CapError::PermissionDenied));
        }
        let base = self.next;
        self.next = next;
        if self.next > self.peak_next {
            self.peak_next = self.next;
        }
        // Pre-zeroed by the idle frontier? Then no zero pass is needed.
        let needs_zero = self.zeroed_until < base + aligned;
        if !needs_zero && self.zeroed_until < base + aligned {
            self.zeroed_until = base + aligned;
        }
        Ok((base, aligned, needs_zero))
    }

    /// P2 idle-hart pre-zeroing: advance the zero-frontier by up to
    /// `budget` bytes. Called from cpu_main's idle path (pool leaf lock only,
    /// never the BKL) — idle cpus pay the memset so vmo_create never does.
    /// Returns bytes zeroed (0 = frontier fully ahead, caller can WFI).
    pub(super) fn idle_zero_step(&mut self, budget: usize) -> usize {
        self.ensure_initialized();
        // Dirty recycled ranges first (they unblock allocate's clean reuse).
        for i in 0..self.dirty_list.len() {
            let (base, len) = self.dirty_list[i];
            if len == 0 {
                continue;
            }
            let n = len.min(budget);
            unsafe {
                ptr::write_bytes(base as *mut u8, 0, n);
            }
            if n == len {
                self.dirty_list[i] = (0, 0);
                // Promote to the clean free list (coalesce with neighbours).
                let mut base = base;
                let mut len = len;
                for entry in self.free_list.iter_mut() {
                    if entry.1 == 0 {
                        continue;
                    }
                    if entry.0 + entry.1 == base {
                        base = entry.0;
                        len += entry.1;
                        *entry = (0, 0);
                    } else if base + len == entry.0 {
                        len += entry.1;
                        *entry = (0, 0);
                    }
                }
                let mut placed = false;
                for entry in self.free_list.iter_mut() {
                    if entry.1 == 0 {
                        *entry = (base, len);
                        placed = true;
                        break;
                    }
                }
                if !placed {
                    self.leaked = self.leaked.saturating_add(len);
                }
            } else {
                // Partially zeroed: keep the (still dirty) tail parked.
                self.dirty_list[i] = (base + n, len - n);
            }
            return n;
        }
        // Then advance the bump-side frontier, bounded ahead of `next` (no
        // point zeroing the whole 96MB arena eagerly).
        const FRONTIER_AHEAD: usize = 8 * 1024 * 1024;
        if self.zeroed_until < self.next {
            self.zeroed_until = self.next;
        }
        let target = self.limit.min(self.next.saturating_add(FRONTIER_AHEAD));
        let end = target.min(self.zeroed_until.saturating_add(budget));
        if self.zeroed_until >= end {
            return 0;
        }
        let n = end - self.zeroed_until;
        unsafe {
            ptr::write_bytes(self.zeroed_until as *mut u8, 0, n);
        }
        self.zeroed_until = end;
        n
    }

    pub(super) fn allocate(&mut self, len: usize) -> Result<(usize, usize), Error> {
        self.ensure_initialized();
        if len == 0 {
            return Err(Error::Capability(CapError::PermissionDenied));
        }
        let aligned = align_len(len).ok_or(Error::Capability(CapError::PermissionDenied))?;
        // Reuse a freed range first (best fit) so one-shot allocations stop
        // growing the bump frontier. Same zeroing guarantee as the bump path.
        let mut best: Option<usize> = None;
        for (index, &(_, entry_len)) in self.free_list.iter().enumerate() {
            if entry_len >= aligned && best.is_none_or(|prev| entry_len < self.free_list[prev].1) {
                best = Some(index);
            }
        }
        if let Some(index) = best {
            let (entry_base, entry_len) = self.free_list[index];
            self.free_list[index] = if entry_len > aligned {
                (entry_base + aligned, entry_len - aligned)
            } else {
                (0, 0)
            };
            // Clean by contract: entries reach free_list only through the
            // idle zeroer (or pre-zeroed bump frontier) — no memset here.
            return Ok((entry_base, aligned));
        }
        let next =
            self.next.checked_add(aligned).ok_or(Error::Capability(CapError::PermissionDenied))?;
        if next > self.limit {
            // Exhaustion was SILENT once and cost a day of bisection
            // (TASK-0076B): a service image allocation failing here kills the
            // spawn with no output anywhere. Say what ran out, with values.
            let pressure = self.stats();
            log_error!(
                "VMO-POOL exhausted: want=0x{:x} used=0x{:x} remaining=0x{:x} peak=0x{:x}",
                aligned,
                pressure.used,
                pressure.remaining,
                pressure.peak_used
            );
            return Err(Error::Capability(CapError::PermissionDenied));
        }
        let base = self.next;
        self.next = next;
        if self.next > self.peak_next {
            self.peak_next = self.next;
        }
        // RFC-0004 (loader + shared-page guard): pages must never leak stale
        // contents. P2 zero-frontier: idle harts pre-zero [next, zeroed_until)
        // via `idle_zero_step`, so we memset only the part the frontier has
        // not covered — the common case is a microsecond no-op instead of the
        // measured 82-90ms BKL hold.
        let end = base + aligned;
        let dirty_from = self.zeroed_until.clamp(base, end);
        if dirty_from < end {
            unsafe {
                ptr::write_bytes(dirty_from as *mut u8, 0, end - dirty_from);
            }
        }
        if self.zeroed_until < end {
            self.zeroed_until = end;
        }
        Ok((base, aligned))
    }

    /// Returns a range handed out by [`allocate`] to the pool (task #124).
    /// Rejects ranges outside the allocated span, unaligned bases, and any
    /// overlap with already-free memory (double-free defense). Coalesces with
    /// the bump frontier and with adjacent free entries; when the bounded
    /// free list is full the range is leaked (counted), never corrupted.
    pub(super) fn free(&mut self, base: usize, len: usize) -> Result<(), Error> {
        self.ensure_initialized();
        let aligned = align_len(len).ok_or(Error::Capability(CapError::PermissionDenied))?;
        if aligned == 0 || base & (PAGE_SIZE - 1) != 0 {
            return Err(Error::Capability(CapError::PermissionDenied));
        }
        let end = base.checked_add(aligned).ok_or(Error::Capability(CapError::PermissionDenied))?;
        // Must lie inside the currently allocated span [pool.base, pool.next).
        if base < self.base || end > self.next {
            return Err(Error::Capability(CapError::PermissionDenied));
        }
        for &(entry_base, entry_len) in self.free_list.iter().chain(self.dirty_list.iter()) {
            if entry_len != 0 && entry_base < end && base < entry_base + entry_len {
                return Err(Error::Capability(CapError::PermissionDenied));
            }
        }
        // Frontier un-bump: a range touching self.next collapses back into
        // the pool. Its contents are DIRTY, so pull the zero-frontier back —
        // the idle zeroer (or the allocate fallback) re-covers it.
        if base + aligned == self.next {
            self.next = base;
            if self.zeroed_until > base {
                self.zeroed_until = base;
            }
            return Ok(());
        }
        // P2: park in the dirty list; the idle harts zero + promote to
        // free_list (no coalescing across clean/dirty — bounded simplicity;
        // clean-side coalescing happens at promotion).
        for entry in self.dirty_list.iter_mut() {
            if entry.1 == 0 {
                *entry = (base, aligned);
                return Ok(());
            }
        }
        // Dirty list full: zero synchronously (bounded fallback) and place
        // clean, preserving the old behaviour.
        unsafe {
            ptr::write_bytes(base as *mut u8, 0, aligned);
        }
        for entry in self.free_list.iter_mut() {
            if entry.1 == 0 {
                *entry = (base, aligned);
                return Ok(());
            }
        }
        self.leaked = self.leaked.saturating_add(aligned);
        Ok(())
    }

    #[must_use]
    pub(super) fn stats(&self) -> VmoPoolStats {
        let used = self.next.saturating_sub(self.base);
        let remaining = self.limit.saturating_sub(self.next);
        let peak_used = self.peak_next.saturating_sub(self.base);
        VmoPoolStats { base: self.base, used, remaining, peak_used }
    }

    #[allow(dead_code)]
    pub(super) fn contains(&self, addr: usize, len: usize) -> bool {
        if self.base == 0 || len == 0 {
            return false;
        }
        let end = match addr.checked_add(len) {
            Some(end) => end,
            None => return false,
        };
        addr >= self.base && end <= self.limit
    }
}
