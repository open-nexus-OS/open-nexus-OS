// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: What a server's respawn re-pins in its clients (RFC-0112, amendment 2026-11-02) —
//! every declared leg of every service that routes TO the server, read from `SERVICE_SPECS`, so
//! a new client of a respawnable server is re-pinned by declaring it, never by a list kept
//! beside the declarations. A leg's SEND half named the dead server's request endpoint; a
//! `SharedResponse` leg's RECV half its response endpoint (both die with their owner); a
//! `ReplyInbox`/`PrivateInbox` leg's replies ride the client's own inbox, which survives. And
//! the handles a respawned server's own legs are pinned from (`LegHandles`): what init keeps of
//! bootstrap for it. Pure and host-tested; applied by `bootstrap::windowd_respawn`.
//! OWNERS: @runtime @reliability
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below (windowd's five client legs; every one of its own legs answered)

use crate::service_topology::{RouteKind, ServiceId, SlotPair, SERVICE_SPECS};

/// One client leg into a server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClientLeg {
    /// The service that routes to the server.
    pub client: ServiceId,
    /// How the server's replies reach it.
    pub kind: RouteKind,
    /// Its declared slots of the leg.
    pub slots: SlotPair,
}

impl ClientLeg {
    /// Whether the leg's RECV half names the server's response endpoint (`SharedResponse`): it
    /// died with the server and is re-pinned with the SEND half.
    #[must_use]
    pub fn takes_response(&self) -> bool {
        matches!(self.kind, RouteKind::SharedResponse)
    }
}

/// Every declared leg into `server`, in `SERVICE_SPECS` order.
pub fn client_legs_of(server: ServiceId) -> impl Iterator<Item = ClientLeg> {
    SERVICE_SPECS.iter().flat_map(move |spec| {
        spec.routes_to.iter().filter(move |route| route.to == server).map(move |route| ClientLeg {
            client: spec.id,
            kind: route.kind,
            slots: route.slots,
        })
    })
}

/// The endpoints a respawned server's own legs are pinned from, kept from bootstrap: a server
/// pair (request, response) per `SharedResponse` target, a request endpoint per `ReplyInbox`
/// target. Init's slot numbers; `None` where the image does not carry the target.
#[derive(Clone, Copy, Debug, Default)]
pub struct LegHandles {
    pairs: [(Option<ServiceId>, (u32, u32)); MAX_KEPT],
    requests: [(Option<ServiceId>, u32); MAX_KEPT],
}

/// Targets one respawnable server's legs name (windowd: seven).
const MAX_KEPT: usize = 8;

impl LegHandles {
    /// Keeps `target`'s server pair.
    pub fn keep_pair(&mut self, target: ServiceId, pair: Option<(u32, u32)>) {
        if let (Some(pair), Some(free)) = (pair, self.pairs.iter_mut().find(|p| p.0.is_none())) {
            *free = (Some(target), pair);
        }
    }

    /// Keeps `target`'s request endpoint.
    pub fn keep_request(&mut self, target: ServiceId, req: Option<u32>) {
        if let (Some(req), Some(free)) = (req, self.requests.iter_mut().find(|r| r.0.is_none())) {
            *free = (Some(target), req);
        }
    }

    /// `target`'s server pair, if kept.
    #[must_use]
    pub fn pair(&self, target: ServiceId) -> Option<(u32, u32)> {
        self.pairs.iter().find(|p| p.0 == Some(target)).map(|p| p.1)
    }

    /// The request endpoint a leg to `target` sends on: a kept request endpoint, or the
    /// request half of a kept pair.
    #[must_use]
    pub fn request(&self, target: ServiceId) -> Option<u32> {
        self.requests
            .iter()
            .find(|r| r.0 == Some(target))
            .map(|r| r.1)
            .or_else(|| self.pair(target).map(|(req, _)| req))
    }

    /// Whether every leg `server`'s declaration names has its endpoint here.
    #[must_use]
    pub fn answers_every_leg_of(&self, server: ServiceId) -> bool {
        let Some(spec) = SERVICE_SPECS.iter().find(|s| s.id == server) else { return false };
        spec.routes_to.iter().all(|route| match route.kind {
            RouteKind::SharedResponse => self.pair(route.to).is_some(),
            RouteKind::ReplyInbox | RouteKind::PrivateInbox { .. } => {
                self.request(route.to).is_some()
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC-0112: the plan names every client the fleet declares into windowd — inputd, imed,
    /// execd (their SEND and the shared response RECV), screencapd and memd (their SEND; replies
    /// ride their own inboxes) — and nothing else.
    #[test]
    fn windowd_respawn_repins_every_declared_client_leg() {
        let mut legs: alloc::vec::Vec<(ServiceId, u32, Option<u32>)> =
            client_legs_of(ServiceId::Windowd)
                .map(|l| (l.client, l.slots.send, l.takes_response().then_some(l.slots.recv)))
                .collect();
        legs.sort_by_key(|l| l.0 as usize);
        let mut want = alloc::vec![
            (ServiceId::Inputd, 5, Some(6)),
            (ServiceId::Imed, 6, Some(7)),
            (ServiceId::Execd, 8, Some(9)),
            (ServiceId::Screencapd, 7, None),
            (ServiceId::Memd, 11, None),
        ];
        want.sort_by_key(|l| l.0 as usize);
        assert_eq!(legs, want);
    }

    /// The handles init keeps answer every leg windowd's declaration names — a leg the respawn
    /// could not pin would be a dead route after the restart.
    #[test]
    fn kept_handles_answer_every_windowd_leg() {
        let mut kept = LegHandles::default();
        kept.keep_pair(ServiceId::Gpud, Some((40, 41)));
        kept.keep_pair(ServiceId::Abilitymgr, Some((42, 43)));
        for (n, target) in [
            ServiceId::Bundlemgrd,
            ServiceId::Sessiond,
            ServiceId::Settingsd,
            ServiceId::Imed,
            ServiceId::Clipboardd,
        ]
        .into_iter()
        .enumerate()
        {
            kept.keep_request(target, Some(50 + n as u32));
        }
        assert!(kept.answers_every_leg_of(ServiceId::Windowd));
        assert_eq!(kept.request(ServiceId::Gpud), Some(40), "a pair's request half");
        assert_eq!(kept.request(ServiceId::Imed), Some(53));
    }

    #[test]
    fn test_reject_a_missing_leg_handle() {
        let mut kept = LegHandles::default();
        kept.keep_pair(ServiceId::Gpud, Some((40, 41)));
        kept.keep_pair(ServiceId::Abilitymgr, None);
        assert!(!kept.answers_every_leg_of(ServiceId::Windowd));
    }
}
