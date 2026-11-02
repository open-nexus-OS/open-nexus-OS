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

use crate::bootstrap::helpers::debug_write_bytes;
use crate::bootstrap::CtrlChannel;
use crate::os_payload::ENDPOINT_FACTORY_CAP_SLOT;
use crate::route_table::{CapSlot, RouteTable};
use crate::service_supervision::{supervision_for, DEFAULT_BACKOFF};
use crate::service_topology::ServiceId;
use crate::supervision_engine::{EngineDecision, SupervisedChild};
use nexus_abi::{ExitReason, Rights};

/// Boot-held state the respawn path derives from (never mints beyond it).
pub struct RespawnContext {
    /// ADR-0080: the embedded service images, sealed once at boot — a respawn execs the same
    /// object (its frames shared with the instance that died, had it lived).
    pub sealed: alloc::vec::Vec<crate::bootstrap::spawn::SealedImage>,
    /// TASK-0321 P4: services spawned from the system volume — their sealed images (init keeps
    /// the capability) + launch params; a respawn execs the same object.
    pub volume: alloc::vec::Vec<crate::bootstrap::volume_spawn::VolumeSpawned>,
    /// The pilot client's pid (selftest-client) for SEND re-distribution.
    pub selftest_pid: u32,
    /// init's slot for pinched's RESPONSE endpoint (client-owned, survives).
    pub pinch_rsp_parent_slot: Option<u32>,
    /// init's own statefsd pre-minted pair (restart-counter persistence).
    /// statefsd's request endpoint; the answer rides init's ask inbox.
    pub statefs_send: Option<u32>,
    /// init's own ask inbox (TASK-0054C P2-d/P2-f).
    pub ask: nexus_ipc::SlotPair,
    /// ADR-0062: init's cap for the boot-stage fence. A respawn re-pins it WAIT-only into the
    /// new instance — dropping it would leave a restarted service without the barrier the rest
    /// of the fleet stands on.
    pub stage_fence: u32,
    /// RFC-0112: what windowd's respawn pins its legs from (`None`: no windowd in the image).
    pub windowd: Option<crate::bootstrap::windowd_respawn::WindowdKept>,
}

/// Which services the respawn arm can re-provision: the pilot, and the compositor (RFC-0112).
fn respawnable(id: ServiceId) -> bool {
    matches!(id, ServiceId::Pinched | ServiceId::Windowd)
}

/// Restart driver for real services: one engine per service in a restart cycle (a second death
/// used to take the only engine slot and drop the first one's schedule).
pub(crate) struct Respawner {
    ctx: RespawnContext,
    /// The services in a restart cycle.
    engines: alloc::vec::Vec<(ServiceId, SupervisedChild)>,
    /// Restart-counter persistence over init's statefsd wire (RFC-0087 §2).
    persist: Option<crate::bootstrap::persist::SupervisionPersist>,
}

impl RespawnContext {
    /// Bundles the boot-held respawn inputs (orchestrator hand-off).
    pub(crate) fn new(
        sealed: alloc::vec::Vec<crate::bootstrap::spawn::SealedImage>,
        volume: alloc::vec::Vec<crate::bootstrap::volume_spawn::VolumeSpawned>,
        selftest_pid: u32,
        pinch_rsp_parent_slot: Option<u32>,
        statefs_send: Option<u32>,
        ask: nexus_ipc::SlotPair,
        stage_fence: u32,
        windowd: Option<crate::bootstrap::windowd_respawn::WindowdKept>,
    ) -> Self {
        Self {
            sealed,
            volume,
            selftest_pid,
            pinch_rsp_parent_slot,
            statefs_send,
            ask,
            stage_fence,
            windowd,
        }
    }

    /// The sealed image + launch params to re-exec `name` from: the embedded table or the
    /// volume (ServiceSource, ADR-0060) — never both.
    pub(crate) fn image_for(&self, name: &str) -> Option<(u32, usize, u64, &'static str)> {
        if let Some(img) = self.sealed.iter().find(|img| img.name == name) {
            return Some((img.image, img.stack_pages, img.global_pointer, img.name));
        }
        self.volume
            .iter()
            .find(|v| v.name == name)
            .map(|v| (v.image, v.stack_pages.max(1) as usize, v.global_pointer, v.name))
    }
}

impl Respawner {
    pub(crate) fn new(ctx: RespawnContext) -> Self {
        let persist = crate::bootstrap::persist::SupervisionPersist::new(ctx.statefs_send, ctx.ask);
        Self { ctx, engines: alloc::vec::Vec::new(), persist }
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
        let mut child = match self.engines.iter().position(|(prev, _)| *prev == id) {
            Some(at) => self.engines.swap_remove(at).1,
            None => SupervisedChild::new(restart, DEFAULT_BACKOFF),
        };
        match child.on_exit(reason, code, now_ns()) {
            EngineDecision::RestartAt { .. } => {
                self.engines.push((id, child));
            }
            EngineDecision::Blocked => {
                crate::bootstrap::diag::emit_marker_atomic(
                    &[
                        b"init: crash-loop blocked svc=",
                        id.name().as_bytes(),
                        b" reason=",
                        reason.label().as_bytes(),
                    ],
                    None,
                );
            }
            EngineDecision::Rest => {}
        }
    }

