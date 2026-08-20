// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Real-service respawn (TASK-0049B PR-B3b, ADR-0057). Pilot:
//! `pinched` — the minimal declarative service (no routes, no reply inbox,
//! no device). The whole respawn is a DERIVATION from boot-held state, never
//! new authority (no-rights-drift): the service ELF is `&'static` boot
//! image bytes, the ctrl endpoints are init-owned and survive the death
//! (only re-transferred into the new pid's slots 1/2), the response
//! endpoint is client-owned and survives, and only the request endpoint —
//! which died WITH the service (owner-bound) — is re-minted via init's
//! EndpointFactory and re-distributed: RECV to the new instance (slot 3),
//! a SEND clone to its client, and the RouteTable entry updated so the
//! client's STALE re-resolve returns the fresh slots. Widening beyond the
//! pilot = extending `respawnable()` + the per-service re-provision arm.
//! OWNERS: @runtime @reliability
//! STATUS: Functional (pinched pilot)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU E2E (`SELFTEST: service restart ok`, every boot) +
//!   supervision_engine host tests (the decision logic underneath).
//! ADR: docs/adr/0057-service-restart-capability-re-resolve.md

use crate::bootstrap::helpers::{debug_write_byte, debug_write_bytes, debug_write_hex};
use crate::bootstrap::CtrlChannel;
use crate::os_payload::{ServiceImage, ENDPOINT_FACTORY_CAP_SLOT};
use crate::route_table::{CapSlot, RouteTable};
use crate::service_supervision::{supervision_for, DEFAULT_BACKOFF};
use crate::service_topology::ServiceId;
use crate::supervision_engine::{EngineDecision, SupervisedChild};
use nexus_abi::{ExitReason, Rights};

/// Boot-held state the respawn path derives from (never mints beyond it).
pub struct RespawnContext {
    /// The `'static` boot service images (ELF bytes live forever).
    pub images: &'static [ServiceImage],
    /// The pilot client's pid (selftest-client) for SEND re-distribution.
    pub selftest_pid: u32,
    /// init's slot for pinched's RESPONSE endpoint (client-owned, survives).
    pub pinch_rsp_parent_slot: Option<u32>,
}

/// Which services the respawn arm can actually re-provision today.
fn respawnable(id: ServiceId) -> bool {
    matches!(id, ServiceId::Pinched)
}

/// Restart driver for real services (pilot: one engine slot — widened with
/// `respawnable`).
pub(crate) struct Respawner {
    ctx: RespawnContext,
    /// The service currently in a restart cycle, if any.
    engine: Option<(ServiceId, SupervisedChild)>,
}

impl RespawnContext {
    /// Bundles the boot-held respawn inputs (orchestrator hand-off).
    pub(crate) fn new(
        images: &'static [ServiceImage],
        selftest_pid: u32,
        pinch_rsp_parent_slot: Option<u32>,
    ) -> Self {
        Self { images, selftest_pid, pinch_rsp_parent_slot }
    }
}

impl Respawner {
    pub(crate) fn new(ctx: RespawnContext) -> Self {
        Self { ctx, engine: None }
    }

    /// A supervised service died (already announced + marked stale by the
    /// sweep). Feed the SSOT policy through the engine.
    pub(crate) fn on_service_exit(&mut self, id: ServiceId, reason: ExitReason, code: i32) {
        if !respawnable(id) {
            return;
        }
        let Some((_criticality, restart)) = supervision_for(id) else {
            return;
        };
        let mut child = match self.engine.take() {
            Some((prev, child)) if prev == id => child,
            _ => SupervisedChild::new(restart, DEFAULT_BACKOFF),
        };
        match child.on_exit(reason, code, now_ns()) {
            EngineDecision::RestartAt { .. } => {
                self.engine = Some((id, child));
            }
            EngineDecision::Blocked => {
                debug_write_bytes(b"init: crash-loop blocked svc=");
                debug_write_bytes(id.name().as_bytes());
                debug_write_bytes(b" reason=");
                debug_write_bytes(reason.label().as_bytes());
                debug_write_byte(b'\n');
            }
            EngineDecision::Rest => {}
        }
    }

