// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: inputd's and imed's client-route provisioning helpers (split out of `wiring.rs`,
//! structure-gate): inputd's imed key-forward leg and reply inbox, imed's legs, the RTC grant.
//! windowd's legs are its declaration (`windowd_respawn::wire_windowd_legs`, RFC-0112): the
//! per-leg helpers it had here are gone.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: nexus-init host tests + QEMU boot ladder.

use crate::bootstrap::endpoints::Endpoints;
use crate::bootstrap::helpers::debug_write_bytes;
use crate::bootstrap::CtrlChannel;
use crate::service_topology::ServiceId;
use nexus_abi::Rights;

/// Provisions inputd's key-forward route (RFC-0075): SEND on imed's request
/// endpoint + RECV on its response endpoint (fire-and-forget pushes; the
/// recv side answers name-route lookups). Best-effort.
pub(crate) fn provision_inputd_imed_route(pid: u32, eps: &Endpoints, chan: &mut CtrlChannel) {
    let Some((imed_req, imed_rsp)) = eps.server_pair(ServiceId::Imed) else {
        return;
    };
    match (
        crate::bootstrap::declared_slots::pin_route_send(
            pid,
            ServiceId::Inputd,
            ServiceId::Imed,
            imed_req,
        ),
        crate::bootstrap::declared_slots::pin_route_recv(
            pid,
            ServiceId::Inputd,
            ServiceId::Imed,
            imed_rsp,
        ),
    ) {
        (Some(s), Some(r)) => {
            chan.set_send(ServiceId::Imed, s);
            chan.set_recv(ServiceId::Imed, r);
            if crate::bootstrap::diag::raw_or_expanded("inputd") {
                debug_write_bytes(b"init: inputd route->imed ok\n");
            }
        }
        _ => debug_write_bytes(b"init: inputd route->imed FAIL (xfer)\n"),
    }
}

/// inputd's declared reply inbox (RFC-0098 C7): windowd answers inputd's ONE display-space
/// call there — never on windowd's shared response endpoint, which several services read.
/// inputd is priority-wired, so the generic declared-legs arm that mints inboxes never runs for
/// it; this is that arm's inbox step, pinned where `slots::inputd::REPLY` declares it.
pub(crate) fn provision_inputd_reply_inbox(pid: u32, chan: &mut CtrlChannel) {
    let factory = crate::os_payload::ENDPOINT_FACTORY_CAP_SLOT;
    let Ok(reply_ep) = nexus_abi::ipc_endpoint_create_for(factory, pid, 8) else {
        debug_write_bytes(b"init: inputd reply inbox FAIL (create)\n");
        return;
    };
    let pinned =
        crate::bootstrap::declared_slots::pin_reply_inbox(pid, ServiceId::Inputd, reply_ep);
    let _ = nexus_abi::cap_close(reply_ep);
    match pinned {
        Some(inbox) => {
            chan.reply_recv_slot = Some(inbox.recv);
            chan.reply_send_slot = Some(inbox.send);
            if crate::bootstrap::diag::raw_or_expanded("inputd") {
                debug_write_bytes(b"init: inputd reply inbox ok\n");
            }
        }
        None => debug_write_bytes(b"init: inputd reply inbox FAIL (pin)\n"),
    }
}

/// RFC-0076: policy-gated grant of the RTC MMIO window (the node the tree lists by
/// compatible, RFC-0098 C3) to timed — the time authority reads its own wall-clock
/// anchor. ONE waited policy exchange (TASK-0324 P8): a denied/failed/unanswered grant,
/// or a tree without an RTC, leaves walltime honestly UNAVAILABLE, never fatal.
pub(crate) fn grant_rtc_mmio_to_timed(
    timed_pid: u32,
    pol_ctl_route_req: u32,
    pol_ctl_route_rsp: u32,
) -> crate::os_payload::Result<()> {
    use crate::os_payload::{grant_mmio_cap, DEVICE_MMIO_CAP_SLOT};
    let Some(rtc) = crate::bootstrap::device_tree::rtc() else {
        debug_write_bytes(b"init: rtc not in the device tree (walltime unavailable)\n");
        return Ok(());
    };
    if grant_mmio_cap(
        timed_pid,
        "timed",
        "device.mmio.rtc",
        rtc,
        pol_ctl_route_req,
        pol_ctl_route_rsp,
        DEVICE_MMIO_CAP_SLOT,
    )?
    .is_none()
    {
        debug_write_bytes(b"init: rtc mmio grant unavailable\n");
    }
    Ok(())
}

/// imed's two legs the generic route loop cannot find a target endpoint for: the OSK endpoint's
/// RECV half (a dedicated endpoint, not a server pair) and the windowd push route (windowd is
/// priority-wired, so it has no entry in the minted-pair table). Both are pinned where
/// `slots::imed` declares them (TASK-0324 P4f-1b); the settingsd and statefsd legs this function
/// used to hand-build — pinned literals, a `cap_clone` per leg that was never closed — are
/// declared `PrivateInbox` routes provisioned by the generic arm.
pub(crate) fn provision_imed_legs(
    pid: u32,
    imed_osk: u32,
    window_req: u32,
    window_rsp: u32,
    chan: &mut CtrlChannel,
) {
    use crate::service_topology::NamedSlot;
    match crate::bootstrap::declared_slots::pin_named(
        pid,
        ServiceId::Imed,
        NamedSlot::OskServerRecv,
        imed_osk,
        Rights::RECV,
    ) {
        Some(_) => debug_write_bytes(b"init: imed osk recv ok\n"),
        None => debug_write_bytes(b"init: imed osk recv FAIL (xfer)\n"),
    }
    match crate::bootstrap::declared_slots::pin_route(
        pid,
        ServiceId::Imed,
        ServiceId::Windowd,
        window_req,
        window_rsp,
    ) {
        (Some(send), Some(recv)) => {
            chan.set_send(ServiceId::Windowd, send);
            chan.set_recv(ServiceId::Windowd, recv);
            if crate::bootstrap::diag::raw_or_expanded("imed") {
                debug_write_bytes(b"init: imed route->windowd ok\n");
            }
        }
        _ => debug_write_bytes(b"init: imed route->windowd FAIL (xfer)\n"),
    }
}
