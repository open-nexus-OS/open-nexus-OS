// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Settings-watch routes (RFC-0078): inputd's and windowd's watch
//! legs to settingsd — split out of `route_provision.rs` (module-size
//! ratchet). Same contract, same markers.
//! OWNERS: @runtime
//! STATUS: Experimental

use crate::bootstrap::endpoints::Endpoints;
use crate::bootstrap::helpers::debug_write_bytes;
use crate::bootstrap::route_provision::{
    INPUTD_SETTINGS_SEND_SLOT, INPUTD_WATCH_RECV_SLOT, INPUTD_WATCH_SEND_SLOT,
};
use crate::bootstrap::CtrlChannel;
use crate::service_topology::ServiceId;
use nexus_abi::Rights;

/// Provisions inputd's settings-watch channel (RFC-0078): a fresh minted
/// endpoint (both halves to inputd at FIXED slots — mint→grant→close, zero
/// init cap-table accumulation) + SEND on settingsd's request endpoint.
/// Best-effort: without it, live keymap switching stays inert (honest
/// failure; the boot keymap default applies).
pub(crate) fn provision_inputd_settings_watch(pid: u32, eps: &Endpoints, chan: &mut CtrlChannel) {
    let Some((settings_req, _)) = eps.server_pair(ServiceId::Settingsd) else {
        return;
    };
    let ok =
        nexus_abi::cap_transfer_to_slot(pid, settings_req, Rights::SEND, INPUTD_SETTINGS_SEND_SLOT)
            .is_ok();
    // Pre-minted in the orchestrator (init's cap table is at its ceiling by
    // wiring time — a late mint NoSpace-fails); init's cap closes after wiring.
    let ep = eps.inputd_watch_ep;
    let recv_ok =
        nexus_abi::cap_transfer_to_slot(pid, ep, Rights::RECV, INPUTD_WATCH_RECV_SLOT).is_ok();
    let send_ok =
        nexus_abi::cap_transfer_to_slot(pid, ep, Rights::SEND, INPUTD_WATCH_SEND_SLOT).is_ok();
    if ok && recv_ok && send_ok {
        chan.set_send(ServiceId::Settingsd, INPUTD_SETTINGS_SEND_SLOT);
        if crate::bootstrap::diag::raw_or_expanded("inputd") {
            debug_write_bytes(b"init: inputd settings-watch ok\\n");
        }
    } else {
        debug_write_bytes(b"init: inputd settings-watch FAIL (xfer)\n");
    }
}

/// Fixed windowd slots for its settings-watch channel (RFC-0076/0077 —
/// windowd relays region data to surfaces). Kept in sync with
/// `windowd/src/compositor/runtime/region.rs`.
const WINDOWD_WATCH_RECV_SLOT: u32 = 0x40;
const WINDOWD_WATCH_SEND_SLOT: u32 = 0x41;

/// Provisions windowd's settings-watch channel (pre-minted; both halves to
/// fixed slots; windowd's settingsd SEND route already exists via
/// `provision_windowd_settings_route`). Best-effort.
pub(crate) fn provision_windowd_settings_watch(pid: u32, eps: &Endpoints, chan: &mut CtrlChannel) {
    let _ = chan;
    // RFC-0083 NOTE (measured, kernel follow-up recorded in TASK-0307): the
    // watch channel stays a DEDICATED side endpoint. Pointing slot 0x41 at
    // windowd's own service endpoint (so settings events would arrive
    // in-band and wake an idle compositor) wedges the whole system
    // deterministically once settingsd starts sending on the moved clones —
    // the Idle-class selftest ladder starves and never runs (boots
    // 2026-07-27T11-31-56 / 13-32-13; bisected against 13-38-26). Cloning
    // windowd's own server slots instead is PermissionDenied (also
    // measured). Until the kernel semantics of foreign-held send caps on
    // service-pair endpoints are understood, windowd drains this side
    // channel per frame + a bounded idle tick.
    let recv_ok = nexus_abi::cap_transfer_to_slot(
        pid,
        eps.windowd_watch_ep,
        Rights::RECV,
        WINDOWD_WATCH_RECV_SLOT,
    )
    .is_ok();
    let send_ok = nexus_abi::cap_transfer_to_slot(
        pid,
        eps.windowd_watch_ep,
        Rights::SEND,
        WINDOWD_WATCH_SEND_SLOT,
    )
    .is_ok();
    if recv_ok && send_ok {
        if crate::bootstrap::diag::raw_or_expanded("windowd") {
            debug_write_bytes(b"init: windowd settings-watch ok\\n");
        }
    } else {
        debug_write_bytes(b"init: windowd settings-watch FAIL (xfer)\n");
    }
}
