// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the band table and the victim rule (RFC-0100 §3). One entry per live app process
//! the lifecycle authority named — as many as the apps (TASK-0352 P1, ADR-0093: no constant, the
//! heap bounds the table and a push the heap refuses is dropped by name); a band change keeps the
//! entry's age only when the band is unchanged (the age is "how long in THIS band", so the
//! longest-hidden goes first). The victim at a level: the highest band number at or above the
//! level's kill floor, and among those the oldest — never below the floor, so the foreground is
//! never a victim and at `critical` nothing on screen is. Before any kill at `critical`
//! (TASK-0333 6f-1, ADR-0087) the squeeze candidate: the process longest in the suspended band
//! that was not squeezed since it entered its band — a band change makes it a candidate again.
//! After the squeeze (6f-2, ADR-0088) the swap candidate: the process longest in the suspended
//! band that was squeezed and not swapped out since it entered its band. Every entry names its
//! OWNER (TASK-0352 P2): the app's kernel service id the lifecycle authority pushed, the key gpud
//! charges the app's surfaces by — so a charge is joined to the processes it is for.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: tests/contract.rs (`test_reject_kill_of_foreground_or_visible`, the order,
//!   the table past 32)

use alloc::vec::Vec;
use nexus_abi::syscall::memory::{MEM_PRESSURE_CRITICAL, MEM_PRESSURE_EMERGENCY};
use nexus_wire::memd::{band_known, BAND_GONE, BAND_HIDDEN, BAND_SUSPENDED, BAND_VISIBLE};

/// One live app process and its band.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The process (execd's pid — the key the lifecycle authority and the parent speak).
    pub pid: u32,
    /// Its owner: the app's kernel service id (`app:<bundle id>`), shared by every instance of
    /// the app and the key gpud charges the app's surfaces by; 0 when the authority knows none.
    pub owner: u64,
    /// Its band (`nexus_wire::memd::BAND_*`, never `BAND_GONE`).
    pub band: u8,
    /// The table's sequence number when the process entered this band (smaller = longer there).
    pub since: u64,
    /// Squeezed since it entered this band (ADR-0087): not a squeeze candidate again until its
    /// band changes.
    pub squeezed: bool,
    /// Swapped out since it entered this band (ADR-0088): not a swap candidate again until its
    /// band changes.
    pub swapped: bool,
}

/// What applying one band push did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Applied {
    /// The process entered the table.
    Entered,
    /// The process moved from `from` to the pushed band.
    Moved { from: u8 },
    /// The process is in that band already (the authority re-stated it).
    Unchanged,
    /// The process left the table (`BAND_GONE`); `was` its last band.
    Gone { was: u8 },
    /// `BAND_GONE` for a process the table never held.
    UnknownGone,
    /// A band this build does not know: dropped.
    UnknownBand,
    /// The heap refused the entry: dropped, named (a process this authority cannot see is never
    /// a victim — it is also never protected; the authority's next push tries again).
    NoHeap,
}

/// The lowest band number a kill at `level` may take, `None` when the level kills nobody.
#[must_use]
pub const fn kill_floor(level: u8) -> Option<u8> {
    match level {
        MEM_PRESSURE_EMERGENCY => Some(BAND_VISIBLE),
        MEM_PRESSURE_CRITICAL => Some(BAND_HIDDEN),
        _ => None,
    }
}

/// The band table.
#[derive(Clone, Debug)]
pub struct Table {
    entries: Vec<Entry>,
    seq: u64,
}

impl Default for Table {
    fn default() -> Self {
        Self::new()
    }
}

impl Table {
    /// An empty table.
    #[must_use]
    pub const fn new() -> Self {
        Self { entries: Vec::new(), seq: 0 }
    }

    /// Applies one push: `owner` the process's kernel service id as the authority names it.
    pub fn apply(&mut self, pid: u32, band: u8, owner: u64) -> Applied {
        if !band_known(band) {
            return Applied::UnknownBand;
        }
        let slot = self.entries.iter().position(|e| e.pid == pid);
        if band == BAND_GONE {
            return match slot {
                Some(i) => Applied::Gone { was: self.entries.remove(i).band },
                None => Applied::UnknownGone,
            };
        }
        self.seq = self.seq.wrapping_add(1);
        match slot {
            Some(i) => {
                let e = &mut self.entries[i];
                e.owner = owner;
                if e.band == band {
                    return Applied::Unchanged;
                }
                let from = e.band;
                e.band = band;
                e.since = self.seq;
                e.squeezed = false;
                e.swapped = false;
                Applied::Moved { from }
            }
            None => {
                if self.entries.try_reserve(1).is_err() {
                    return Applied::NoHeap;
                }
                self.entries.push(Entry {
                    pid,
                    owner,
                    band,
                    since: self.seq,
                    squeezed: false,
                    swapped: false,
                });
                Applied::Entered
            }
        }
    }

    /// The entry of `pid`, if the table holds it.
    #[must_use]
    pub fn get(&self, pid: u32) -> Option<Entry> {
        self.entries.iter().find(|e| e.pid == pid).copied()
    }

    /// Live processes known.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` when no process is known.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Processes of `owner` (an app's instances): what a GPU charge to that owner is for.
    #[must_use]
    pub fn processes_of(&self, owner: u64) -> usize {
        self.entries.iter().filter(|e| e.owner == owner).count()
    }

    /// Processes in `band`.
    #[must_use]
    pub fn count_in(&self, band: u8) -> usize {
        self.entries.iter().filter(|e| e.band == band).count()
    }

    /// Processes in the suspended band.
    #[must_use]
    pub fn suspended(&self) -> usize {
        self.count_in(BAND_SUSPENDED)
    }

    /// The process a squeeze takes before any kill at `critical` (ADR-0087): the one longest in
    /// the suspended band that was not squeezed since it entered it. `None` when there is none.
    #[must_use]
    pub fn squeeze_candidate(&self) -> Option<Entry> {
        self.entries
            .iter()
            .filter(|e| e.band == BAND_SUSPENDED && !e.squeezed)
            .copied()
            .min_by_key(|e| e.since)
    }

    /// Mark `pid` squeezed (asked once per stay in its band, whatever the answer).
    pub fn mark_squeezed(&mut self, pid: u32) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.pid == pid) {
            e.squeezed = true;
        }
    }

    /// The process the pager takes after the squeeze and before any kill at `critical`
    /// (ADR-0088): the one longest in the suspended band that was squeezed and not swapped out
    /// since it entered it. `None` when there is none.
    #[must_use]
    pub fn swap_candidate(&self) -> Option<Entry> {
        self.entries
            .iter()
            .filter(|e| e.band == BAND_SUSPENDED && e.squeezed && !e.swapped)
            .copied()
            .min_by_key(|e| e.since)
    }

    /// Mark `pid` swapped out (asked once per stay in its band, whatever the answer).
    pub fn mark_swapped(&mut self, pid: u32) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.pid == pid) {
            e.swapped = true;
        }
    }

    /// The process a kill at `level` takes: the highest band at or above the level's floor, the
    /// longest in it first. `None` when the level kills nobody or no process is at or above
    /// the floor — the foreground is never here, and at `critical` nothing on screen is.
    #[must_use]
    pub fn victim(&self, level: u8) -> Option<Entry> {
        let floor = kill_floor(level)?;
        self.entries
            .iter()
            .filter(|e| e.band >= floor)
            .copied()
            // Highest band first; within a band the smallest `since` (the longest there).
            .min_by_key(|e| (core::cmp::Reverse(e.band), e.since))
    }
}