    /// The next scheduled respawn, if any (the responder arms its one-shot timer at it).
    pub(crate) fn next_due_ns(&self) -> Option<u64> {
        self.engines.iter().filter_map(|(_, child)| child.due_ns()).min()
    }

    /// Respawns every service whose scheduled backoff is due (once per responder round).
    pub(crate) fn tick(&mut self, channels: &mut [CtrlChannel], route_table: &mut RouteTable) {
        let now = now_ns();
        for at in 0..self.engines.len() {
            if !self.engines[at].1.restart_due(now) {
                continue;
            }
            let id = self.engines[at].0;
            let persist = self.persist.as_mut();
            let respawned = match id {
                ServiceId::Windowd => self.ctx.windowd.as_ref().and_then(|kept| {
                    crate::bootstrap::windowd_respawn::respawn_windowd(
                        &self.ctx, kept, persist, channels,
                    )
                }),
                _ => respawn_pinched(&self.ctx, persist, channels, route_table),
            };
            // Acked either way, so a permanently failing respawn cannot busy-spin; the
            // paired-marker guard stays the alarm.
            self.engines[at].1.on_restarted();
            match respawned {
                Some(pid) => {
                    route_table.clear_stale(id);
                    crate::bootstrap::diag::emit_marker_atomic(
                        &[b"init: service restarted name=", id.name().as_bytes(), b" pid=0x"],
                        Some(pid as u64),
                    );
                }
                None => crate::bootstrap::diag::emit_marker_atomic(
                    &[b"init: FAIL service respawn name=", id.name().as_bytes()],
                    None,
                ),
            }
        }
    }
}

/// The control plane and the boot-stage fence of a respawned instance: the same init-owned
/// endpoints, re-pinned to the fleet control slots, and the fence WAIT-only (ADR-0062: a respawn
/// that skipped it would make the stage contract hold only until the first restart).
pub(crate) fn reattach_ctrl(pid: u32, (ctrl_req, ctrl_rsp): (u32, u32), stage_fence: u32) -> bool {
    let ctrl_send = nexus_abi::cap_transfer_to_slot(
        pid,
        ctrl_req,
        Rights::SEND,
        crate::service_topology::CTRL_SLOTS.send,
    );
    let ctrl_recv = nexus_abi::cap_transfer_to_slot(
        pid,
        ctrl_rsp,
        Rights::RECV,
        crate::service_topology::CTRL_SLOTS.recv,
    );
    if ctrl_send.is_err() || ctrl_recv.is_err() {
        debug_write_bytes(b"init: FAIL respawn ctrl re-transfer\n");
        return false;
    }
    if nexus_abi::cap_transfer_to_slot(
        pid,
        stage_fence,
        Rights::WAIT,
        crate::service_topology::STAGE_FENCE_SLOT,
    )
    .is_err()
    {
        debug_write_bytes(b"init: FAIL respawn stage fence\n");
        return false;
    }
    true
}

/// Re-provisions pinched exactly like its boot wiring, from boot-held state:
/// exec_v2 (suspended) → ctrl re-transfer into slots 1/2 (init-owned
/// endpoints, unchanged) → NEW request endpoint (the old died with the
/// owner) RECV→slot 3 → surviving response endpoint SEND→slot 4 → client
/// SEND clone + RouteTable update → resume. Returns the new pid.
fn respawn_pinched(
    ctx: &RespawnContext,
    persist: Option<&mut crate::bootstrap::persist::SupervisionPersist>,
    channels: &mut [CtrlChannel],
    route_table: &mut RouteTable,
) -> Option<u32> {
    let (image, stack_pages, global_pointer, name) = ctx.image_for("pinched")?;
    let rsp_parent = ctx.pinch_rsp_parent_slot?;
    let chan = channels.iter_mut().find(|c| c.svc_name == "pinched")?;

    let pid = nexus_abi::exec_vmo(image, stack_pages, global_pointer, name, false).ok()?;
    if !reattach_ctrl(pid, (chan.ctrl_req_parent_slot, chan.ctrl_rsp_parent_slot), ctx.stage_fence)
    {
        return None;
    }

    // New request endpoint owned by the NEW instance, pinned with the surviving response
    // endpoint into pinched's DECLARED server slots — the boot layout by declaration
    // (TASK-0324 P4f-1a), where it used to be "first free after ctrl 1/2", checked afterwards.
    let req = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, 8).ok()?;
    if crate::bootstrap::declared_slots::pin_server_pair(pid, ServiceId::Pinched, req, rsp_parent)
        .is_none()
    {
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

    // RFC-0087 §2: the restart counter is durable BEFORE anyone can observe
    // the restarted instance (strict PUT+SYNC → resume ordering keeps the
    // shared statefs response queue race-free for readers).
    if let Some(persist) = persist {
        persist.bump_restart_count(name);
    }

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
