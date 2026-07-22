// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Bootstrap endpoint caps — the bag of IPC request/response endpoint
//! capabilities init-lite mints before the per-service cap-distribution phase
//! (`wiring::wire_services`). One named, `Copy` struct replacing ~49 bare locals.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! RFC: docs/rfcs/RFC-0061-selftest-observer-init-refactoring.md

/// The IPC endpoint capabilities init-lite mints up front (one pair per service,
/// plus reply inboxes and the policyd control request endpoints). All handles are
/// raw cap slots in init-lite's own table; the wiring phase `cap_transfer`s them
/// into each service. `Copy` so the wiring phase can destructure it back into the
/// original local names without moving any caps.
#[derive(Clone, Copy)]
pub(crate) struct Endpoints {
    /// vfsd server request endpoint.
    pub vfs_req: u32,
    /// vfsd server response endpoint (owned by selftest-client).
    pub vfs_rsp: u32,
    /// packagefsd server request endpoint.
    pub pkg_req: u32,
    /// packagefsd server response endpoint.
    pub pkg_rsp: u32,
    /// packagefsd CAP_MOVE reply inbox.
    pub pkg_reply_ep: u32,
    /// policyd server request endpoint.
    pub pol_req: u32,
    /// policyd server response endpoint.
    pub pol_rsp: u32,
    /// bundlemgrd server request endpoint.
    pub bnd_req: u32,
    /// bundlemgrd server response endpoint.
    pub bnd_rsp: u32,
    /// bundlemgrd response endpoint owned by updated.
    pub bnd_rsp_updated: u32,
    /// bundlemgrd→execd dedicated request endpoint.
    pub bnd_exe_req: u32,
    /// bundlemgrd→execd dedicated response endpoint.
    pub bnd_exe_rsp: u32,
    /// updated server request endpoint.
    pub upd_req: u32,
    /// updated server response endpoint.
    pub upd_rsp: u32,
    /// samgrd server request endpoint.
    pub sam_req: u32,
    /// samgrd server response endpoint.
    pub sam_rsp: u32,
    /// execd server request endpoint.
    pub exe_req: u32,
    /// execd server response endpoint.
    pub exe_rsp: u32,
    /// keystored server request endpoint (init-owned).
    pub key_req: u32,
    /// keystored server response endpoint.
    pub key_rsp: u32,
    /// statefsd server request endpoint (init-owned).
    pub state_req: u32,
    /// statefsd server response endpoint.
    pub state_rsp: u32,
    /// rngd server request endpoint.
    pub rng_req: u32,
    /// rngd server response endpoint.
    pub rng_rsp: u32,
    /// timed server request endpoint.
    pub timed_req: u32,
    /// timed server response endpoint.
    pub timed_rsp: u32,
    /// imed server request endpoint (RFC-0075).
    pub imed_req: u32,
    /// imed server response endpoint (owned by inputd, the direct-reply peer).
    pub imed_rsp: u32,
    /// imed OSK-injection endpoint (RFC-0075 Phase 2): RECV → imed; SEND
    /// clones below go to execd (app provisioning) + selftest (probe).
    pub imed_osk: u32,
    /// SEND clone of `imed_osk` for execd's `imed-osk` named route.
    pub imed_osk_execd: u32,
    /// SEND clone of `imed_osk` for the selftest harness probe.
    pub imed_osk_selftest: u32,
    /// inputd's settings-watch push channel (RFC-0078; both halves go to
    /// inputd at fixed slots, init's cap closes after wiring).
    pub inputd_watch_ep: u32,
    /// windowd's settings-watch push channel (RFC-0076/0077 region relay;
    /// both halves to windowd at fixed slots, init's cap closes after wiring).
    pub windowd_watch_ep: u32,
    /// windowd server request endpoint.
    pub window_req: u32,
    /// windowd server response endpoint.
    pub window_rsp: u32,
    /// inputd server request endpoint.
    pub input_req: u32,
    /// inputd server response endpoint (owned by hidrawd).
    pub input_rsp: u32,
    /// gpud server request endpoint.
    pub gpud_req: u32,
    /// gpud server response endpoint.
    pub gpud_rsp: u32,
    /// netstackd server request endpoint.
    pub net_req: u32,
    /// netstackd server response endpoint.
    pub net_rsp: u32,
    /// netstackd response endpoint owned by selftest-client.
    pub net_selftest_rsp: u32,
    /// netstackd response endpoint owned by dsoftbusd.
    pub net_dsoft_rsp: u32,
    /// dsoftbusd server request endpoint.
    pub dsoft_req: u32,
    /// dsoftbusd server response endpoint.
    pub dsoft_rsp: u32,
    /// dsoftbusd CAP_MOVE reply inbox.
    pub dsoft_reply_ep: u32,
    /// execd CAP_MOVE reply inbox.
    pub execd_reply_ep: u32,
    /// selftest-client CAP_MOVE reply inbox.
    pub reply_ep: u32,
    /// logd server request endpoint (high fan-in; present only if logd is in the image set).
    pub log_req: Option<u32>,
    /// logd server response endpoint.
    pub log_rsp: Option<u32>,
    /// metricsd server request endpoint (present only if metricsd is in the image set).
    pub metrics_req: Option<u32>,
    /// metricsd server response endpoint.
    pub metrics_rsp: Option<u32>,
    /// sessiond server request endpoint (present only if sessiond is in the
    /// image set). Pre-minted so windowd's session route can be granted long
    /// before sessiond spawns (TASK-0065B ordering).
    pub sess_req: Option<u32>,
    /// sessiond server response endpoint.
    pub sess_rsp: Option<u32>,
    /// abilitymgr server request endpoint (pre-minted so windowd's
    /// launch-request route can be granted — TASK-0080D launch path).
    pub abil_req: Option<u32>,
    /// abilitymgr server response endpoint.
    pub abil_rsp: Option<u32>,
    /// settingsd server request endpoint (pre-minted so the windowd theme
    /// route AND execd's per-app `svc.settings` grants clone the SAME pair
    /// settingsd actually serves — a fresh per-arm pair orphaned both).
    pub sett_req: Option<u32>,
    /// settingsd server response endpoint.
    pub sett_rsp: Option<u32>,
    /// pinched (compute broker) server request endpoint (pre-minted so the
    /// selftest client route clones the SAME pair pinched serves).
    pub pinch_req: Option<u32>,
    /// pinched server response endpoint (owned by selftest-client).
    pub pinch_rsp: Option<u32>,
}

