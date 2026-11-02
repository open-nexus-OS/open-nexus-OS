// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: inputd's settings route (RFC-0078) — its SEND to settingsd, over which it
//! subscribes its watch channel. The watch channels themselves (inputd's, windowd's settings and
//! session pushes) are declared waits, minted and pinned before the service runs
//! (`declared_routes::pin_declared_waits`) — the same path windowd's respawn takes (RFC-0112).
//! OWNERS: @runtime
//! STATUS: Experimental

use crate::bootstrap::endpoints::Endpoints;
use crate::bootstrap::helpers::debug_write_bytes;
use crate::bootstrap::CtrlChannel;
use crate::service_topology::ServiceId;
use nexus_abi::Rights;

/// Provisions inputd's settings route (RFC-0078): SEND on settingsd's request endpoint, over
/// which inputd subscribes its watch channel — the channel itself is a declared wait, minted and
/// pinned with the others before inputd runs (`declared_routes::pin_declared_waits`).
/// Best-effort: without it, live keymap switching stays inert (honest failure; the boot keymap
/// default applies).
pub(crate) fn provision_inputd_settings_route(pid: u32, eps: &Endpoints, chan: &mut CtrlChannel) {
    let Some((settings_req, _)) = eps.server_pair(ServiceId::Settingsd) else {
        return;
    };
    let settings_send = crate::bootstrap::declared_slots::pin_named(
        pid,
        ServiceId::Inputd,
        crate::service_topology::NamedSlot::Settings,
        settings_req,
        Rights::SEND,
    );
    if let Some(settings_send) = settings_send {
        chan.set_send(ServiceId::Settingsd, settings_send);
        if crate::bootstrap::diag::raw_or_expanded("inputd") {
            debug_write_bytes(b"init: inputd settings-watch ok\n");
        }
    } else {
        debug_write_bytes(b"init: inputd settings-watch FAIL (xfer)\n");
    }
}
