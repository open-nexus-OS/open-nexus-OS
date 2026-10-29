// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the pager on the OS (RFC-0100 §5, ADR-0088, TASK-0333 6f-2) — memd moves a squeezed
//! frozen app's sealed records to the `swap` partition and back. The partition is opened once,
//! at the first swap-out, through blkd's block plane (blkd grants `swap` to memd alone); the
//! boot must have its key (`mm_stats` says so — the RNG service seeds it) or nothing is swapped.
//! A swap-out walks the app's records in batches (`swap_export`, syscall 71): each sealed record
//! is written to a free slot and only then committed (`swap_commit`, 72) — a refused commit
//! frees its slot. A page-in request from the kernel (`OP_PAGE_IN` on the sink) is answered at
//! once with the slot's record (`swap_page_in`, 73); a thaw pages the app's slots back before it
//! asks; a drop (`OP_SWAP_DROP`) frees the slots of a dead object. A page-in that found no frame
//! is retried at the loop's next pass (the kernel does not ask twice). Ciphertext only: the
//! records open in the kernel alone. Counts on the console, never content.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: the books on the host (tests/contract.rs); the chain on the usb-visible lane
//!   (`SELFTEST: swap out ok (…)`, `SELFTEST: swap in ok (…)`)

use alloc::vec::Vec;

use nexus_abi::{AbiError, SwapEntries};
use storage::blockproto::{
    CLIENT_REPLY_RECV_SLOT, CLIENT_REPLY_SEND_SLOT, CLIENT_REQ_SLOT, PART_SWAP,
};
use storage::remote_blk::RemoteBlockDevice;
use storage::BlockDevice;

use crate::order::SwappedOut;
use crate::swap::{decode_slot, encode_slot, Slots, SECTORS_PER_SLOT, SLOT_BYTES};

/// The export buffer handed to the kernel: eight of the largest entries, about sixteen of the
/// usual ones (a record is a page compressed, then sealed).
const EXPORT_BUF: usize = 32 * 1024;
/// Export batches one swap-out takes at most (a frozen app is hundreds of pages, a batch
/// sixteen records).
const MAX_BATCHES: usize = 1024;
/// Page-ins waiting for a frame, at most (a request beyond is answered at its next touch).
const MAX_RETRIES: usize = 64;

fn emit(line: &str) {
    let _ = nexus_abi::debug_println(line);
}

/// Asks the RNG service to seed the kernel's swap key (one waited exchange on the declared route:
/// its answer or its death). `true` when it says the kernel holds one now.
fn ask_rngd_for_key() -> bool {
    use core::sync::atomic::{AtomicU32, Ordering};
    static NONCE: AtomicU32 = AtomicU32::new(1);
    let route = nexus_service_topology::slots::memd::RNGD;
    let reply = nexus_service_topology::slots::memd::REPLY;
    let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
    let mut rsp = [0u8; 16];
    let req = crate::order::encode_seed_swap_key(nonce);
    let Ok(n) = nexus_ipc::exchange::call_into(route.send, reply, &req, &mut rsp) else {
        return false;
    };
    crate::order::seed_swap_key_status(&rsp[..n.min(rsp.len())], nonce)
        == Some(crate::order::RNGD_STATUS_OK)
}

/// What one answer to a page-in did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageIn {
    /// The page is back; its slot is free.
    Back,
    /// The kernel no longer has the page in that slot; the slot is free.
    Stale,
    /// The record did not open: the threads waiting for it were killed; the slot stays taken
    /// until its object dies (every later touch meets the same bytes).
    Dead,
    /// No frame, or the disk failed: kept for the next pass.
    Later,
    /// Not a slot of ours (a request that crossed a drop or a prefetch).
    Unknown,
}

enum State {
    /// Not opened yet (no swap-out so far, or no key yet).
    Unopened,
    /// The partition is open.
    Ready { dev: RemoteBlockDevice, slots: Slots },
    /// No partition: swap stays off for the boot.
    Off,
}

/// The pager.
pub struct Pager {
    state: State,
    /// The export buffer (taken at the first swap-out, kept).
    buf: Vec<u8>,
    /// One slot's bytes (taken at the first swap-out, kept).
    slot: Vec<u8>,
    /// Page-ins that found no frame: `(token, slot)`.
    retry: Vec<(u64, u32)>,
    said_no_key: bool,
}

impl Pager {
    pub const fn new() -> Self {
        Self {
            state: State::Unopened,
            buf: Vec::new(),
            slot: Vec::new(),
            retry: Vec::new(),
            said_no_key: false,
        }
    }

