// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The driver's in-flight requests and what the device's interrupts say, as pure
//! logic (TASK-0333 6f-3, ADR-0089). A slot is the driver's until it submits a chain on it, and
//! the device's from then until that chain comes back on the used ring. A request whose waiter
//! fails stays the DEVICE's — its buffers may still be read or written — and only its late
//! completion frees the slot (before this table a 2 s watchdog gave requests up, their slots
//! stayed busy for the rest of the boot, and four host stalls left a device that answered every
//! later request with `slots exhausted`, 2026-10-29). The wait itself is reactive: a request
//! waits for the device's interrupt, and a device that failed says so with its own event — a
//! configuration-change interrupt with `DEVICE_NEEDS_RESET` in its status ([`interrupt`]).
//! OWNERS: @storage @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `tests/inflight.rs` (host) — claim/complete/release, a late completion
//!   reclaiming an abandoned slot, the interrupt's meaning, `test_reject_*` for an unknown head,
//!   a release while the device owns the slot, a reuse before the late completion, a
//!   configuration change that is no failure

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Slot {
    /// The driver's: free to claim.
    Free,
    /// Submitted; its waiter waits for the chain to come back.
    Waiting { head: u16 },
    /// Back on the used ring; its waiter has not collected it yet.
    Done,
    /// Given up by its waiter; the device still owns the chain.
    Abandoned { head: u16 },
}

/// What a completion on the used ring meant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Completion {
    /// The waiter's request is done (slot index).
    Waiter(usize),
    /// A request its waiter gave up on came back late: the slot is free again.
    Reclaimed(usize),
    /// No slot waits for this head (a duplicate or a stray id): ignored.
    Unknown,
}

/// The request slots of one queue.
#[derive(Clone, Debug)]
pub struct InFlight<const N: usize> {
    slots: [Slot; N],
}

impl<const N: usize> Default for InFlight<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> InFlight<N> {
    /// Every slot free.
    #[must_use]
    pub const fn new() -> Self {
        Self { slots: [Slot::Free; N] }
    }

    /// A free slot to build a request in, or `None` while the device owns every one. Claiming
    /// changes nothing: the slot is the device's only once [`Self::submitted`] says so.
    #[must_use]
    pub fn claim(&self) -> Option<usize> {
        self.slots.iter().position(|s| *s == Slot::Free)
    }

    /// The chain built in free slot `idx` was published with descriptor `head`. A slot that is
    /// not free is left alone (`false`): the device's buffers are never handed out twice.
    pub fn submitted(&mut self, idx: usize, head: u16) -> bool {
        match self.slots.get_mut(idx) {
            Some(slot @ Slot::Free) => {
                *slot = Slot::Waiting { head };
                true
            }
            _ => false,
        }
    }

    /// The used ring returned `head`.
    pub fn completed(&mut self, head: u16) -> Completion {
        for (idx, slot) in self.slots.iter_mut().enumerate() {
            match *slot {
                Slot::Waiting { head: h } if h == head => {
                    *slot = Slot::Done;
                    return Completion::Waiter(idx);
                }
                Slot::Abandoned { head: h } if h == head => {
                    *slot = Slot::Free;
                    return Completion::Reclaimed(idx);
                }
                _ => {}
            }
        }
        Completion::Unknown
    }

    /// Whether the request in slot `idx` is back.
    #[must_use]
    pub fn is_done(&self, idx: usize) -> bool {
        self.slots.get(idx) == Some(&Slot::Done)
    }

    /// The waiter collected its completed request: the slot is the driver's again. Only a
    /// DONE slot is released (`false` otherwise) — a slot the device still owns never is.
    pub fn release(&mut self, idx: usize) -> bool {
        match self.slots.get_mut(idx) {
            Some(slot @ Slot::Done) => {
                *slot = Slot::Free;
                true
            }
            _ => false,
        }
    }

    /// The waiter gave up on slot `idx`: a request still waiting stays the device's until its
    /// chain comes back; one that completed meanwhile is simply free.
    pub fn abandon(&mut self, idx: usize) {
        if let Some(slot) = self.slots.get_mut(idx) {
            *slot = match *slot {
                Slot::Waiting { head } => Slot::Abandoned { head },
                Slot::Done => Slot::Free,
                other => other,
            };
        }
    }

    /// Slots the device holds for requests nobody waits for any more.
    #[must_use]
    pub fn abandoned(&self) -> usize {
        self.slots.iter().filter(|s| matches!(s, Slot::Abandoned { .. })).count()
    }
}

/// `InterruptStatus`: a buffer came back on the used ring.
pub const ISR_USED: u32 = 1;
/// `InterruptStatus`: the device's configuration changed.
pub const ISR_CONFIG: u32 = 2;
/// Device status: the device hit an error it cannot recover from on its own.
pub const STATUS_NEEDS_RESET: u32 = 64;

/// What one interrupt of the device said.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceSays {
    /// Completions are on the used ring.
    Used,
    /// The device failed (`DEVICE_NEEDS_RESET`): nothing in flight comes back.
    Failed,
    /// Nothing for the requests (a configuration change that is no failure, or a wake without
    /// a cause).
    Nothing,
}

/// The meaning of `isr` (`InterruptStatus`), with the device `status` read on a configuration
/// change. A failure outranks completions that came with it: the device's state is gone.
#[must_use]
pub fn interrupt(isr: u32, status: u32) -> DeviceSays {
    if isr & ISR_CONFIG != 0 && status & STATUS_NEEDS_RESET != 0 {
        DeviceSays::Failed
    } else if isr & ISR_USED != 0 {
        DeviceSays::Used
    } else {
        DeviceSays::Nothing
    }
}
