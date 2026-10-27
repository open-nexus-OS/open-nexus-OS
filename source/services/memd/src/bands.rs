// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the band table and the victim rule (RFC-0100 §3). One entry per live app process
//! the lifecycle authority named, bounded; a band change keeps the entry's age only when the
//! band is unchanged (the age is "how long in THIS band", so the longest-hidden goes first).
//! The victim at a level: the highest band number at or above the level's kill floor, and
//! among those the oldest — never below the floor, so the foreground is never a victim and at
//! `critical` nothing on screen is.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: tests/contract.rs (`test_reject_kill_of_foreground_or_visible`, the order)

use nexus_abi::syscall::memory::{MEM_PRESSURE_CRITICAL, MEM_PRESSURE_EMERGENCY};
use nexus_wire::memd::{band_known, BAND_GONE, BAND_HIDDEN, BAND_SUSPENDED, BAND_VISIBLE};

/// Most app processes the table holds (the lifecycle authority's own bound is 32 instances).
pub const MAX_PROCESSES: usize = 32;

/// One live app process and its band.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The process (execd's pid — the key the lifecycle authority and the parent speak).
    pub pid: u32,
    /// Its band (`nexus_wire::memd::BAND_*`, never `BAND_GONE`).
    pub band: u8,
    /// The table's sequence number when the process entered this band (smaller = longer there).
    pub since: u64,
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
    /// The table is full: dropped (a process this authority cannot see is never a victim —
    /// it is also never protected; the bound is the lifecycle authority's too).
    Full,
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
    entries: [Option<Entry>; MAX_PROCESSES],
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
        Self { entries: [None; MAX_PROCESSES], seq: 0 }
    }

    /// Applies one push.
    pub fn apply(&mut self, pid: u32, band: u8) -> Applied {
        if !band_known(band) {
            return Applied::UnknownBand;
        }
        let slot = self.entries.iter().position(|e| e.is_some_and(|e| e.pid == pid));
        if band == BAND_GONE {
            return match slot {
                Some(i) => {
                    let was = self.entries[i].map_or(BAND_GONE, |e| e.band);
                    self.entries[i] = None;
                    Applied::Gone { was }
                }
                None => Applied::UnknownGone,
            };
        }
        self.seq = self.seq.wrapping_add(1);
        match slot {
            Some(i) => {
                let Some(e) = self.entries[i].as_mut() else { return Applied::UnknownGone };
                if e.band == band {
                    return Applied::Unchanged;
                }
                let from = e.band;
                e.band = band;
                e.since = self.seq;
                Applied::Moved { from }
            }
            None => match self.entries.iter().position(Option::is_none) {
                Some(i) => {
                    self.entries[i] = Some(Entry { pid, band, since: self.seq });
                    Applied::Entered
                }
                None => Applied::Full,
            },
        }
    }

    /// The entry of `pid`, if the table holds it.
    #[must_use]
    pub fn get(&self, pid: u32) -> Option<Entry> {
        self.entries.iter().flatten().find(|e| e.pid == pid).copied()
    }

    /// Live processes known.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.iter().flatten().count()
    }

    /// `true` when no process is known.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Processes in `band`.
    #[must_use]
    pub fn count_in(&self, band: u8) -> usize {
        self.entries.iter().flatten().filter(|e| e.band == band).count()
    }

    /// Processes in the suspended band.
    #[must_use]
    pub fn suspended(&self) -> usize {
        self.count_in(BAND_SUSPENDED)
    }

    /// The process a kill at `level` takes: the highest band at or above the level's floor, the
    /// longest in it first. `None` when the level kills nobody or no process is at or above
    /// the floor — the foreground is never here, and at `critical` nothing on screen is.
    #[must_use]
    pub fn victim(&self, level: u8) -> Option<Entry> {
        let floor = kill_floor(level)?;
        self.entries
            .iter()
            .flatten()
            .filter(|e| e.band >= floor)
            .copied()
            // Highest band first; within a band the smallest `since` (the longest there).
            .min_by_key(|e| (core::cmp::Reverse(e.band), e.since))
    }
}