    /// Respawn when the scheduled backoff is due (once per responder round).
    pub(crate) fn tick(&mut self, channels: &mut [CtrlChannel], route_table: &mut RouteTable) {
        let due = matches!(&self.engine, Some((_, child)) if child.restart_due(now_ns()));
        if !due {
            return;
        }
        let Some((id, mut child)) = self.engine.take() else {
            return;
        };
        match respawn_pinched(&self.ctx, channels, route_table) {
            Some(pid) => {
                child.on_restarted();
                self.engine = Some((id, child));
                route_table.clear_stale(id);
                debug_write_bytes(b"init: service restarted name=");
                debug_write_bytes(id.name().as_bytes());
                debug_write_bytes(b" pid=0x");
                debug_write_hex(pid as usize);
                debug_write_byte(b'\n');
            }
            None => {
                // Ack the schedule so a permanently failing respawn cannot
                // busy-spin; the paired-marker guard stays the alarm.
                child.on_restarted();
                self.engine = Some((id, child));
                debug_write_bytes(b"init: FAIL service respawn name=");
                debug_write_bytes(id.name().as_bytes());
                debug_write_byte(b'\n');
            }
        }
    }
}

/// Re-provisions pinched exactly like its boot wiring, from boot-held state:
/// exec_v2 (suspended) → ctrl re-transfer into slots 1/2 (init-owned
/// endpoints, unchanged) → NEW request endpoint (the old died with the
/// owner) RECV→slot 3 → surviving response endpoint SEND→slot 4 → client
/// SEND clone + RouteTable update → resume. Returns the new pid.
fn respawn_pinched(
    ctx: &RespawnContext,
    channels: &mut [CtrlChannel],
    route_table: &mut RouteTable,
) -> Option<u32> {
    let image = ctx.images.iter().find(|img| img.name == "pinched")?;
    let rsp_parent = ctx.pinch_rsp_parent_slot?;
    let chan = channels.iter_mut().find(|c| c.svc_name == "pinched")?;

    let pid =
        nexus_abi::exec_v2(image.elf, image.stack_pages as usize, image.global_pointer, image.name)
            .ok()?;

    // Ctrl plane: same init-owned endpoints, re-transferred to the fixed
    // child slots (1/2) the routing client expects.
    const CTRL_CHILD_SEND_SLOT: u32 = 1;
    const CTRL_CHILD_RECV_SLOT: u32 = 2;
    let ctrl_send = nexus_abi::cap_transfer_to_slot(
        pid,
        chan.ctrl_req_parent_slot,
        Rights::SEND,
        CTRL_CHILD_SEND_SLOT,
    );
    let ctrl_recv = nexus_abi::cap_transfer_to_slot(
        pid,
        chan.ctrl_rsp_parent_slot,
        Rights::RECV,
        CTRL_CHILD_RECV_SLOT,
    );
    if ctrl_send.is_err() || ctrl_recv.is_err() {
        debug_write_bytes(b"init: FAIL respawn ctrl re-transfer\n");
        return None;
    }

    // New request endpoint owned by the NEW instance; RECV lands at the
    // deterministic server slot 3 (first free after ctrl 1/2), the surviving
    // response endpoint's SEND at 4 — byte-identical to the boot layout.
    let req = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, 8).ok()?;
    let recv_slot = nexus_abi::cap_transfer(pid, req, Rights::RECV).ok()?;
    let send_slot = nexus_abi::cap_transfer(pid, rsp_parent, Rights::SEND).ok()?;
    if recv_slot != 3 || send_slot != 4 {
        debug_write_bytes(b"init: FAIL respawn server slots\n");
        let _ = nexus_abi::cap_close(req);
        return None;
    }

    // Client re-distribution: a fresh SEND clone into the pilot client; its
    // surviving RECV slot is carried over from the old route entry.
    let old_recv = route_table
        .lookup(ServiceId::SelftestClient, ServiceId::Pinched)
        .map(|route| route.recv)?;
    let client_send = nexus_abi::cap_transfer(ctx.selftest_pid, req, Rights::SEND).ok()?;
    route_table.add_route(
        ServiceId::SelftestClient,
        ServiceId::Pinched,
        CapSlot::new(client_send, Rights::SEND),
        old_recv,
    );
    // init's own handle to the new endpoint is not needed further (the next
    // respawn mints again); close it so respawns never accumulate caps.
    let _ = nexus_abi::cap_close(req);

    if nexus_abi::task_resume(pid).is_err() {
        debug_write_bytes(b"init: FAIL respawn resume\n");
        return None;
    }
    chan.pid = pid;
    Some(pid)
}

/// Monotonic now; `nsec` is the boot-proven time source on the OS path.
fn now_ns() -> u64 {
    nexus_abi::nsec().unwrap_or(0)
}
