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
}

impl Endpoints {
    /// Pre-minted server endpoint pair (request, response) for a service, when
    /// bootstrap minted one. The declarative wire path (RFC-0069) transfers THIS
    /// pair — its client side is already distributed to the service's callers —
    /// instead of creating a fresh endpoint that would orphan those clients.
    /// Grows one entry per migrated service; a service without a minted pair
    /// falls back to a freshly provisioned endpoint.
    pub(crate) fn server_pair(
        &self,
        id: crate::service_topology::ServiceId,
    ) -> Option<(u32, u32)> {
        use crate::service_topology::ServiceId;
        match id {
            ServiceId::Rngd => Some((self.rng_req, self.rng_rsp)),
            ServiceId::Timed => Some((self.timed_req, self.timed_rsp)),
            _ => None,
        }
    }
}
