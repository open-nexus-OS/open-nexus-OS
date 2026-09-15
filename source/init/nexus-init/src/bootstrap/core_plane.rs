// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The CORE control plane + block plane stage (TASK-0321 P4,
//! RFC-0089 §12.3, ADR-0060). Everything the volume spawn pass needs and
//! nothing that needs a volume service: policyd's server pair, control
//! channels (fixed slots 5..8) and priority wiring, bundlemgrd's server
//! pair + block-plane client, virtioblkd's server pair + IRQ endpoint,
//! the deny-by-default MMIO proof, the virtio slot probe and the ONE
//! policy-gated grant the plane needs (`device.mmio.blk` → virtioblkd).
//! Then the volume pass runs — so every later stage (endpoint mints,
//! driver grants, wiring, resume) sees volume-spawned services exactly
//! like embedded ones. The transfers here are the SAME transfers the
//! orchestrator used to make for these three CORE services, in the same
//! per-service order, so their slot layouts are byte-identical.
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU ladder (headless/smp1/reset/ota lanes — the whole
//!   slot-sensitive fleet boots over this stage); `init: timing … volume_ms=`
//! ADR: docs/adr/0060-verified-system-volume-bundlemgrd-verifier-init-spawner.md

use alloc::vec::Vec;
use core::cell::Cell;

use crate::bootstrap::diag::iw;
use crate::bootstrap::helpers::{
    grant_mmio_cap, probe_virtio_mmio_slots, virtio_mmio_window, DEVICE_MMIO_CAP_SLOT,
};
use crate::bootstrap::volume_spawn::VolumeSpawned;
use crate::bootstrap::CtrlChannel;
use crate::os_payload::*;
use crate::service_topology::ServiceId;

/// Accumulated policy-grant wait (the prime "services waiting" suspect in
/// the boot-timing line).
#[derive(Default)]
pub(crate) struct GrantStats {
    pub wait_ns: Cell<u64>,
    pub count: Cell<u32>,
}

/// What the stage hands the orchestrator: the early-minted endpoints (they
/// join the `Endpoints` bundle unchanged), policyd's control request
/// endpoints, the virtio slot probe, the volume-spawned set and the cost
/// of spawning it.
pub(crate) struct CorePlane {
    pub pol_req: u32,
    pub pol_rsp: u32,
    pub bnd_req: u32,
    pub bnd_rsp: u32,
    pub vblk_req: u32,
    pub vblk_rsp: u32,
    pub pol_ctl_route_req: u32,
    pub pol_ctl_exec_req: u32,
    pub net_slot: usize,
    pub rng_slot: usize,
    pub blk_slots: [Option<usize>; 2],
    pub gpu_slot: Option<usize>,
    pub input_slots: [Option<usize>; 3],
    pub volume: Vec<VolumeSpawned>,
    /// Wall time of the volume pass (query → stream → map → exec, all
    /// services) — the boot cost of serving services from the volume.
    pub volume_ms: u64,
    /// init's reply-inbox stash (foreign frames seen during the pass stay
    /// available to the later boot-attempt handshake).
    pub pending: nexus_ipc::reqrep::FrameStash<8, 16>,
}

/// One policy-gated DeviceMmio grant, waited to a 1 s deadline (policyd
/// may still be binding on the first call).
pub(crate) fn grant_mmio_with_wait(
    stats: &GrantStats,
    pol_route: (u32, u32),
    pid: u32,
    svc_name: &str,
    cap_name: &str,
    slot: usize,
    cap_slot: u32,
) -> Result<()> {
    let (mmio_base, mmio_len) = virtio_mmio_window(slot);
    let grant_span = nexus_abi::Span::begin();
    // ONE waited policy exchange (TASK-0324 P8): policyd's verdict or its death — no retry
    // cadence, no clock. `None` is a refused/absent authority: fail-closed, named.
    if grant_mmio_cap(
        pid,
        svc_name,
        cap_name,
        mmio_base,
        mmio_len,
        pol_route.0,
        pol_route.1,
        cap_slot,
    )?
    .is_none()
    {
        return Err(InitError::Map("mmio policy unavailable"));
    }
    stats.wait_ns.set(stats.wait_ns.get().saturating_add(grant_span.elapsed_ns()));
    stats.count.set(stats.count.get().saturating_add(1));
    Ok(())
}

/// Policy negative proof: deny-by-default for a non-matching MMIO
/// capability (a stable, always-present subject and a capability that must
/// not be granted to it). Proves init consults policyd — no local
/// allowlist — and that the marker appears only on a real denial.
fn mmio_policy_deny_probe(pol_route: (u32, u32)) -> Result<()> {
    let subject_id = nexus_abi::service_id_from_name(b"netstackd");
    match policyd_cap_allowed(pol_route.0, pol_route.1, subject_id, b"device.mmio.blk") {
        Some(false) => {
            debug_write_str("init: mmio policy deny ok");
            debug_write_byte(b'\n');
            Ok(())
        }
        Some(true) => Err(InitError::Map("mmio policy deny unexpectedly allowed")),
        None => Err(InitError::Map("mmio policy deny unavailable")),
    }
}

/// A CORE service's pre-minted server pair → its deterministic slots 3/4
/// (the `distribute_server_pair_for` transfer, made here for the three
/// services the plane needs; the bulk pass later skips a pair already set).
fn transfer_server_pair(chan: &mut CtrlChannel, id: ServiceId, req: u32, rsp: u32, vblk_req: u32) {
    // TASK-0324 P4f: all three core-plane services are declared — pinned before they run.
    // TASK-0054C P2-b: their declared timer endpoints too (virtioblkd's device watchdog).
    crate::bootstrap::declared_routes::pin_declared_timers(chan.pid, id);
    if let Some(pair) = crate::bootstrap::declared_slots::pin_server_pair(chan.pid, id, req, rsp) {
        chan.set_send(id, pair.send);
        chan.set_recv(id, pair.recv);
    }
    crate::bootstrap::blk_plane::wire_blk_plane_for_with(chan, vblk_req);
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn bring_up(
    ctrls: &mut Vec<CtrlChannel>,
    selftest_pid: u32,
    policyd_pid: u32,
    bundlemgrd_pid: u32,
    pol_ctl_route_rsp: u32,
    pol_ctl_exec_rsp: u32,
    init_reply_send: u32,
    stats: &GrantStats,
    init_fold: bool,
    init_wire: &mut nexus_event::SpanTally,
    stage_fence: u32,
) -> Result<CorePlane> {
    let mint = |pid: u32, depth: usize| {
        nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, depth)
            .map_err(InitError::Abi)
    };
    let pol_req = mint(policyd_pid, 8)?;
    let pol_rsp = mint(selftest_pid, 8)?;
    let bnd_req = mint(bundlemgrd_pid, 8)?;
    let bnd_rsp = mint(selftest_pid, 8)?;
    // TASK-0315: virtioblkd's blockproto request endpoint (block plane).
    let vblk_req =
        nexus_abi::ipc_endpoint_create_v2(ENDPOINT_FACTORY_CAP_SLOT, 8).map_err(InitError::Abi)?;
    let vblk_rsp =
        nexus_abi::ipc_endpoint_create_v2(ENDPOINT_FACTORY_CAP_SLOT, 8).map_err(InitError::Abi)?;

    // Server pairs (their declared slots) + block-plane wiring for the three CORE
    // services this stage talks to — the same pins the bulk distribution makes.
    for (name, id, req, rsp) in [
        ("policyd", ServiceId::Policyd, pol_req, pol_rsp),
        ("bundlemgrd", ServiceId::Bundlemgrd, bnd_req, bnd_rsp),
        ("virtioblkd", ServiceId::Virtioblkd, vblk_req, vblk_rsp),
    ] {
        if let Some(chan) = ctrls.iter_mut().find(|c| c.svc_name == name) {
            transfer_server_pair(chan, id, req, rsp, vblk_req);
        }
    }

    // Private init-lite <-> policyd check channels: request endpoints are owned by policyd (it
    // receives the queries). Pinned where `slots::policyd` declares them, here in the core plane
    // — BEFORE policyd runs (`resume_plane` below), so none of its own allocations can take them.
    let pol_ctl_route_req = mint(policyd_pid, 8)?;
    let pol_ctl_exec_req = mint(policyd_pid, 8)?;
    {
        use crate::service_topology::NamedSlot;
        for (cap, rights, name) in [
            (pol_ctl_route_req, Rights::RECV, NamedSlot::PolicyRouteCheckRecv),
            (pol_ctl_route_rsp, Rights::SEND, NamedSlot::PolicyRouteCheckSend),
            (pol_ctl_exec_req, Rights::RECV, NamedSlot::PolicyExecCheckRecv),
            (pol_ctl_exec_rsp, Rights::SEND, NamedSlot::PolicyExecCheckSend),
        ] {
            crate::bootstrap::declared_slots::pin_named(
                policyd_pid,
                ServiceId::Policyd,
                name,
                cap,
                rights,
            )
            .ok_or(InitError::Map("policyd check channel"))?;
        }
    }

    // policyd is priority-wired BEFORE any policy-gated grant so policy checks complete in
    // microseconds: its server pair and both check channels are pinned above. The audit inbox
    // (0x9/0xA) and its logd leg (0xB) are pinned by the generic arm from `slots::policyd` —
    // a clone pair once transferred here landed on 9/10 and silently displaced them.
    if let Some(chan) = ctrls.iter_mut().find(|c| c.svc_name == "policyd") {
        debug_assert!(chan.send(ServiceId::Policyd).is_some());
        if iw(init_wire, init_fold, "init:policyd") {
            debug_write_bytes(b"init: policyd priority-wired\n");
        }
    }

    // Wave 0: the three plane services run from here — pairs + control
    // channels are in place, so none of them retries a route probe.
    crate::bootstrap::resume::resume_plane(ctrls);

    let pol_route = (pol_ctl_route_req, pol_ctl_route_rsp);
    mmio_policy_deny_probe(pol_route)?;
    let (net_slot, rng_slot, blk_slots, gpu_slot, input_slots) = probe_virtio_mmio_slots()?;

    // The ONE grant the plane needs: the disk → virtioblkd (ADR-0044 one
    // owner; every other client is a blockproto client). From here the
    // block plane is live and bundlemgrd can attach the measured volume.
    if let Some(virtioblkd_pid) = ctrls.iter().find(|c| c.svc_name == "virtioblkd").map(|c| c.pid) {
        let blk_slot = blk_slots[0].ok_or(InitError::Map("virtio-blk slot not found"))?;
        grant_mmio_with_wait(
            stats,
            pol_route,
            virtioblkd_pid,
            "virtioblkd",
            "device.mmio.blk",
            blk_slot,
            DEVICE_MMIO_CAP_SLOT,
        )?;
    }

    // TASK-0321 (RFC-0089 §12.3, ADR-0060): the SECOND spawn pass — services
    // on the verified system volume, spawned BEFORE any per-pid endpoint
    // mint so the rest of bootstrap treats them exactly like embedded ones.
    let mut pending: nexus_ipc::reqrep::FrameStash<8, 16> = nexus_ipc::reqrep::FrameStash::new();
    let volume_span = nexus_abi::Span::begin();
    let volume = crate::bootstrap::volume_spawn::spawn_volume_services(
        ctrls,
        &mut pending,
        bnd_req,
        init_reply_send,
        pol_ctl_route_rsp,
        init_fold,
        stage_fence,
    )?;
    let volume_ms = volume_span.elapsed_ms();

    Ok(CorePlane {
        pol_req,
        pol_rsp,
        bnd_req,
        bnd_rsp,
        vblk_req,
        vblk_rsp,
        pol_ctl_route_req,
        pol_ctl_exec_req,
        net_slot,
        rng_slot,
        blk_slots,
        gpu_slot,
        input_slots,
        volume,
        volume_ms,
        pending,
    })
}