    /// Whether a swap-out can run now: the boot has its key and the partition is open (opened
    /// here, once). Without a key it says so once; the key may still come (the RNG service).
    pub fn ready(&mut self) -> bool {
        match self.state {
            State::Ready { .. } => return true,
            State::Off => return false,
            State::Unopened => {}
        }
        // The key: the kernel's, seeded by the RNG service when the pager first needs it (its
        // entropy device is granted after it starts — by now it is, or never: a board without one).
        let key = || nexus_abi::mm_stats().is_ok_and(|s| s.swap_key == 1);
        if !key() && !(ask_rngd_for_key() && key()) {
            if !self.said_no_key {
                self.said_no_key = true;
                emit("memd: swap off (no key)");
            }
            return false;
        }
        let opened = RemoteBlockDevice::open(
            CLIENT_REQ_SLOT,
            CLIENT_REPLY_SEND_SLOT,
            CLIENT_REPLY_RECV_SLOT,
            PART_SWAP,
        );
        let Some(dev) = opened else {
            emit("memd: swap off (no partition)");
            self.state = State::Off;
            return false;
        };
        let capacity = u32::try_from(dev.block_count() / SECTORS_PER_SLOT).unwrap_or(u32::MAX);
        self.buf.resize(EXPORT_BUF, 0);
        self.slot.resize(SLOT_BYTES, 0);
        emit(&alloc::format!("memd: swap on (slots={capacity})"));
        self.state = State::Ready { dev, slots: Slots::new(capacity) };
        true
    }

    /// Swaps `pid`'s records out: export, write each to a free slot, commit. `None` when the
    /// pager is not ready.
    pub fn swap_out(&mut self, pid: u32) -> Option<SwappedOut> {
        let Self { state, buf, slot, .. } = self;
        let State::Ready { dev, slots } = state else { return None };
        let block: &mut [u8; SLOT_BYTES] = slot.as_mut_slice().try_into().ok()?;
        let mut pages = 0u32;
        let mut from = 0usize;
        'batches: for _ in 0..MAX_BATCHES {
            let Ok(next) = nexus_abi::swap_export(pid, from, buf) else { break };
            let Some(entries) = SwapEntries::decode(buf) else { break };
            for entry in entries {
                // A full partition ends the swap-out: the rest stays in the pool (exported, a
                // later export supersedes it).
                let Some(at) = slots.take(pid, entry.token) else { break 'batches };
                let written = encode_slot(entry.sealed, block).is_some()
                    && dev.write_blocks(u64::from(at) * SECTORS_PER_SLOT, &block[..]).is_ok();
                if written && nexus_abi::swap_commit(entry.token, entry.ctr, at).is_ok() {
                    pages += 1;
                } else {
                    slots.release(at);
                }
            }
            if next == 0 {
                break;
            }
            from = next;
        }
        let held = slots.held_by(pid).len().min(u32::MAX as usize) as u32;
        Some(SwappedOut { pages, slots: held })
    }

    /// Answers the kernel's request for `token` from `slot`.
    pub fn page_in(&mut self, token: u64, slot: u32) -> PageIn {
        let outcome = self.answer(token, slot);
        if outcome == PageIn::Later
            && self.retry.len() < MAX_RETRIES
            && !self.retry.contains(&(token, slot))
        {
            self.retry.push((token, slot));
        }
        outcome
    }

    /// The page-ins that found no frame, once more.
    pub fn retry(&mut self) {
        let waiting = core::mem::take(&mut self.retry);
        for (token, slot) in waiting {
            if self.answer(token, slot) == PageIn::Later {
                self.retry.push((token, slot));
            }
        }
    }

    fn answer(&mut self, token: u64, slot: u32) -> PageIn {
        let Self { state, slot: bytes, .. } = self;
        let State::Ready { dev, slots } = state else { return PageIn::Unknown };
        if slots.owner(slot).is_none_or(|u| u.token != token) {
            return PageIn::Unknown;
        }
        if dev.read_blocks(u64::from(slot) * SECTORS_PER_SLOT, bytes).is_err() {
            return PageIn::Later;
        }
        // A slot that holds no record (the disk changed it) is handed over as it is: the kernel
        // refuses it like any record that does not open.
        let sealed = decode_slot(bytes).unwrap_or(&bytes[..16]);
        match nexus_abi::swap_page_in(token, slot, sealed) {
            Ok(()) => {
                slots.release(slot);
                PageIn::Back
            }
            Err(AbiError::NoSuchPid) => {
                slots.release(slot);
                PageIn::Stale
            }
            Err(AbiError::InvalidArgument) => PageIn::Dead,
            Err(_) => PageIn::Later,
        }
    }

    /// A thaw: every slot of `pid` paged back before it asks. Answers `(back, held)`.
    pub fn swap_in(&mut self, pid: u32) -> (u32, u32) {
        let held = match &self.state {
            State::Ready { slots, .. } => slots.held_by(pid),
            _ => return (0, 0),
        };
        let back = held.iter().filter(|u| self.page_in(u.token, u.slot) == PageIn::Back).count();
        (back as u32, held.len() as u32)
    }

    /// The kernel's drop: the slots of a dead object are free. Answers how many were ours.
    pub fn dropped(&mut self, freed: &[u32]) -> usize {
        let State::Ready { slots, .. } = &mut self.state else { return 0 };
        self.retry.retain(|(_, slot)| !freed.contains(slot));
        freed.iter().filter(|slot| slots.release(**slot).is_some()).count()
    }
}
