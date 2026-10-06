// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The CORE control plane + block plane stage (TASK-0321 P4,
//! RFC-0089 §12.3, ADR-0060). Everything the volume spawn pass needs and
//! nothing that needs a volume service: policyd's server pair, control
//! channels (fixed slots 5..8) and priority wiring, bundlemgrd's server
//! pair + block-plane client, blkd's server pair + IRQ endpoint,
//! the deny-by-default MMIO proof, device discovery from the tree and the ONE
//! policy-gated grant the plane needs: the disk the loader booted from → blkd, asked
//! for its kind's class (TASK-0246 P4b) — made while only policyd runs, so blkd starts
//! with its disk in place.
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

use crate::bootstrap::device_tree::{self, DeviceWindow, VirtioDevices};
use crate::bootstrap::diag::iw;
use crate::bootstrap::helpers::{grant_mmio_cap, DEVICE_MMIO_CAP_SLOT};
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
/// endpoints, the devices the tree lists, the volume-spawned set and the cost
/// of spawning it.
pub(crate) struct CorePlane {
    pub pol_req: u32,
    pub pol_rsp: u32,
    pub bnd_req: u32,
    pub bnd_rsp: u32,
    pub blk_req: u32,
    pub blk_rsp: u32,
    /// socd's server pair (TASK-0246 P4c: minted here — blkd asks it before the volume pass).
    pub soc_req: u32,
    pub soc_rsp: u32,
    pub pol_ctl_route_req: u32,
    pub pol_ctl_exec_req: u32,
    /// The virtio transports the tree lists, classified (RFC-0098 C3).
    pub devices: VirtioDevices,
    /// The USB host controller (TASK-0328 U1), if the machine has one.
    pub usb: Option<UsbPlane>,
    pub volume: Vec<VolumeSpawned>,
    /// Wall time of the volume pass (query → stream → map → exec, all
    /// services) — the boot cost of serving services from the volume.
    pub volume_ms: u64,
}

/// The USB host controller (TASK-0328 U1): QEMU's PCI xHCI or the board's tree node — the
/// window xhcid is granted and, for a PCI function, what that grant turns bus mastering on for.
#[derive(Clone, Copy)]
pub(crate) struct UsbPlane {
    pub window: DeviceWindow,
    pub pci: Option<crate::bootstrap::pci::PciFunction>,
    /// The USB 2.0 PHY's window (the board's tree names it; a PCI controller has none).
    pub phy: Option<DeviceWindow>,
    /// The SuperSpeed (combo) PHY's window, read for a measurement (v1 leaves the port alone).
    pub ss_phy: Option<DeviceWindow>,
}

/// One policy-gated DeviceMmio grant, waited to a 1 s deadline (policyd
/// may still be binding on the first call).
pub(crate) fn grant_mmio_with_wait(
    stats: &GrantStats,
    pol_route: (u32, u32),
    pid: u32,
    svc_name: &str,
    cap_name: &str,
    dev: DeviceWindow,
    cap_slot: u32,
) -> Result<()> {
    grant_mmio_waited(stats, pol_route, pid, svc_name, cap_name, dev, cap_slot).map(|_| ())
}

/// [`grant_mmio_with_wait`] that also says whether policyd granted it (`false` = denied, named
/// by the grant itself).
#[allow(clippy::too_many_arguments)]
fn grant_mmio_waited(
    stats: &GrantStats,
    pol_route: (u32, u32),
    pid: u32,
    svc_name: &str,
    cap_name: &str,
    dev: DeviceWindow,
    cap_slot: u32,
) -> Result<bool> {
    let grant_span = nexus_abi::Span::begin();
    // ONE waited policy exchange (TASK-0324 P8): policyd's verdict or its death — no retry
    // cadence, no clock. `None` is a refused/absent authority: fail-closed, named. `false` is
    // policyd's denial (named by the grant itself).
    let Some(granted) =
        grant_mmio_cap(pid, svc_name, cap_name, dev, pol_route.0, pol_route.1, cap_slot)?
    else {
        return Err(InitError::Map("mmio policy unavailable"));
    };
    stats.wait_ns.set(stats.wait_ns.get().saturating_add(grant_span.elapsed_ns()));
    stats.count.set(stats.count.get().saturating_add(1));
    Ok(granted)
}

