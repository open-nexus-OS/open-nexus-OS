// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Parked route asks (RFC-0093 §1, TASK-0324 P3). A route to a service that is
//! momentarily UNRESOLVABLE — the supervisor marked it stale and is restarting it
//! (ADR-0057) — is held here and answered EXACTLY ONCE when the route is re-provisioned,
//! instead of answering `STATUS_STALE` and letting the client re-ask in a loop. That client
//! loop was the re-ask storm that filled init's control queue (TASK-0324 P2 finding).
//!
//! Scope decision (recorded 2026-09-09, amends the RFC-0093 §1 seed): an ask is NEVER parked
//! on the target's *readiness*. Readiness is a service-side fact and waiting for it inside a
//! synchronous route ask is cyclic — `bundlemgrd` asks for `metricsd`, and `metricsd` boots
//! from the very volume `bundlemgrd` serves. Ordering by readiness is the stage fence's job
//! (P5); routing only waits on init's OWN bookkeeping.
//!
//! OWNERS: @runtime
//! STATUS: Production (TASK-0324 P3)
//! API_STABILITY: Internal
//! TEST_COVERAGE: Unit tests below (`cargo test -p nexus-init`)

use crate::service_topology::ServiceId;

/// Upper bound on simultaneously parked asks. One restarting service can hold at most one
/// ask per requester; 32 covers the whole fleet asking a single dead target at once.
pub const PARK_MAX: usize = 32;

/// A route ask waiting for its target to become resolvable again.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ParkedRoute {
    /// Index into init's control-channel list (the requester).
    pub chan: u16,
    /// The service the requester asked for.
    pub target: ServiceId,
    /// The requester's nonce — the reply MUST echo it (RFC-0093 §1).
    pub nonce: u32,
}

/// Why an ask could not be parked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParkError {
    /// More than [`PARK_MAX`] asks outstanding — the caller answers `STATUS_STALE`
    /// and prints a loud marker; a silently dropped ask would hang the client.
    Full,
}

/// Bounded set of parked route asks.
#[derive(Debug, Clone)]
pub struct RoutePark {
    entries: [Option<ParkedRoute>; PARK_MAX],
    len: usize,
}

impl Default for RoutePark {
    fn default() -> Self {
        Self::new()
    }
}

impl RoutePark {
    /// Empty park.
    pub const fn new() -> Self {
        Self { entries: [None; PARK_MAX], len: 0 }
    }

    /// Parks `ask`. A second ask from the same requester for the same target REPLACES the
    /// first: the client re-asked with a fresh nonce (its own budget expired), and answering
    /// the superseded nonce would hand it a frame it must drop.
    pub fn park(&mut self, ask: ParkedRoute) -> Result<(), ParkError> {
        if let Some(slot) = self.entries[..self.len]
            .iter_mut()
            .flatten()
            .find(|e| e.chan == ask.chan && e.target == ask.target)
        {
            slot.nonce = ask.nonce;
            return Ok(());
        }
        if self.len >= PARK_MAX {
            return Err(ParkError::Full);
        }
        self.entries[self.len] = Some(ask);
        self.len += 1;
        Ok(())
    }

