// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the swap stage's kernel half (RFC-0100 §5, TASK-0333 6f-2, ADR-0088) — the boot's key
//! and the seal's counter, the export batch over a process's lazy regions, the page-in, and the
//! notices the pager hears on the pressure sink. The key is seeded ONCE per boot by the RNG
//! service (syscall 70); without it every export is refused: no entropy, no swap. An export seals
//! each record under a fresh counter with the page's identity as associated data
//! (`crate::lazy::seal`), so the pager and its disk hold ciphertext alone and a record opens only
//! as the page — and the export — it was sealed for. A thread that touches a swapped page parks
//! (`BlockReason::PageIn`); the first asks the pager (`OP_PAGE_IN`, one request per page), and the
//! pager's answer (syscall 73) wakes every thread parked on it. An object that dies or is purged
//! with swapped pages tells the pager its slots (`OP_SWAP_DROP`). Notices go out where the
//! pressure level does (`pressure_endpoint::dispatch`) and stay queued until the sink takes them:
//! a full sink loses no request.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional (TASK-0333 6f-2)
//! API_STABILITY: Internal (syscalls 70–73; the notices are `nexus-abi`'s `OP_PAGE_IN` and
//!   `OP_SWAP_DROP`, the export buffer `nexus-abi`'s `SwapExport` v1)
//! TEST_COVERAGE: the seal, the swap map, the buffer and the notices on the host (`crate::lazy`,
//!   `crate::pressure`); `KSELFTEST: swap ok (…)` on every lane and the board
//! INVARIANTS: one key per boot, never readable and never logged; a counter seals one export
//!   only; a parked thread's request stays queued until its page is back, gone or the pager is;
//!   the locks here are leaves — the seal is taken before the object table, the notices with
//!   nothing else held.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use super::address_space::AddressSpace;
use super::page_table::PAGE_SIZE;
use super::squeeze::SCAN_PAGES;
use crate::lazy::page_index;
use crate::lazy::seal::{Sealer, KEY_LEN, MAX_SEALED, TAG_LEN};
use crate::lazy::swap::{self as wire, SwapEntry};
use crate::pressure;
use crate::sched::Scheduler;
use crate::task::{self, BlockReason, Pid};
use crate::va_space::VaRegion;

/// An entry's bytes at most (the buffer an entry is built and a record opened in).
const ENTRY_MAX: usize = wire::entry_len(MAX_SEALED);

/// The boot's sealer, the seal's counter, and the one buffer an entry is built or a record is
/// opened in. One export or page-in at a time (the kernel lock makes that a fact; this lock keeps
/// the key's use in one place).
struct Seal {
    sealer: Option<Sealer>,
    /// The next export's nonce: never reused within the boot (the counter only grows, across the
    /// boot proof's key and the real one).
    ctr: u64,
    buf: [u8; ENTRY_MAX],
}

static SEAL: spin::Mutex<Seal> =
    spin::Mutex::new(Seal { sealer: None, ctr: 1, buf: [0; ENTRY_MAX] });

/// Why a key was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyError {
    /// The boot has its key already.
    Held,
}

/// Install the boot's key (the RNG service's seed, syscall 70). Once per boot.
pub fn install_key(key: &[u8; KEY_LEN]) -> Result<(), KeyError> {
    let mut seal = SEAL.lock();
    if seal.sealer.is_some() {
        return Err(KeyError::Held);
    }
    seal.sealer = Some(Sealer::new(key));
    Ok(())
}

/// Whether the boot has its key (swap is possible).
pub fn has_key() -> bool {
    SEAL.lock().sealer.is_some()
}

/// The boot proof removes its own key when it is done: a key compiled into the image must never
/// seal a real record, and the RNG service's seed must find the place free.
pub fn selftest_remove_key() {
    SEAL.lock().sealer = None;
}

/// Why an export was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportError {
    /// No key: swap is off.
    NoKey,
    /// The buffer cannot hold the header and the largest entry.
    Short,
    /// The pager's buffer refused a write.
    Fault,
}

/// What one export batch did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Exported {
    /// Entries written.
    pub records: u32,
    /// Bytes of the buffer used, the header included.
    pub bytes: usize,
    /// The address the next batch starts at (`0`: the walk reached the end).
    pub next: usize,
}