/// The board's display plane to gpud (TASK-0251 P2): the controller's and the encoder's windows
/// in gpud's declared slots, each a policy-checked `device.mmio.display` grant carrying the
/// node's line, coherence and DMA reach. A tree without the pair says so; the GPU's window is
/// then the plane (`init: gpu plane …`).
pub(crate) fn grant_display_plane(
    stats: &GrantStats,
    pol_route: (u32, u32),
    gpud_pid: u32,
) -> Result<()> {
    use crate::service_topology::slots::gpud;
    let plane = device_tree::display_plane();
    let (Some(controller), Some(encoder)) = (plane.controller, plane.encoder) else {
        debug_write_str(match (plane.controller, plane.encoder) {
            (None, None) => "init: display plane none (no device in the tree)\n",
            _ => "init: display plane none (the tree names one of controller and encoder)\n",
        });
        return Ok(());
    };
    let class = "device.mmio.display";
    let c = grant_mmio_waited(
        stats,
        pol_route,
        gpud_pid,
        "gpud",
        class,
        controller,
        gpud::DISPLAY_CONTROLLER,
    )?;
    let e = grant_mmio_waited(
        stats,
        pol_route,
        gpud_pid,
        "gpud",
        class,
        encoder,
        gpud::DISPLAY_ENCODER,
    )?;
    if c && e {
        debug_write_str("init: display plane ok (controller + encoder to gpud)\n");
    }
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

/// The endpoints that exist during the core plane — policyd's and socd's server pairs — for the
/// legs of the services that talk before init's responder serves (TASK-0246 P4c: socd asks
/// policyd, blkd asks socd).
struct CoreEndpoints {
    pol: (u32, u32),
    soc: (u32, u32),
}

impl crate::bootstrap::declared_routes::LegEndpoints for CoreEndpoints {
    fn minted_reply_ep(&self, _: ServiceId) -> Option<u32> {
        None
    }
    fn server_pair(&self, id: ServiceId) -> Option<(u32, u32)> {
        match id {
            ServiceId::Policyd => Some(self.pol),
            ServiceId::Socd => Some(self.soc),
            _ => None,
        }
    }
    fn request_ep(&self, _: ServiceId, to: ServiceId) -> Option<u32> {
        self.server_pair(to).map(|(req, _)| req)
    }
}

/// A CORE service's pre-minted server pair → its deterministic slots 3/4
/// (the `distribute_server_pair_for` transfer, made here for the three
/// services the plane needs; the bulk pass later skips a pair already set).
fn transfer_server_pair(chan: &mut CtrlChannel, id: ServiceId, req: u32, rsp: u32, blk_req: u32) {
    // TASK-0324 P4f: all three core-plane services are declared — pinned before they run.
    // TASK-0054C P2-b, TASK-0328 U1: their declared wait endpoints too (blkd's device watchdog
    // and interrupt notify endpoint).
    crate::bootstrap::declared_routes::pin_declared_waits(chan.pid, id);
    if let Some(pair) = crate::bootstrap::declared_slots::pin_server_pair(chan.pid, id, req, rsp) {
        chan.set_send(id, pair.send);
        chan.set_recv(id, pair.recv);
    }
    crate::bootstrap::blk_plane::wire_blk_plane_for_with(chan, blk_req);
}

/// The boot disk → blkd (TASK-0246 P4b): the tree its record is read from, the grant for the
/// disk's kind, bus mastering for a function behind PCI after its grant, one line. A record
/// that names no disk init may grant is refused by name and ends the boot.
fn grant_boot_disk(
    stats: &GrantStats,
    pol_route: (u32, u32),
    blkd_pid: u32,
    (devices, pci): (&VirtioDevices, &crate::bootstrap::pci::PciDevices),
    init_wire: &mut nexus_event::SpanTally,
    init_fold: bool,
) -> Result<()> {
    use crate::bootstrap::boot_disk;
    let disk = boot_disk::resolve(devices, pci).map_err(|reason| {
        boot_disk::refused(reason);
        InitError::Map("boot disk refused")
    })?;
    let pinned = crate::bootstrap::declared_slots::pin_named(
        blkd_pid,
        ServiceId::Blkd,
        crate::service_topology::NamedSlot::DeviceTree,
        nexus_abi::INIT_DEVICE_TREE_SLOT,
        Rights::MAP,
    );
    if pinned.is_some() && iw(init_wire, init_fold, "init:blkd") {
        debug_write_bytes(b"init: device tree grant ok svc=blkd\n");
    }
    // RFC-0107 Phase 2: the kernel console ring, read-only, to its one reader — the block owner
    // keeps it in the boot trace. A kernel without the ring leaves the slot empty; blkd then
    // keeps no OS text and says so.
    let ring = crate::bootstrap::declared_slots::pin_named(
        blkd_pid,
        ServiceId::Blkd,
        crate::service_topology::NamedSlot::ConsoleRing,
        nexus_abi::INIT_CONSOLE_RING_SLOT,
        Rights::MAP,
    );
    if ring.is_some() && iw(init_wire, init_fold, "init:blkd") {
        debug_write_bytes(b"init: console ring grant ok svc=blkd\n");
    }
    grant_mmio_with_wait(
        stats,
        pol_route,
        blkd_pid,
        "blkd",
        disk.kind.policy_class(),
        disk.window,
        DEVICE_MMIO_CAP_SLOT,
    )?;
    if let Some(sd) = disk.pci {
        crate::bootstrap::pci::enable_bus_master(sd.function()).map_err(|reason| {
            boot_disk::refused(reason);
            InitError::Map("bus mastering")
        })?;
    }
    boot_disk::report(&disk);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn bring_up(
    ctrls: &mut Vec<CtrlChannel>,
    selftest_pid: u32,
    policyd_pid: u32,
    bundlemgrd_pid: u32,
    pol_ctl_route_rsp: u32,
    pol_ctl_exec_rsp: u32,
    ask: nexus_ipc::SlotPair,
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
    // TASK-0315: blkd's blockproto request endpoint (block plane).
    let blk_req =
        nexus_abi::ipc_endpoint_create_v2(ENDPOINT_FACTORY_CAP_SLOT, 8).map_err(InitError::Abi)?;
    let blk_rsp =
        nexus_abi::ipc_endpoint_create_v2(ENDPOINT_FACTORY_CAP_SLOT, 8).map_err(InitError::Abi)?;
    // TASK-0246 P4c: the SoC glue owner's pair — its request endpoint socd's, its response
    // endpoint the harness's (RFC-0106).
    let socd_pid =
        ctrls.iter().find(|c| c.svc_name == "socd").map(|c| c.pid).ok_or(InitError::MissingElf)?;
    let soc_req = mint(socd_pid, 8)?;
    let soc_rsp = mint(selftest_pid, 8)?;

    // Server pairs (their declared slots) + block-plane wiring for the four CORE
    // services this stage talks to — the same pins the bulk distribution makes.
    for (name, id, req, rsp) in [
        ("policyd", ServiceId::Policyd, pol_req, pol_rsp),
        ("socd", ServiceId::Socd, soc_req, soc_rsp),
        ("bundlemgrd", ServiceId::Bundlemgrd, bnd_req, bnd_rsp),
        ("blkd", ServiceId::Blkd, blk_req, blk_rsp),
    ] {
        if let Some(chan) = ctrls.iter_mut().find(|c| c.svc_name == name) {
            transfer_server_pair(chan, id, req, rsp, blk_req);
        }
    }

    // TASK-0246 P4c: socd and blkd talk before init's responder serves — socd asks policyd
    // whether a requester holds `soc.glue`, blkd asks socd to bring its disk's node up. Their
    // declared legs are pinned now, from the endpoints that exist, before either runs (the later
    // generic run keeps them and adds what was missing: socd's logd leg).
    let core_eps = CoreEndpoints { pol: (pol_req, pol_rsp), soc: (soc_req, soc_rsp) };
    for name in ["socd", "blkd"] {
        let Some(spec) = crate::service_topology::spec_for(name.as_bytes()) else { continue };
        if let Some(chan) = ctrls.iter_mut().find(|c| c.svc_name == name) {
            let pid = chan.pid;
            crate::bootstrap::declared_routes::wire_declared_legs(pid, spec, &core_eps, chan);
        }
    }

    // Private init-lite <-> policyd check channels: request endpoints are owned by policyd (it
    // receives the queries). Pinned where `slots::policyd` declares them, here in the core plane
    // — BEFORE policyd runs (`resume_authority` below), so none of its own allocations can take them.
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

    // Wave 0a: the policy authority runs first — pairs + control channels are in place, so it
    // retries no route probe; the grants below ask it, and the disk's owner must find its disk
    // in place when it starts (TASK-0246 P4b: no owner waits for its grant).
    crate::bootstrap::resume::resume_authority(ctrls);

    let pol_route = (pol_ctl_route_req, pol_ctl_route_rsp);
    mmio_policy_deny_probe(pol_route)?;
    // RFC-0098 C3: every window and interrupt line below comes from the tree.
    let devices = device_tree::discover_virtio()?;
    device_tree::report(&devices);
    // TASK-0246 P3: the tree's ECAM hosts are a device source too (the boot disk may be one's).
    let pci = crate::bootstrap::pci::discover();
    crate::bootstrap::pci::report(&pci);
    // TASK-0328 U1: the USB plane — QEMU's PCI xHCI, or (U3) the board's tree node: its owner
    // has socd bring its clocks, resets and glue up before it touches the window (a read of a
    // gated block can stall the bus), so the grant alone is safe.
    let usb = pci
        .usb
        .map(|host| UsbPlane {
            window: host.window,
            pci: Some(host.function),
            phy: None,
            ss_phy: None,
        })
        .or_else(device_tree::usb_plane);

    // RFC-0106 (TASK-0246 P4c): the SoC glue owner — the tree alias and every provider window —
    // runs before the disk is granted: the disk's owner asks it to bring the disk's node up
    // before it touches the controller.
    crate::bootstrap::soc_glue::provision(socd_pid, stats, pol_route, init_wire, init_fold)?;
    crate::bootstrap::resume::resume_soc_glue(ctrls);

    // The ONE grant the plane needs: the disk the boot came from → blkd (ADR-0044/0067: one
    // owner; every other client is a blockproto client). From here the block plane is live and
    // bundlemgrd can attach the measured volume.
    if let Some(blkd_pid) = ctrls.iter().find(|c| c.svc_name == "blkd").map(|c| c.pid) {
        grant_boot_disk(stats, pol_route, blkd_pid, (&devices, &pci), init_wire, init_fold)?;
    }

    // Wave 0b: the disk's owner (its disk granted) and the volume verifier.
    crate::bootstrap::resume::resume_plane(ctrls);

    // TASK-0321 (RFC-0089 §12.3, ADR-0060): the SECOND spawn pass — services
    // on the verified system volume, spawned BEFORE any per-pid endpoint
    // mint so the rest of bootstrap treats them exactly like embedded ones.
    let volume_span = nexus_abi::Span::begin();
    let volume = crate::bootstrap::volume_spawn::spawn_volume_services(
        ctrls,
        bnd_req,
        ask,
        init_fold,
        stage_fence,
    )?;
    let volume_ms = volume_span.elapsed_ms();

    Ok(CorePlane {
        pol_req,
        pol_rsp,
        bnd_req,
        bnd_rsp,
        blk_req,
        blk_rsp,
        soc_req,
        soc_rsp,
        pol_ctl_route_req,
        pol_ctl_exec_req,
        devices,
        usb,
        volume,
        volume_ms,
    })
}
