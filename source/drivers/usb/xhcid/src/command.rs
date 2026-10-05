// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The command queue: the command ring is serial, so one command is in flight; the rest wait
//! here in order with the state machine that issued them. A completion names its command by
//! the TRB's bus address; a command that does not complete before its deadline is abandoned
//! and reported to its issuer as a timeout.

use crate::trb::Trb;

/// Commands waiting at most (devices × the two-command recoveries, with room).
const DEPTH: usize = 32;
/// How long a command may take.
pub const COMMAND_NS: u64 = 5_000_000_000;

/// Who issued a command (and gets its completion).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Issuer {
    /// The device at this index of the device table.
    Device(usize),
    /// Nobody: the device that issued it is gone (its completion is dropped).
    Orphan,
}

/// A command in flight.
#[derive(Clone, Copy, Debug)]
pub struct InFlight {
    /// The TRB's bus address.
    pub address: u64,
    /// Who waits for it.
    pub issuer: Issuer,
    /// When it is given up.
    pub deadline: u64,
}

/// The commands not yet submitted, oldest first, and the one in flight.
pub struct Queue {
    waiting: [Option<(Trb, Issuer)>; DEPTH],
    head: usize,
    count: usize,
    /// The command the controller works on.
    pub in_flight: Option<InFlight>,
}

impl Queue {
    /// An empty queue.
    #[must_use]
    pub const fn new() -> Self {
        Self { waiting: [None; DEPTH], head: 0, count: 0, in_flight: None }
    }

    /// Queue `trb` for `issuer`; false when full.
    pub fn push(&mut self, trb: Trb, issuer: Issuer) -> bool {
        if self.count == DEPTH {
            return false;
        }
        self.waiting[(self.head + self.count) % DEPTH] = Some((trb, issuer));
        self.count += 1;
        true
    }

    /// The next command to submit, if none is in flight.
    pub fn take_next(&mut self) -> Option<(Trb, Issuer)> {
        if self.in_flight.is_some() {
            return None;
        }
        while self.count > 0 {
            let item = self.waiting[self.head].take();
            self.head = (self.head + 1) % DEPTH;
            self.count -= 1;
            if item.is_some() {
                return item;
            }
        }
        None
    }

    /// Drop every command `issuer` still has waiting and orphan the one in flight (its
    /// device went away); true when one of its commands is still in flight.
    pub fn forget(&mut self, issuer: Issuer) -> bool {
        for i in 0..self.count {
            let at = (self.head + i) % DEPTH;
            if matches!(self.waiting[at], Some((_, who)) if who == issuer) {
                // An empty slot: `take_next` skips it.
                self.waiting[at] = None;
            }
        }
        self.in_flight.is_some_and(|c| c.issuer == issuer)
    }
}

impl Default for Queue {
    fn default() -> Self {
        Self::new()
    }
}