    /// Removes every ask whose target `resolvable` reports as answerable now and copies it
    /// into `out`; returns how many were written. Entries that do not fit stay parked.
    pub fn take_resolved<F>(&mut self, mut resolvable: F, out: &mut [ParkedRoute]) -> usize
    where
        F: FnMut(ServiceId) -> bool,
    {
        let mut written = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let Some(entry) = self.entries[i] else {
                i += 1;
                continue;
            };
            if written < out.len() && resolvable(entry.target) {
                out[written] = entry;
                written += 1;
                self.remove_at(i);
                continue;
            }
            i += 1;
        }
        written
    }

    /// Drops every ask from `chan` (its process exited; the reply would go to a corpse).
    pub fn drop_chan(&mut self, chan: u16) {
        let mut i = 0usize;
        while i < self.len {
            match self.entries[i] {
                Some(e) if e.chan == chan => self.remove_at(i),
                _ => i += 1,
            }
        }
    }

    /// Number of parked asks.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether nothing is parked.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn remove_at(&mut self, i: usize) {
        self.entries[i] = self.entries[self.len - 1];
        self.entries[self.len - 1] = None;
        self.len -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask(chan: u16, target: ServiceId, nonce: u32) -> ParkedRoute {
        ParkedRoute { chan, target, nonce }
    }

    #[test]
    fn parked_ask_is_answered_once_when_the_target_resolves() {
        let mut park = RoutePark::new();
        park.park(ask(1, ServiceId::Statefsd, 7)).unwrap();
        let mut out = [ask(0, ServiceId::Vfsd, 0); 4];
        // Target still unresolvable: nothing comes back, the ask stays parked.
        assert_eq!(park.take_resolved(|_| false, &mut out), 0);
        assert_eq!(park.len(), 1);
        // Re-provisioned: answered exactly once.
        assert_eq!(park.take_resolved(|_| true, &mut out), 1);
        assert_eq!(out[0], ask(1, ServiceId::Statefsd, 7));
        assert!(park.is_empty());
        assert_eq!(park.take_resolved(|_| true, &mut out), 0);
    }

    #[test]
    fn re_ask_replaces_the_superseded_nonce() {
        let mut park = RoutePark::new();
        park.park(ask(1, ServiceId::Statefsd, 7)).unwrap();
        park.park(ask(1, ServiceId::Statefsd, 8)).unwrap();
        assert_eq!(park.len(), 1, "one requester + one target = one parked ask");
        let mut out = [ask(0, ServiceId::Vfsd, 0); 4];
        assert_eq!(park.take_resolved(|_| true, &mut out), 1);
        assert_eq!(out[0].nonce, 8);
    }

    #[test]
    fn only_the_resolvable_target_is_taken() {
        let mut park = RoutePark::new();
        park.park(ask(1, ServiceId::Statefsd, 1)).unwrap();
        park.park(ask(2, ServiceId::Vfsd, 2)).unwrap();
        let mut out = [ask(0, ServiceId::Vfsd, 0); 4];
        let n = park.take_resolved(|t| t == ServiceId::Vfsd, &mut out);
        assert_eq!(n, 1);
        assert_eq!(out[0].target, ServiceId::Vfsd);
        assert_eq!(park.len(), 1);
    }

    #[test]
    fn test_reject_park_overflow_is_loud() {
        let mut park = RoutePark::new();
        for chan in 0..PARK_MAX as u16 {
            park.park(ask(chan, ServiceId::Statefsd, chan as u32)).unwrap();
        }
        assert_eq!(park.park(ask(9999, ServiceId::Statefsd, 1)), Err(ParkError::Full));
        assert_eq!(park.len(), PARK_MAX);
    }

    #[test]
    fn exited_requester_loses_its_parked_asks() {
        let mut park = RoutePark::new();
        park.park(ask(1, ServiceId::Statefsd, 1)).unwrap();
        park.park(ask(1, ServiceId::Vfsd, 2)).unwrap();
        park.park(ask(2, ServiceId::Vfsd, 3)).unwrap();
        park.drop_chan(1);
        assert_eq!(park.len(), 1);
        let mut out = [ask(0, ServiceId::Vfsd, 0); 4];
        assert_eq!(park.take_resolved(|_| true, &mut out), 1);
        assert_eq!(out[0].chan, 2);
    }

    #[test]
    fn take_resolved_respects_the_output_bound() {
        let mut park = RoutePark::new();
        for chan in 0..4u16 {
            park.park(ask(chan, ServiceId::Statefsd, chan as u32)).unwrap();
        }
        let mut out = [ask(0, ServiceId::Vfsd, 0); 2];
        assert_eq!(park.take_resolved(|_| true, &mut out), 2);
        assert_eq!(park.len(), 2, "asks that did not fit stay parked");
    }
}
