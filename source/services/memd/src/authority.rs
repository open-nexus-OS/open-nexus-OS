// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the authority's state machine (RFC-0100 §3/§5, ADR-0086/0087): the level the kernel
//! last said, the bands the lifecycle authority pushed, the kill in flight. At `critical` a
//! frozen app is SQUEEZED before anyone is killed (6f-1): the squeeze candidate
//! ([`crate::bands::Table::squeeze_candidate`]) is ordered through the carrier and answered
//! with the parent's totals at once; one decision per level judged — the OS loop reads a
//! kernel-sourced level again after a squeeze. With no candidate left, or at `emergency`, a
//! level at or past `critical` picks ONE victim ([`crate::bands::Table::victim`]) and orders it;
//! the kill is PROVEN only when the victim's `gone` band arrives (the parent reaped it, the
//! lifecycle authority reported it) — the marker says so then, not at the order. A
//! kernel-sourced kill asks for the level again once proven (`again`), so a pool still at
//! `critical` takes the next victim without a new frame (the kernel sends one per level
//! CHANGE). Pure; the OS loop owns the clock, the sink and the route.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: tests/contract.rs

use nexus_abi::syscall::memory::{
    MEM_PRESSURE_CRITICAL, MEM_PRESSURE_EMERGENCY, MEM_PRESSURE_NORMAL,
};

use crate::bands::{Applied, Table};
use crate::order::{Orders, Squeezed};

/// Where a level came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// The kernel's pressure frame: the pool IS this tight.
    Kernel,
    /// The proof harness's drill: a simulated level over the real bands.
    Drill,
}

impl Source {
    /// The `reason=` word on the console.
    #[must_use]
    pub const fn reason(self) -> &'static str {
        match self {
            Self::Kernel => "oom",
            Self::Drill => "drill",
        }
    }
}

/// A kill the authority ordered and waits to see proven.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pending {
    /// The victim.
    pub pid: u32,
    /// Its band when chosen.
    pub band: u8,
    /// Why.
    pub source: Source,
}

/// What a level did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// The level kills nobody (normal, warn), or a kill is already in flight.
    None,
    /// No process is at or above the level's floor.
    NoVictim,
    /// A victim was chosen and the order carried (`ordered`: the authority took it).
    Kill { pid: u32, band: u8, ordered: bool },
    /// A frozen app was squeezed before any kill (ADR-0087): the parent's totals, `None` when
    /// the order was refused (the app is not asked again until its band changes).
    Squeeze { pid: u32, band: u8, squeezed: Option<Squeezed> },
}

/// What a band push did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BandEvent {
    /// Applied (named for the console); `resumed` when a suspended process runs again.
    Applied { pid: u32, band: u8, applied: Applied },
    /// The pending kill's victim is gone: the kill is proven. `again` asks the OS loop to read
    /// the level once more (a kernel-sourced kill may need a successor).
    KillProven { pending: Pending, again: bool },
}

/// The authority.
#[derive(Debug)]
pub struct Authority {
    table: Table,
    level: u8,
    free: u64,
    total: u64,
    sink: bool,
    resumed: u16,
    kills: u16,
    squeezes: u16,
    pending: Option<Pending>,
}

impl Authority {
    /// A fresh authority; `sink` says whether the kernel's pressure endpoint was bound.
    #[must_use]
    pub const fn new(sink: bool) -> Self {
        Self {
            table: Table::new(),
            level: MEM_PRESSURE_NORMAL,
            free: 0,
            total: 0,
            sink,
            resumed: 0,
            kills: 0,
            squeezes: 0,
            pending: None,
        }
    }

    /// The band table (read-only; the tests look at it).
    #[must_use]
    pub fn table(&self) -> &Table {
        &self.table
    }

    /// `(level, free, total)` of the last kernel frame.
    #[must_use]
    pub fn level(&self) -> (u8, u64, u64) {
        (self.level, self.free, self.total)
    }

    /// Whether the kernel's sink is this authority's.
    #[must_use]
    pub fn sink(&self) -> bool {
        self.sink
    }

    /// Suspended-to-running transitions seen.
    #[must_use]
    pub fn resumed(&self) -> u16 {
        self.resumed
    }

    /// Kills ordered.
    #[must_use]
    pub fn kills(&self) -> u16 {
        self.kills
    }

    /// Squeezes carried out.
    #[must_use]
    pub fn squeezes(&self) -> u16 {
        self.squeezes
    }

    /// The kill in flight, if any.
    #[must_use]
    pub fn pending(&self) -> Option<Pending> {
        self.pending
    }

    /// One band push from the lifecycle authority.
    pub fn on_band(&mut self, pid: u32, band: u8) -> BandEvent {
        let applied = self.table.apply(pid, band);
        if let Applied::Moved { from } = applied {
            if from == nexus_wire::memd::BAND_SUSPENDED {
                self.resumed = self.resumed.saturating_add(1);
            }
        }
        if let (Applied::Gone { .. }, Some(p)) = (applied, self.pending) {
            if p.pid == pid {
                self.pending = None;
                return BandEvent::KillProven { pending: p, again: p.source == Source::Kernel };
            }
        }
        BandEvent::Applied { pid, band, applied }
    }

    /// One kernel pressure frame: remembered, then judged like any level.
    pub fn on_pressure(
        &mut self,
        level: u8,
        free: u64,
        total: u64,
        orders: &mut dyn Orders,
    ) -> Decision {
        self.level = level;
        self.free = free;
        self.total = total;
        self.judge(level, Source::Kernel, orders)
    }

    /// A level from `source`: at `critical` a frozen app not yet squeezed is squeezed first
    /// (ADR-0087); otherwise, at or past `critical`, one victim is ordered killed unless a kill
    /// is already in flight (the pool is re-read when that one is proven).
    pub fn judge(&mut self, level: u8, source: Source, orders: &mut dyn Orders) -> Decision {
        if level > MEM_PRESSURE_EMERGENCY || crate::bands::kill_floor(level).is_none() {
            return Decision::None;
        }
        if self.pending.is_some() {
            return Decision::None;
        }
        if level == MEM_PRESSURE_CRITICAL {
            if let Some(candidate) = self.table.squeeze_candidate() {
                self.table.mark_squeezed(candidate.pid);
                let squeezed = orders.squeeze(candidate.pid);
                if squeezed.is_some() {
                    self.squeezes = self.squeezes.saturating_add(1);
                }
                return Decision::Squeeze { pid: candidate.pid, band: candidate.band, squeezed };
            }
        }
        let Some(victim) = self.table.victim(level) else {
            return Decision::NoVictim;
        };
        let ordered = orders.kill(victim.pid);
        if ordered {
            self.kills = self.kills.saturating_add(1);
            self.pending = Some(Pending { pid: victim.pid, band: victim.band, source });
        }
        Decision::Kill { pid: victim.pid, band: victim.band, ordered }
    }
}
