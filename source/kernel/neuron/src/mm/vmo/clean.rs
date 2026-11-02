// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: CPU writes made visible to a device that does not snoop the CPU caches (ADR-0092 —
//! ADR-0079's "kernel maintenance call over an object the caller may write", built with its
//! first user). An object made for such a device carries the cache block its device needs
//! (`set_block`, before any capability exists); a frame the kernel zeroes for it is cleaned
//! before anyone can read it (`frame`, `range_polled`); `vmo_clean` (syscall 74) cleans what a
//! writer drew (`plan` under the kernel lock, `run` with it dropped). `cbo.clean` writes dirty
//! lines back and changes no memory — a frame recycled while the lock is dropped only gets a
//! redundant write-back, so no pin is needed. Frames the pool does not own (a device window)
//! are never cleaned.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `KSELFTEST: vmo clean ok (`; the range rule host-tested in `crate::clean_span`
//! ADR: docs/adr/0092-a-surface-is-a-buffer-the-app-draws-and-the-gpu-reads-gpud-allocates-it-for-its-owner-windowd-composes-by-reference.md

use core::sync::atomic::{AtomicUsize, Ordering};

use super::{with, VmoError, VmoId, VmoKind, TABLE};
use crate::lazy::{PageState, PAGE};

/// Pages whose frames one table-lock hold collects before the lock is let go to clean them.
const CHUNK_PAGES: usize = 64;

/// Cache lines cleaned for objects since boot (the selftest's witness).
static LINES: AtomicUsize = AtomicUsize::new(0);

/// Lines cleaned since boot.
pub fn lines_cleaned() -> usize {
    LINES.load(Ordering::Relaxed)
}

/// Mark `id` as made for a non-coherent device whose CPU writes need `block`-byte cleans (0:
/// coherent). Called before the object's capability exists; false when it is gone.
pub fn set_block(id: VmoId, block: usize) -> bool {
    let block = u16::try_from(block).unwrap_or(0);
    TABLE.lock().get_mut(id).map(|o| o.clean_block = block).is_some()
}

/// The cache block CPU writes to `id` must be cleaned in (0: nothing to clean).
pub fn block(id: VmoId) -> usize {
    with(id, |o| usize::from(o.clean_block)).unwrap_or(0)
}

/// A clean `vmo_clean` runs with the kernel lock dropped: `[offset, end)` of `id`.
#[derive(Clone, Copy, Debug)]
pub struct Job {
    id: VmoId,
    offset: usize,
    end: usize,
    block: usize,
}

/// Phase A (kernel lock held): the job for `[offset, offset + len)` of `id` — `None` when its
/// device snoops the caches (nothing to clean). The caller checked the range against its
/// capability; a frame the pool does not own is never cleaned (`BadLength` for a fixed object).
pub fn plan(id: VmoId, offset: usize, len: usize) -> Result<Option<Job>, VmoError> {
    let end = offset.checked_add(len).ok_or(VmoError::BadLength)?;
    with(id, |o| {
        if o.kind == VmoKind::Fixed || end > o.len {
            return Err(VmoError::BadLength);
        }
        let block = usize::from(o.clean_block);
        Ok((block != 0 && len != 0).then_some(Job { id, offset, end, block }))
    })
    .ok_or(VmoError::NoSuchObject)?
}

/// Phase B (kernel lock dropped): clean every committed frame of the job's range — a lazy page
/// never committed holds nothing a device could read stale. `CHUNK_PAGES` pages are collected per
/// table-lock hold, and this hart serves its shootdown mailbox between chunks.
pub fn run(job: &Job) {
    let mut at = job.offset;
    let mut pieces = [(0usize, 0usize); CHUNK_PAGES];
    while at < job.end {
        let (n, next) = collect(job, at, &mut pieces);
        for &(pa, len) in &pieces[..n] {
            frame(pa, len, job.block);
        }
        at = next;
        poll();
    }
}

/// The resident pieces of up to `CHUNK_PAGES` pages from `at`, and where the next chunk starts.
fn collect(job: &Job, at: usize, pieces: &mut [(usize, usize); CHUNK_PAGES]) -> (usize, usize) {
    let stop = job.end.min((at / PAGE + CHUNK_PAGES) * PAGE);
    let n = with(job.id, |o| {
        let mut n = 0;
        let mut cur = at;
        while cur < stop {
            let piece = (stop - cur).min(PAGE - cur % PAGE);
            let pa = match &o.lazy {
                Some(pages) => match pages.state(cur / PAGE) {
                    Some(PageState::Frame(pa)) => Some(pa as usize + cur % PAGE),
                    _ => None,
                },
                None => o.translate(cur),
            };
            if let Some(pa) = pa {
                pieces[n] = (pa, piece);
                n += 1;
            }
            cur += piece;
        }
        n
    })
    // The object went away while the lock was dropped: nothing more of it to clean.
    .unwrap_or(0);
    let next = if n == 0 && with(job.id, |_| ()).is_none() { job.end } else { stop };
    (n, next)
}

/// Clean `[pa, pa + len)` — one contiguous physical piece — through the direct map, `block`
/// bytes a line (`block == 0`: a coherent device, nothing to do).
pub(super) fn frame(pa: usize, len: usize, block: usize) {
    if block == 0 || len == 0 {
        return;
    }
    let va = crate::phys::phys_to_virt(pa);
    let first = va & !(block - 1);
    let mut line = first;
    while line < va + len {
        clean_line(line);
        line += block;
    }
    LINES.fetch_add((line - first) / block, Ordering::Relaxed);
}

/// `frame` over a long run in 64 KiB pieces with the shootdown mailbox served between them (the
/// create's phase B, kernel lock dropped).
pub(super) fn range_polled(pa: usize, len: usize, block: usize) {
    if block == 0 {
        return;
    }
    const PIECE: usize = 64 * 1024;
    let mut off = 0;
    while off < len {
        let n = PIECE.min(len - off);
        frame(pa + off, n, block);
        off += n;
        poll();
    }
}

fn poll() {
    let _ = crate::smp::tlb::poll_mailbox(crate::smp::cpu_current_id());
}

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
fn clean_line(addr: usize) {
    // SAFETY: `cbo.clean` of a line of the direct map over RAM the pool owns: it writes a dirty
    // line back and changes no memory; the firmware enabled Zicbom for S-mode
    // (`menvcfg.CBCFE`), as the console ring's flush relies on.
    unsafe {
        core::arch::asm!(
            ".option push",
            ".option arch, +zicbom",
            "cbo.clean ({0})",
            ".option pop",
            in(reg) addr,
            options(nostack)
        );
    }
}

#[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
fn clean_line(_addr: usize) {}