/// One export batch from `from_va` over the lazy regions of `space` whose object `allowed`
/// names: every record that is not swapped is sealed under a fresh counter and handed to `emit`
/// as one entry at its offset in the pager's buffer of `buf_len` bytes (the header is the
/// caller's, written last). At most `EXPORT_BATCH` records and `SCAN_PAGES` page words a batch.
pub fn export(
    space: &AddressSpace,
    from_va: usize,
    allowed: &[u32],
    buf_len: usize,
    emit: &mut dyn FnMut(usize, &[u8]) -> bool,
) -> Result<Exported, ExportError> {
    if buf_len < wire::EXPORT_MIN_BUF {
        return Err(ExportError::Short);
    }
    let mut seal = SEAL.lock();
    let Seal { sealer, ctr, buf } = &mut *seal;
    let sealer = sealer.as_ref().ok_or(ExportError::NoKey)?;
    let regions: Vec<VaRegion> = space
        .va_space()
        .regions()
        .iter()
        .filter(|r| r.lazy && allowed.contains(&r.vmo) && r.va + r.len > from_va)
        .copied()
        .collect();
    let mut done = Exported { records: 0, bytes: wire::EXPORT_HEADER, next: 0 };
    let mut scanned = 0usize;
    'walk: for region in &regions {
        let mut va = from_va.max(region.va);
        while va < region.va + region.len {
            let full = done.records as usize == wire::EXPORT_BATCH
                || buf_len - done.bytes < ENTRY_MAX
                || scanned == SCAN_PAGES;
            if full {
                done.next = va;
                break 'walk;
            }
            scanned += 1;
            let index = page_index(region.va, region.offset, va);
            va += PAGE_SIZE;
            let Ok(page) = u32::try_from(index) else { continue };
            // The record's block into the entry's body, the page exported under this counter;
            // sealed here, with the object table's lock dropped.
            let body = &mut buf[wire::ENTRY_HEAD..];
            let nonce = *ctr;
            let Some(len) = super::vmo::swap::export(region.vmo, index, nonce, body) else {
                continue;
            };
            // The counter is the page's from here on — consumed whatever follows, so no two
            // seals ever share a nonce. A failed seal or write leaves the page exported: the next
            // export supersedes it.
            *ctr += 1;
            let Ok(sealed) = sealer.seal(nonce, region.vmo, page, body, len) else { continue };
            let token = wire::token(region.vmo, page);
            let Some(bytes) = wire::finish_entry(&mut buf[..], token, nonce, sealed) else {
                continue;
            };
            if !emit(done.bytes, &buf[..bytes]) {
                return Err(ExportError::Fault);
            }
            done.records += 1;
            done.bytes += bytes;
        }
    }
    Ok(done)
}

/// Why a page-in did not bring the page back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageInError {
    /// The page is not swapped in that slot (any more): the answer is stale.
    Stale,
    /// The record did not open (a changed byte, another page's or export's record, a bad length).
    Forged,
    /// It opened but did not decode to one page.
    Damaged,
    /// No frame for the page now.
    Exhausted,
    /// No key (nothing can be swapped).
    NoKey,
    /// The pager's buffer could not be read.
    Fault,
}

/// The pager's answer for `token`: the `len` bytes `fill` reads are the sealed record it wrote
/// to `slot`. Opened under the page's own counter and identity, decoded into a fresh frame,
/// entered while the page is still swapped as it was looked at.
pub fn page_in(
    token: u64,
    slot: u32,
    len: usize,
    fill: impl FnOnce(&mut [u8]) -> bool,
) -> Result<(), PageInError> {
    let (id, page) = wire::token_parts(token);
    let index = page as usize;
    let entry: SwapEntry = super::vmo::swap::entry(id, index)
        .filter(|e| e.slot == Some(slot))
        .ok_or(PageInError::Stale)?;
    if !(TAG_LEN..=MAX_SEALED).contains(&len) {
        return Err(PageInError::Forged);
    }
    let mut seal = SEAL.lock();
    let Seal { sealer, buf, .. } = &mut *seal;
    let sealer = sealer.as_ref().ok_or(PageInError::NoKey)?;
    if !fill(&mut buf[..len]) {
        return Err(PageInError::Fault);
    }
    let block =
        sealer.open(entry.ctr, id, page, &mut buf[..len]).map_err(|_| PageInError::Forged)?;
    super::vmo::swap::page_in(id, index, entry, &buf[..block]).map_err(|e| match e {
        super::vmo::swap::PageInError::Stale => PageInError::Stale,
        super::vmo::swap::PageInError::Damaged => PageInError::Damaged,
        super::vmo::swap::PageInError::Exhausted => PageInError::Exhausted,
    })
}

/// One page-in request: the page, the pager's slot, whether the sink has it.
struct Request {
    token: u64,
    slot: u32,
    sent: bool,
}

/// What waits for the pager: the requests (one per page), the slots to free, and the pages whose
/// parked threads are to be woken (their page went away with its object).
struct Notices {
    requests: Vec<Request>,
    drops: Vec<u32>,
    wake: Vec<u64>,
}

static NOTICES: spin::Mutex<Notices> =
    spin::Mutex::new(Notices { requests: Vec::new(), drops: Vec::new(), wake: Vec::new() });

