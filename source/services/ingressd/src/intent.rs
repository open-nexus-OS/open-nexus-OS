// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The identity-bound intent registry (RFC-0092 §3). One slot per
//! table entry; an exposure opens only when the kernel-attributed sender IS
//! the declared subject AND policyd (the authority, reached through
//! `IntentHost`) grants `net.expose` — an unreachable authority refuses
//! (fail closed). The accept side admits a peer only through the CIDR
//! allow-list and the exposure's token bucket; every refusal is labelled
//! and counted where it happens.
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: tests/ingress_host/ (allow, `test_reject_intent_policy_denied`,
//!   `test_reject_forged_intent_sender`, `test_reject_cidr`, `test_reject_rate_exceeded`)

use crate::cidr;
use crate::rate::TokenBucket;
use crate::table::{lookup, ExposeEntry, Proto};
use crate::wire::{Counters, Reason};

/// Open exposures per boot (mirrors the grammar's `MAX_EXPOSURES_TOTAL`).
pub const MAX_OPEN_EXPOSURES: usize = 64;

/// The authority is unreachable (policyd routing/IPC failure).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostError;

/// The seam to policyd. The OS host (P3) issues
/// `OP_CHECK_CAP_DELEGATED(subject, "net.expose")` over the fixed slots init
/// wires; host tests script the verdicts.
pub trait IntentHost {
    /// `Ok(true)` = the subject holds `net.expose`; `Err` = unreachable.
    fn expose_capability(&mut self, subject: u64) -> Result<bool, HostError>;
}

#[derive(Debug, Clone, Copy)]
struct Slot {
    open: bool,
    bucket: TokenBucket,
    counters: Counters,
}

impl Slot {
    const IDLE: Self =
        Self { open: false, bucket: TokenBucket::new(1, 1), counters: Counters::default_const() };
}

impl Counters {
    const fn default_const() -> Self {
        Self { accepted: 0, denied_cidr: 0, denied_rate: 0 }
    }
}

/// Registry over a table (slot `i` ↔ `table[i]`).
pub struct Registry<'t, const N: usize = MAX_OPEN_EXPOSURES> {
    table: &'t [ExposeEntry],
    slots: [Slot; N],
}

impl<'t, const N: usize> Registry<'t, N> {
    /// Nothing open.
    pub const fn new(table: &'t [ExposeEntry]) -> Self {
        Self { table, slots: [Slot::IDLE; N] }
    }

    /// The table this registry fronts.
    pub fn table(&self) -> &'t [ExposeEntry] {
        self.table
    }

    /// Resolves `(port, proto)` for `sender`: declared (else `Policy`) and
    /// owned by the sender (else `Identity`), within the slot bound (else
    /// `Limit`).
    fn resolve(&self, sender: u64, port: u16, proto: Proto) -> Result<usize, Reason> {
        let (idx, entry) = lookup(self.table, port, proto).ok_or(Reason::Policy)?;
        if entry.subject_id != sender {
            return Err(Reason::Identity);
        }
        if idx >= N {
            return Err(Reason::Limit);
        }
        Ok(idx)
    }

    /// `OP_EXPOSE`: opens the exposure for its declared subject. Idempotent
    /// for the owner; the bucket starts full on a fresh open.
    pub fn open(
        &mut self,
        host: &mut impl IntentHost,
        sender: u64,
        port: u16,
        proto: Proto,
    ) -> Result<usize, Reason> {
        let idx = self.resolve(sender, port, proto)?;
        match host.expose_capability(sender) {
            Ok(true) => {}
            Ok(false) | Err(HostError) => return Err(Reason::Policy),
        }
        let entry = &self.table[idx];
        let slot = &mut self.slots[idx];
        if !slot.open {
            slot.open = true;
            slot.bucket = TokenBucket::new(entry.rate_per_s, entry.burst);
            slot.counters = Counters::default();
        }
        Ok(idx)
    }

    /// `OP_UNEXPOSE`: only the owner may close; closing a closed exposure
    /// is `Policy` (nothing to close).
    pub fn close(&mut self, sender: u64, port: u16, proto: Proto) -> Result<usize, Reason> {
        let idx = self.resolve(sender, port, proto)?;
        let slot = &mut self.slots[idx];
        if !slot.open {
            return Err(Reason::Policy);
        }
        slot.open = false;
        Ok(idx)
    }

    /// `OP_EXPOSE_STATUS`: authority observability — any `net.expose`
    /// holder may read any declared exposure's state.
    pub fn status(
        &mut self,
        host: &mut impl IntentHost,
        sender: u64,
        port: u16,
        proto: Proto,
    ) -> Result<(bool, Counters), Reason> {
        match host.expose_capability(sender) {
            Ok(true) => {}
            Ok(false) | Err(HostError) => return Err(Reason::Policy),
        }
        let (idx, _) = lookup(self.table, port, proto).ok_or(Reason::Policy)?;
        let slot = self.slots.get(idx).ok_or(Reason::Limit)?;
        Ok((slot.open, slot.counters))
    }

    /// Accept side: admits `peer` on open exposure `idx` through the CIDR
    /// allow-list and the token bucket; refusals are counted.
    pub fn admit_peer(&mut self, idx: usize, peer: [u8; 4], now_ns: u64) -> Result<(), Reason> {
        let entry = self.table.get(idx).ok_or(Reason::Policy)?;
        let slot = self.slots.get_mut(idx).ok_or(Reason::Limit)?;
        if !slot.open {
            return Err(Reason::Policy);
        }
        if !cidr::allowed(entry.cidr_allow, peer) {
            slot.counters.denied_cidr = slot.counters.denied_cidr.saturating_add(1);
            return Err(Reason::Cidr);
        }
        if !slot.bucket.try_take(now_ns) {
            slot.counters.denied_rate = slot.counters.denied_rate.saturating_add(1);
            return Err(Reason::Rate);
        }
        slot.counters.accepted = slot.counters.accepted.saturating_add(1);
        Ok(())
    }

    /// Whether `idx` is open.
    pub fn is_open(&self, idx: usize) -> bool {
        self.slots.get(idx).is_some_and(|s| s.open)
    }

    /// Counters of `idx` (zero when unknown).
    pub fn counters(&self, idx: usize) -> Counters {
        self.slots.get(idx).map_or_else(Counters::default, |s| s.counters)
    }

    /// Open exposures right now.
    pub fn open_count(&self) -> usize {
        self.slots.iter().filter(|s| s.open).count()
    }
}