impl Endpoints {
    /// Pre-minted server endpoint pair (request, response) for a service, when
    /// bootstrap minted one. The declarative wire path (RFC-0069) transfers THIS
    /// pair — its client side is already distributed to the service's callers —
    /// instead of creating a fresh endpoint that would orphan those clients.
    /// Grows one entry per migrated service; a service without a minted pair
    /// falls back to a freshly provisioned endpoint.
    pub(crate) fn server_pair(&self, id: crate::service_topology::ServiceId) -> Option<(u32, u32)> {
        use crate::service_topology::ServiceId;
        match id {
            ServiceId::Rngd => Some((self.rng_req, self.rng_rsp)),
            ServiceId::Timed => Some((self.timed_req, self.timed_rsp)),
            ServiceId::Imed => Some((self.imed_req, self.imed_rsp)),
            ServiceId::Vfsd => Some((self.vfs_req, self.vfs_rsp)),
            ServiceId::Packagefsd => Some((self.pkg_req, self.pkg_rsp)),
            ServiceId::Samgrd => Some((self.sam_req, self.sam_rsp)),
            ServiceId::Statefsd => Some((self.state_req, self.state_rsp)),
            // Optional service: pair exists only when logd is in the image set.
            // (When absent, the generic arm falls back to provisioning a fresh —
            // unused — pair; every current image profile includes logd.)
            ServiceId::Logd => self.log_req.zip(self.log_rsp),
            // Still-bespoke arms (task #123 hardening): their pairs are ALSO
            // distributed pre-grants; the bespoke arm skips the transfer when
            // already set and keeps its markers verbatim. Drivers
            // (gpud/windowd/inputd) are priority-wired even earlier; dsoftbusd
            // has no own server pair (its low slots carry netstackd routes).
            ServiceId::Bundlemgrd => Some((self.bnd_req, self.bnd_rsp)),
            ServiceId::Updated => Some((self.upd_req, self.upd_rsp)),
            ServiceId::Keystored => Some((self.key_req, self.key_rsp)),
            ServiceId::Execd => Some((self.exe_req, self.exe_rsp)),
            ServiceId::Netstackd => Some((self.net_req, self.net_rsp)),
            ServiceId::Metricsd => self.metrics_req.zip(self.metrics_rsp),
            // Session authority (TASK-0065B): pre-minted so windowd/abilitymgr
            // client routes exist long before sessiond (spawned last) binds.
            ServiceId::Sessiond => self.sess_req.zip(self.sess_rsp),
            // Launch authority (TASK-0080D): pre-minted so windowd's
            // OP_LAUNCH route targets the pair abilitymgr actually serves.
            ServiceId::Abilitymgr => self.abil_req.zip(self.abil_rsp),
            // Settings authority (TASK-0072 P10 / svc.settings): pre-minted so
            // windowd's theme route and execd's app grants share the served pair.
            ServiceId::Settingsd => self.sett_req.zip(self.sett_rsp),
            // Compute broker (SMP track Phase D): pre-minted so the selftest
            // client route targets the pair pinched actually serves.
            ServiceId::Pinched => self.pinch_req.zip(self.pinch_rsp),
            _ => None,
        }
    }

    /// Pre-minted CAP_MOVE reply-inbox endpoint for a service, when bootstrap
    /// minted a dedicated one. The declarative arm transfers RECV+SEND from it
    /// and closes the init-side slot — the same lifecycle as a freshly created
    /// inbox, just on the endpoint bring-up already made.
    pub(crate) fn minted_reply_ep(&self, id: crate::service_topology::ServiceId) -> Option<u32> {
        use crate::service_topology::ServiceId;
        match id {
            ServiceId::Packagefsd => Some(self.pkg_reply_ep),
            _ => None,
        }
    }
}

/// Closes init's already-wired endpoint slots post-`wire_services` (every
/// leg was granted; a parked full cap table broke runtime `@mint-pair`).
pub(crate) fn close_wired_eps(eps: &Endpoints) {
    for cap in [eps.imed_req, eps.imed_rsp, eps.inputd_watch_ep, eps.windowd_watch_ep] {
        let _ = nexus_abi::cap_close(cap);
    }
}

/// Two SEND clones of the OSK endpoint (RFC-0075 Phase 2) — cloned EARLY
/// (orchestrator, cap-table headroom) because each wiring leg MOVES its
/// cap: the original's RECV goes to imed, these SENDs to execd + selftest.
pub(crate) fn clone_osk_pair(imed_osk: u32) -> Result<(u32, u32), crate::os_payload::InitError> {
    use crate::os_payload::InitError;
    let execd = nexus_abi::cap_clone(imed_osk).map_err(InitError::Abi)?;
    let selftest = nexus_abi::cap_clone(imed_osk).map_err(InitError::Abi)?;
    Ok((execd, selftest))
}