/// Something waits for a delivery.
static PENDING: AtomicBool = AtomicBool::new(false);
/// Threads parked on a swapped page since boot (`MmStats::parked_faults`).
static PARKED: AtomicU64 = AtomicU64::new(0);

/// Whether a thread that touched swapped page `token` (in the pager's `slot`) may park: the
/// pager is asked once per page. `false`: no pager holds the sink — the page cannot come back.
pub fn ask(token: u64, slot: u32) -> bool {
    if super::pressure_endpoint::binding().is_none() {
        return false;
    }
    let mut notices = NOTICES.lock();
    if !notices.requests.iter().any(|r| r.token == token) {
        notices.requests.push(Request { token, slot, sent: false });
        PENDING.store(true, Ordering::Release);
    }
    PARKED.fetch_add(1, Ordering::Relaxed);
    true
}

/// The request for `token` is answered (the page is back, stale, or failed).
pub fn answered(token: u64) {
    NOTICES.lock().requests.retain(|r| r.token != token);
}

/// Object `id` died or was purged with its swapped pages in `slots`: the pager frees them. A
/// thread parked on one of its pages is woken at the next delivery (it retries and finds its page
/// gone — it cannot have been parked on a page of an object no space maps).
pub fn dropped(id: u32, slots: &[u32]) {
    let mut notices = NOTICES.lock();
    let Notices { requests, drops, wake } = &mut *notices;
    let before = wake.len();
    requests.retain(|r| {
        let gone = wire::token_parts(r.token).0 == id;
        if gone {
            wake.push(r.token);
        }
        !gone
    });
    drops.extend_from_slice(slots);
    if !slots.is_empty() || wake.len() != before {
        PENDING.store(true, Ordering::Release);
    }
}

/// Whether a notice waits for the sink.
pub fn pending() -> bool {
    PENDING.load(Ordering::Acquire)
}

/// Deliver what waits — called by `pressure_endpoint::dispatch` (exclusive borrows, no pool lock
/// held). The unsent requests and the drops go to the sink frame by frame until it is full (the
/// rest stays queued); the threads whose page went away are woken. With no sink nothing can come
/// back: every request is dropped and its threads woken (they retry, find no pager and fail).
pub fn deliver(
    router: &mut crate::ipc::Router,
    tasks: &mut task::TaskTable,
    scheduler: &mut Scheduler,
) {
    if !PENDING.swap(false, Ordering::AcqRel) {
        return;
    }
    let endpoint = super::pressure_endpoint::binding();
    let woken: Vec<u64> = {
        let mut notices = NOTICES.lock();
        let Notices { requests, drops, wake } = &mut *notices;
        let mut woken = core::mem::take(wake);
        match endpoint {
            None => {
                woken.extend(requests.drain(..).map(|r| r.token));
                drops.clear();
            }
            Some(endpoint) => {
                for request in requests.iter_mut().filter(|r| !r.sent) {
                    let frame = pressure::encode_page_in(request.token, request.slot);
                    if !super::pressure_endpoint::send_frame(
                        router, tasks, scheduler, endpoint, &frame,
                    ) {
                        break;
                    }
                    request.sent = true;
                }
                while !drops.is_empty() {
                    let mut frame = [0u8; pressure::DROP_MAX_LEN];
                    let len = pressure::encode_drops(drops, &mut frame);
                    if !super::pressure_endpoint::send_frame(
                        router,
                        tasks,
                        scheduler,
                        endpoint,
                        &frame[..len],
                    ) {
                        break;
                    }
                    drops.drain(..(len - 2) / 4);
                }
            }
        }
        if requests.iter().any(|r| !r.sent) || !drops.is_empty() {
            PENDING.store(true, Ordering::Release);
        }
        woken
    };
    for token in woken {
        wake_parked(tasks, scheduler, token);
    }
}

/// The threads parked on swapped page `token`.
pub fn parked_on(tasks: &task::TaskTable, token: u64) -> Vec<Pid> {
    (0..tasks.len())
        .map(|i| Pid::from_raw(i as u32))
        .filter(|&pid| {
            tasks.task(pid).is_some_and(|t| t.block_reason() == Some(BlockReason::PageIn { token }))
        })
        .collect()
}

/// Wake every thread parked on `token` (its page is back, or went away): each re-executes the
/// instruction or the syscall that touched it. Answers how many.
pub fn wake_parked(tasks: &mut task::TaskTable, scheduler: &mut Scheduler, token: u64) -> usize {
    let parked = parked_on(tasks, token);
    for &pid in &parked {
        let _ = tasks.wake(pid, scheduler);
    }
    parked.len()
}

/// Threads parked on a swapped page since boot.
pub fn parked() -> u64 {
    PARKED.load(Ordering::Relaxed)
}
