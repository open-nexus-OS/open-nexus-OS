// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Service bootstrap orchestrator — extracted from os_payload.rs per RFC-0061.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os)
//! ADR: docs/adr/0017-service-architecture.md
//! RFC: docs/rfcs/RFC-0061-selftest-observer-init-refactoring.md

use crate::bootstrap::route_builder;
use crate::bootstrap::spawn::spawn_service_with_probe;
use crate::bootstrap::{BootstrapState, CtrlChannel};
use crate::os_payload::*;
use crate::route_table::RouteTable;
use alloc::vec::Vec;

pub(crate) fn run_bootstrap<F>(
    images: &'static [ServiceImage],
    notifier: ReadyNotifier<F>,
) -> Result<BootstrapState>
where
    F: FnOnce() + Send,
{
    debug_write_bytes(b"!init-lite entry\n");
    debug_write_str("init: entry");
    debug_write_byte(b'\n');
    probe_debug_write_words();
    configure_log_topics();
    // Boot-timing signposts (Phase 3): total boot duration plus accumulated policy-grant wait,
    // emitted as a compact table at the end so boot bottlenecks (e.g. services waiting on policyd
    // MMIO grants) are visible without a separate profiler.
    let boot_span = nexus_abi::Span::begin();
    let grant_wait_ns = core::cell::Cell::new(0u64);
    let grant_count = core::cell::Cell::new(0u32);
    log_str_ptr("init-msg", "init: start");
    debug_write_str("init: start");
    debug_write_byte(b'\n');
    if probes_enabled() {
        debug_write_bytes(b"!images\n");
    }

    if images.is_empty() {
        debug_write_str("init: warn no services configured");
        debug_write_byte(b'\n');
    }

    // RFC-0005: Service IPC capability distribution (minimal VFS wiring).
    //
    // Phase-2 hardening: init-lite holds an EndpointFactory capability (slot 1) for endpoint_create.
    const ENDPOINT_FACTORY_CAP_SLOT: u32 = 1;
    //
    // Phase-2 hardening (ownership correctness):
    // We create *service request endpoints* owned by the target service PID (close-on-exit),
    // which requires knowing the PID. Therefore we create response endpoints up front, spawn
    // services, then create request endpoints (owner=service PID) and distribute caps in a
    // second pass before the first yield.
    // NOTE: response endpoints are owned by their receiver (typically the requester).
    // We create them after spawning once the requester PID is known.
    // Private init-lite -> policyd response channels (init-lite receives replies).
    let pol_ctl_route_rsp =
        nexus_abi::ipc_endpoint_create_v2(ENDPOINT_FACTORY_CAP_SLOT, 8).map_err(InitError::Abi)?;
    let init_pid = nexus_abi::pid().map_err(InitError::Abi)?;
    let init_reply_send = nexus_abi::cap_clone(pol_ctl_route_rsp).map_err(InitError::Abi)?;
    let init_reply_send =
        nexus_abi::cap_transfer(init_pid, init_reply_send, Rights::SEND).map_err(InitError::Abi)?;
    let pol_ctl_exec_rsp =
        nexus_abi::ipc_endpoint_create_v2(ENDPOINT_FACTORY_CAP_SLOT, 8).map_err(InitError::Abi)?;

    let mut ctrl_channels: Vec<CtrlChannel> = Vec::new();
    let spawn_span = nexus_abi::Span::begin();
    for (_idx, image) in images.iter().enumerate() {
        if probes_enabled() {
            debug_write_bytes(b"!svc-loop\n");
        }
        let name = ServiceNameGuard::new(image.name);
        if probes_enabled() {
            // Keep probe-only pointer diagnostics out of nexus_log to avoid boot-time coupling.
            raw_probe_str("svc-name", image.name);
        }
        name.trace_metadata();
        debug_write_str("init: start ");
        if let Some(value) = name.value {
            debug_write_str(value);
        } else {
            debug_write_str("[svc@0x");
            debug_write_hex(name.ptr);
            debug_write_str("/");
            debug_write_hex(name.len);
            debug_write_byte(b']');
        }
        debug_write_byte(b'\n');
        match crate::bootstrap::spawn::spawn_service_with_probe(image, probes_enabled()) {
            Ok(pid) => {
                // Create private control endpoints (REQ/RSP) for this service and transfer them first.
                // This ensures a deterministic slot assignment in the child (slots 1 and 2).
                //
                // IMPORTANT: These endpoints must remain usable by init-lite for the routing responder
                // loop. Creating them as init-owned endpoints avoids needing `cap_clone` (which adds
                // extra syscalls and increases preemption windows during bring-up).
                let ctrl_req_parent_slot =
                    nexus_abi::ipc_endpoint_create_v2(ENDPOINT_FACTORY_CAP_SLOT, CTRL_EP_DEPTH)
                        .map_err(InitError::Abi)?;
                let ctrl_rsp_parent_slot =
                    nexus_abi::ipc_endpoint_create_v2(ENDPOINT_FACTORY_CAP_SLOT, CTRL_EP_DEPTH)
                        .map_err(InitError::Abi)?;
                // IMPORTANT: The kernel IPC backend assumes the per-service routing control
                // channels live in deterministic slots (userspace `nexus-ipc` uses 1/2).
                // Use cap_transfer_to_slot to avoid slot drift when we add new capabilities.
                let child_send_slot = nexus_abi::cap_transfer_to_slot(
                    pid,
                    ctrl_req_parent_slot,
                    Rights::SEND,
                    CTRL_CHILD_SEND_SLOT,
                )
                .map_err(InitError::Abi)?;
                let child_recv_slot = nexus_abi::cap_transfer_to_slot(
                    pid,
                    ctrl_rsp_parent_slot,
                    Rights::RECV,
                    CTRL_CHILD_RECV_SLOT,
                )
                .map_err(InitError::Abi)?;
                if image.name == "updated" {
                    debug_write_bytes(b"init: updated ctrl slots send=0x");
                    debug_write_hex(child_send_slot as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(child_recv_slot as usize);
                    debug_write_byte(b'\n');
                }
                if probes_enabled()
                    && (child_send_slot != CTRL_CHILD_SEND_SLOT
                        || child_recv_slot != CTRL_CHILD_RECV_SLOT)
                {
                    debug_write_bytes(b"!route-warn ctrl-child-slots send=0x");
                    debug_write_hex(child_send_slot as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(child_recv_slot as usize);
                    debug_write_bytes(b" expected send=0x");
                    debug_write_hex(CTRL_CHILD_SEND_SLOT as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(CTRL_CHILD_RECV_SLOT as usize);
                    debug_write_byte(b'\n');
                }

                let ctrl = CtrlChannel {
                    svc_name: image.name,
                    pid,
                    ctrl_req_parent_slot,
                    ctrl_rsp_parent_slot,
                    vfs_send_slot: None,
                    vfs_recv_slot: None,
                    pkg_send_slot: None,
                    pkg_recv_slot: None,
                    pol_send_slot: None,
                    pol_recv_slot: None,
                    bnd_send_slot: None,
                    bnd_recv_slot: None,
                    upd_send_slot: None,
                    upd_recv_slot: None,
                    sam_send_slot: None,
                    sam_recv_slot: None,
                    exe_send_slot: None,
                    exe_recv_slot: None,
                    key_send_slot: None,
                    key_recv_slot: None,
                    state_send_slot: None,
                    state_recv_slot: None,
                    rng_send_slot: None,
                    rng_recv_slot: None,
                    timed_send_slot: None,
                    timed_recv_slot: None,
                    window_send_slot: None,
                    window_recv_slot: None,
                    input_send_slot: None,
                    input_recv_slot: None,
                    gpud_send_slot: None,
                    gpud_recv_slot: None,
                    net_send_slot: None,
                    net_recv_slot: None,
                    metrics_send_slot: None,
                    metrics_recv_slot: None,
                    log_send_slot: None,
                    log_recv_slot: None,
                    dsoft_send_slot: None,
                    dsoft_recv_slot: None,
                    reply_send_slot: None,
                    reply_recv_slot: None,
                };
                ctrl_channels.push(ctrl);
                if probes_enabled() {
                    debug_write_bytes(b"!spawn ok pid=0x");
                    debug_write_hex(pid as usize);
                    debug_write_byte(b'\n');
                }
                debug_write_str("init: up ");
                if let Some(value) = name.value {
                    debug_write_str(value);
                } else {
                    debug_write_str("[svc@0x");
                    debug_write_hex(name.ptr);
                    debug_write_str("/");
                    debug_write_hex(name.len);
                    debug_write_byte(b']');
                }
                debug_write_byte(b'\n');
            }
            Err(err) => {
                debug_write_str("init: fail ");
                if let Some(value) = name.value {
                    debug_write_str(value);
                } else {
                    debug_write_str("[svc@0x");
                    debug_write_hex(name.ptr);
                    debug_write_str("/");
                    debug_write_hex(name.len);
                    debug_write_byte(b']');
                }
                debug_write_str(" err=");
                // Minimal reason tag for UART; richer info stays in fatal_err.
                match &err {
                    InitError::Abi(_) => debug_write_str("abi"),
                    InitError::Ipc(_) => debug_write_str("ipc"),
                    InitError::Elf(_) => debug_write_str("elf"),
                    InitError::Map(_) => debug_write_str("map"),
                    InitError::MissingElf => debug_write_str("missing-elf"),
                }
                debug_write_byte(b'\n');
                fatal_err(err);
            }
        }
        // Yielding here is helpful for cooperative bring-up, but it can also mask
        // scheduler/AS-switching issues by jumping into the newly spawned task mid-print.
        // Keep the default bring-up deterministic: spawn the full set first, then yield.
    }
    let spawn_ms = spawn_span.elapsed_ms();

    notifier.notify();
    debug_write_str("init: ready");
    debug_write_byte(b'\n');
    debug_write_bytes(b"!init-lite ready\n");
    // Resume all spawned services now so policyd can handle MMIO policy
    // checks during the grant phase. IPC wiring happens after grants.
    for chan in &ctrl_channels {
        // Display + input device drivers are resumed AFTER grants + route wiring so they
        // initialize without startup probe/handoff races. A driver resumed before its MMIO
        // is granted busy-yields waiting for it — wasting scheduler cycles that slow the very
        // grant phase it is blocked on. Keeping it suspended (zero CPU) until ready is the
        // reactive, faster path. `hidrawd` previously raced here (see its `entry_to_ready_ms`).
        if matches!(chan.svc_name, "gpud" | "windowd" | "inputd" | "hidrawd") {
            continue;
        }
        match nexus_abi::task_resume(chan.pid) {
            Ok(()) => {}
            Err(e) => {
                debug_write_bytes(b"init: resume fail pid=0x");
                debug_write_hex(chan.pid as usize);
                debug_write_str(" svc=");
                debug_write_str(chan.svc_name);
                debug_write_str(" err=0x");
                debug_write_hex(e as usize);
                debug_write_byte(b'\n');
            }
        }
    }
    // Yield once so resumed services can bind their servers before init
    // starts sending IPC (grants need policyd, routes need samgrd, etc.).
    let _ = nexus_abi::yield_();

    // Second pass: create request endpoints owned by the target service PID and distribute caps.
    fn find_pid(ctrls: &[CtrlChannel], name: &str) -> Option<u32> {
        ctrls.iter().find(|c| c.svc_name == name).map(|c| c.pid)
    }

    let selftest_pid = find_pid(&ctrl_channels, "selftest-client").ok_or(InitError::MissingElf)?;
    let vfsd_pid = find_pid(&ctrl_channels, "vfsd").ok_or(InitError::MissingElf)?;
    let packagefsd_pid = find_pid(&ctrl_channels, "packagefsd").ok_or(InitError::MissingElf)?;
    let policyd_pid = find_pid(&ctrl_channels, "policyd").ok_or(InitError::MissingElf)?;
    let netstackd_pid = find_pid(&ctrl_channels, "netstackd").ok_or(InitError::MissingElf)?;
    let dsoftbusd_pid = find_pid(&ctrl_channels, "dsoftbusd").ok_or(InitError::MissingElf)?;
    let bundlemgrd_pid = find_pid(&ctrl_channels, "bundlemgrd").ok_or(InitError::MissingElf)?;
    let updated_pid = find_pid(&ctrl_channels, "updated").ok_or(InitError::MissingElf)?;
    let samgrd_pid = find_pid(&ctrl_channels, "samgrd").ok_or(InitError::MissingElf)?;
    let execd_pid = find_pid(&ctrl_channels, "execd").ok_or(InitError::MissingElf)?;
    let _keystored_pid = find_pid(&ctrl_channels, "keystored").ok_or(InitError::MissingElf)?;
    let _statefsd_pid = find_pid(&ctrl_channels, "statefsd").ok_or(InitError::MissingElf)?;
    let rngd_pid = find_pid(&ctrl_channels, "rngd").ok_or(InitError::MissingElf)?;
    let timed_pid = find_pid(&ctrl_channels, "timed").ok_or(InitError::MissingElf)?;
    let hidrawd_pid = find_pid(&ctrl_channels, "hidrawd").ok_or(InitError::MissingElf)?;
    let windowd_pid = find_pid(&ctrl_channels, "windowd").ok_or(InitError::MissingElf)?;
    let inputd_pid = find_pid(&ctrl_channels, "inputd").ok_or(InitError::MissingElf)?;
    let gpud_pid = find_pid(&ctrl_channels, "gpud").ok_or(InitError::MissingElf)?;
    let logd_pid = find_pid(&ctrl_channels, "logd");
    let metricsd_pid = find_pid(&ctrl_channels, "metricsd");

    // selftest-client <-> service endpoint pairs
    let vfs_req = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, vfsd_pid, 8)
        .map_err(InitError::Abi)?;
    let vfs_rsp = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, selftest_pid, 8)
        .map_err(InitError::Abi)?;
    let pkg_req = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, packagefsd_pid, 8)
        .map_err(InitError::Abi)?;
    let pkg_rsp = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, selftest_pid, 8)
        .map_err(InitError::Abi)?;
    let pol_req = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, policyd_pid, 8)
        .map_err(InitError::Abi)?;
    let pol_rsp = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, selftest_pid, 8)
        .map_err(InitError::Abi)?;
    let bnd_req = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, bundlemgrd_pid, 8)
        .map_err(InitError::Abi)?;
    let bnd_rsp = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, selftest_pid, 8)
        .map_err(InitError::Abi)?;
    let bnd_rsp_updated =
        nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, updated_pid, 8)
            .map_err(InitError::Abi)?;
    let upd_req = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, updated_pid, 8)
        .map_err(InitError::Abi)?;
    let upd_rsp = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, selftest_pid, 8)
        .map_err(InitError::Abi)?;
    let sam_req = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, samgrd_pid, 8)
        .map_err(InitError::Abi)?;
    // Clone so init-lite keeps a SEND cap to samgrd for registry population.
    let init_sam_send = nexus_abi::cap_clone(sam_req).map_err(InitError::Abi)?;
    let sam_rsp = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, selftest_pid, 8)
        .map_err(InitError::Abi)?;
    let init_sam_recv = nexus_abi::cap_clone(sam_rsp).map_err(InitError::Abi)?;
    let exe_req = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, execd_pid, 8)
        .map_err(InitError::Abi)?;
    let exe_rsp = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, selftest_pid, 8)
        .map_err(InitError::Abi)?;
    // Create init-owned endpoints so init-lite can deterministically distribute RECV/SEND rights.
    // `ipc_endpoint_create_for(... owner=keystored ...)` does not guarantee the creator holds RECV,
    // and `cap_transfer(... Rights::RECV)` can be rejected by the kernel.
    let key_req =
        nexus_abi::ipc_endpoint_create_v2(ENDPOINT_FACTORY_CAP_SLOT, 8).map_err(InitError::Abi)?;
    // #region agent log (probe key_req rights via self-transfer)
    if let Ok(me) = nexus_abi::pid() {
        debug_write_bytes(b"init: probe key_req self-xfer pid=0x");
        debug_write_hex(me as usize);
        debug_write_bytes(b" cap=0x");
        debug_write_hex(key_req as usize);
        debug_write_byte(b'\n');
        match nexus_abi::cap_transfer(me, key_req, Rights::SEND) {
            Ok(slot) => {
                debug_write_bytes(b"init: probe key_req self-xfer SEND ok slot=0x");
                debug_write_hex(slot as usize);
                debug_write_byte(b'\n');
                let _ = nexus_abi::cap_close(slot);
            }
            Err(e) => {
                debug_write_bytes(b"init: probe key_req self-xfer SEND err=abi:");
                debug_write_str(abi_error_label(e.clone()));
                debug_write_byte(b'\n');
            }
        }
        match nexus_abi::cap_transfer(me, key_req, Rights::RECV) {
            Ok(slot) => {
                debug_write_bytes(b"init: probe key_req self-xfer RECV ok slot=0x");
                debug_write_hex(slot as usize);
                debug_write_byte(b'\n');
                let _ = nexus_abi::cap_close(slot);
            }
            Err(e) => {
                debug_write_bytes(b"init: probe key_req self-xfer RECV err=abi:");
                debug_write_str(abi_error_label(e.clone()));
                debug_write_byte(b'\n');
            }
        }
    } else {
        debug_write_bytes(b"init: probe key_req self-xfer pid() failed\n");
    }
    // #endregion agent log
    let key_rsp = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, selftest_pid, 8)
        .map_err(InitError::Abi)?;
    // NOTE: keep this endpoint init-owned so statefsd's cap table stays clear at slot 0x30
    // until the policy-gated MMIO grant is transferred there (statefsd probes MMIO at slot 48).
    let state_req =
        nexus_abi::ipc_endpoint_create_v2(ENDPOINT_FACTORY_CAP_SLOT, 8).map_err(InitError::Abi)?;
    let state_rsp = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, selftest_pid, 8)
        .map_err(InitError::Abi)?;

    // rngd <-> clients endpoints:
    // - rng_req owned by rngd (server receives requests)
    // - rng_rsp owned by selftest-client (server can send direct replies to selftest without CAP_MOVE)
    let rng_req = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, rngd_pid, 8)
        .map_err(InitError::Abi)?;
    let rng_rsp = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, selftest_pid, 8)
        .map_err(InitError::Abi)?;
    let timed_req = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, timed_pid, 8)
        .map_err(InitError::Abi)?;
    let timed_rsp = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, selftest_pid, 8)
        .map_err(InitError::Abi)?;
    let window_req = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, windowd_pid, 32)
        .map_err(InitError::Abi)?;
    let window_rsp = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, windowd_pid, 8)
        .map_err(InitError::Abi)?;
    let input_req = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, inputd_pid, 8)
        .map_err(InitError::Abi)?;
    let input_rsp = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, hidrawd_pid, 8)
        .map_err(InitError::Abi)?;

    // Priority-wire display services early (right after their endpoints exist)
    // so they get scheduled by the existing yield after MMIO grants.

    let gpud_req = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, gpud_pid, 8)
        .map_err(InitError::Abi)?;
    let gpud_rsp = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, gpud_pid, 8)
        .map_err(InitError::Abi)?;

    // logd (optional) service endpoints (request/response).
    // If logd is present in the image set, selftest-client gets a dedicated pair.
    let (log_req, log_rsp) = if let Some(_pid) = logd_pid {
        // logd is a high-fan-in sink (policyd/execd/bundlemgrd/updated/etc). Use a larger queue
        // budget to avoid CAP_MOVE senders hitting QueueFull under cooperative scheduling.
        // NOTE: Keep the request endpoint init-owned so it remains valid independent of bring-up
        // ordering. Init-lite distributes SEND/RECV rights explicitly to the participants.
        let req = nexus_abi::ipc_endpoint_create_v2(ENDPOINT_FACTORY_CAP_SLOT, 64)
            .map_err(InitError::Abi)?;
        let rsp = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, selftest_pid, 8)
            .map_err(InitError::Abi)?;
        (Some(req), Some(rsp))
    } else {
        (None, None)
    };

    // metricsd (optional) service endpoints (request/response).
    // If metricsd is present, selftest-client gets a deterministic pair.
    let (metrics_req, metrics_rsp) = if let Some(pid) = metricsd_pid {
        let req = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, 8)
            .map_err(InitError::Abi)?;
        let rsp = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, selftest_pid, 8)
            .map_err(InitError::Abi)?;
        (Some(req), Some(rsp))
    } else {
        (None, None)
    };

    // bundlemgrd <-> execd dedicated pair (avoid reusing selftest-client <-> execd channels)
    let bnd_exe_req = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, execd_pid, 8)
        .map_err(InitError::Abi)?;
    let bnd_exe_rsp =
        nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, bundlemgrd_pid, 8)
            .map_err(InitError::Abi)?;

    // Selftest reply-inbox endpoint:
    // - owned by selftest-client (receiver)
    // - selftest-client holds RECV to await replies and a SEND cap that it can CAP_MOVE to a server
    let reply_ep = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, selftest_pid, 8)
        .map_err(InitError::Abi)?;

    // execd reply-inbox endpoint (for CAP_MOVE request/reply, e.g. logd crash append).
    let execd_reply_ep =
        nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, execd_pid, 8)
            .map_err(InitError::Abi)?;

    // DSoftBusd reply-inbox endpoint (for CAP_MOVE request/reply).
    let dsoft_reply_ep =
        nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, dsoftbusd_pid, 8)
            .map_err(InitError::Abi)?;

    // DSoftBusd service endpoints (request/response) so other tasks (e.g. selftest-client) can route to it.
    let dsoft_req = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, dsoftbusd_pid, 8)
        .map_err(InitError::Abi)?;
    let dsoft_rsp = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, selftest_pid, 8)
        .map_err(InitError::Abi)?;

    // Netstackd service endpoints (request/response).
    let net_req = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, netstackd_pid, 8)
        .map_err(InitError::Abi)?;
    let net_rsp = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, netstackd_pid, 8)
        .map_err(InitError::Abi)?;
    // Client-side netstackd receive endpoints (currently unused by the CAP_MOVE protocol but required for routing).
    let net_selftest_rsp =
        nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, selftest_pid, 8)
            .map_err(InitError::Abi)?;
    let net_dsoft_rsp =
        nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, dsoftbusd_pid, 8)
            .map_err(InitError::Abi)?;

    // packagefsd reply-inbox endpoint (for CAP_MOVE request/reply to other services, e.g. bundlemgrd):
    let pkg_reply_ep =
        nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, packagefsd_pid, 8)
            .map_err(InitError::Abi)?;

    // Private init-lite <-> policyd channels: request endpoints are owned by policyd (it receives queries).
    let pol_ctl_route_req =
        nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, policyd_pid, 8)
            .map_err(InitError::Abi)?;
    let pol_ctl_exec_req =
        nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, policyd_pid, 8)
            .map_err(InitError::Abi)?;

    // Ensure policyd control channels are live before policy-gated grants.
    // These must be pinned to fixed child slots; `policyd` reads route/exec control on 5/6 and 7/8.
    const POLICYD_CTL_ROUTE_RECV_SLOT: u32 = 5;
    const POLICYD_CTL_ROUTE_SEND_SLOT: u32 = 6;
    const POLICYD_CTL_EXEC_RECV_SLOT: u32 = 7;
    const POLICYD_CTL_EXEC_SEND_SLOT: u32 = 8;
    let _ = nexus_abi::cap_transfer_to_slot(
        policyd_pid,
        pol_ctl_route_req,
        Rights::RECV,
        POLICYD_CTL_ROUTE_RECV_SLOT,
    )
    .map_err(InitError::Abi)?;
    let _ = nexus_abi::cap_transfer_to_slot(
        policyd_pid,
        pol_ctl_route_rsp,
        Rights::SEND,
        POLICYD_CTL_ROUTE_SEND_SLOT,
    )
    .map_err(InitError::Abi)?;
    let _ = nexus_abi::cap_transfer_to_slot(
        policyd_pid,
        pol_ctl_exec_req,
        Rights::RECV,
        POLICYD_CTL_EXEC_RECV_SLOT,
    )
    .map_err(InitError::Abi)?;
    let _ = nexus_abi::cap_transfer_to_slot(
        policyd_pid,
        pol_ctl_exec_rsp,
        Rights::SEND,
        POLICYD_CTL_EXEC_SEND_SLOT,
    )
    .map_err(InitError::Abi)?;

    // Priority-wire policyd BEFORE MMIO grants so policy checks complete in microseconds.
    // Clone caps so the originals stay available for other services that need SEND rights.
    {
        let pol_req_clone = nexus_abi::cap_clone(pol_req).map_err(InitError::Abi)?;
        let pol_rsp_clone = nexus_abi::cap_clone(pol_rsp).map_err(InitError::Abi)?;
        if let Some(chan) = ctrl_channels.iter_mut().find(|c| c.svc_name == "policyd") {
            let pid = chan.pid;
            chan.pol_recv_slot = Some(
                nexus_abi::cap_transfer(pid, pol_req_clone, Rights::RECV)
                    .map_err(InitError::Abi)?,
            );
            chan.pol_send_slot = Some(
                nexus_abi::cap_transfer(pid, pol_rsp_clone, Rights::SEND)
                    .map_err(InitError::Abi)?,
            );
            debug_write_bytes(b"init: policyd priority-wired\n");
        }
    }

    // Priority-wire windowd + inputd using clones.
    {
        let window_req_clone = nexus_abi::cap_clone(window_req).map_err(InitError::Abi)?;
        let window_rsp_clone = nexus_abi::cap_clone(window_rsp).map_err(InitError::Abi)?;
        let input_req_clone = nexus_abi::cap_clone(input_req).map_err(InitError::Abi)?;
        let input_rsp_clone = nexus_abi::cap_clone(input_rsp).map_err(InitError::Abi)?;
        if let Some(chan) = ctrl_channels.iter_mut().find(|c| c.svc_name == "windowd") {
            let pid = chan.pid;
            chan.window_recv_slot = Some(
                nexus_abi::cap_transfer(pid, window_req_clone, Rights::RECV)
                    .map_err(InitError::Abi)?,
            );
            chan.window_send_slot = Some(
                nexus_abi::cap_transfer(pid, window_rsp_clone, Rights::SEND)
                    .map_err(InitError::Abi)?,
            );
            debug_write_bytes(b"init: windowd priority-wired\n");
            // NOTE: windowd's registry reply-inbox + bundlemgrd route caps are
            // provisioned LATE (after the gpud caps land at the fallback slots
            // 5/6 the display handoff hardcodes) — see the windowd block after the
            // wiring loop. Provisioning them HERE shifted gpud to slots 8/9 and
            // broke the present handoff with kernel-permission-denied.
        }
        if let Some(chan) = ctrl_channels.iter_mut().find(|c| c.svc_name == "inputd") {
            let pid = chan.pid;
            chan.input_recv_slot = Some(
                nexus_abi::cap_transfer(pid, input_req_clone, Rights::RECV)
                    .map_err(InitError::Abi)?,
            );
            chan.input_send_slot = Some(
                nexus_abi::cap_transfer(pid, input_rsp_clone, Rights::SEND)
                    .map_err(InitError::Abi)?,
            );
            debug_write_bytes(b"init: inputd priority-wired\n");
        }
    }

    // Policy-gated DeviceMmio grants (per-device windows) before other cap transfers.
    let grant_mmio_with_wait =
        |pid: u32, svc_name: &str, cap_name: &str, slot: usize, cap_slot: u32| -> Result<()> {
            let (mmio_base, mmio_len) = virtio_mmio_window(slot);
            let grant_span = nexus_abi::Span::begin();
            let deadline = match nexus_abi::nsec() {
                Ok(now) => now.saturating_add(1_000_000_000),
                Err(_) => 0,
            };
            loop {
                match grant_mmio_cap(
                    pid,
                    svc_name,
                    cap_name,
                    mmio_base,
                    mmio_len,
                    pol_ctl_route_req,
                    pol_ctl_route_rsp,
                    cap_slot,
                )? {
                    Some(_) => break,
                    None => {
                        let now = match nexus_abi::nsec() {
                            Ok(value) => value,
                            Err(_) => 0,
                        };
                        if now >= deadline {
                            return Err(InitError::Map("mmio policy timeout"));
                        }
                        let _ = nexus_abi::yield_();
                    }
                }
            }
            grant_wait_ns.set(grant_wait_ns.get().saturating_add(grant_span.elapsed_ns()));
            grant_count.set(grant_count.get().saturating_add(1));
            Ok(())
        };

    // Policy negative proof: deny-by-default for a non-matching MMIO capability.
    //
    // Today we use a stable, always-present subject (`netstackd`) and a capability that must not
    // be granted to it (`device.mmio.blk`). This is independent of device enumeration and proves:
    // - init consults policyd (no local allowlist)
    // - policyd denies by default for a capability not in policy
    // - a deterministic UART marker is emitted only on real denial
    let deny_deadline = match nexus_abi::nsec() {
        Ok(now) => now.saturating_add(1_000_000_000),
        Err(_) => 0,
    };
    loop {
        let subject_id = nexus_abi::service_id_from_name(b"netstackd");
        match policyd_cap_allowed(
            pol_ctl_route_req,
            pol_ctl_route_rsp,
            subject_id,
            b"device.mmio.blk",
        ) {
            Some(false) => {
                debug_write_str("init: mmio policy deny ok");
                debug_write_byte(b'\n');
                break;
            }
            Some(true) => {
                return Err(InitError::Map("mmio policy deny unexpectedly allowed"));
            }
            None => {
                let now = match nexus_abi::nsec() {
                    Ok(value) => value,
                    Err(_) => 0,
                };
                if now >= deny_deadline {
                    return Err(InitError::Map("mmio policy deny timeout"));
                }
                let _ = nexus_abi::yield_();
            }
        }
    }

    let (net_slot, rng_slot, blk_slot, gpu_slot, input_slots) = probe_virtio_mmio_slots()?;
    grant_mmio_with_wait(
        netstackd_pid,
        "netstackd",
        "device.mmio.net",
        net_slot,
        DEVICE_MMIO_CAP_SLOT,
    )?;
    grant_mmio_with_wait(rngd_pid, "rngd", "device.mmio.rng", rng_slot, DEVICE_MMIO_CAP_SLOT)?;
    let gpu_slot = gpu_slot.ok_or(InitError::Map("virtio-gpu slot not found"))?;
    grant_mmio_with_wait(gpud_pid, "gpud", "device.mmio.gpu", gpu_slot, DEVICE_MMIO_CAP_SLOT)?;
    grant_mmio_with_wait(
        selftest_pid,
        "selftest-client",
        "device.mmio.net",
        net_slot,
        DEVICE_MMIO_CAP_SLOT,
    )?;

    // fw_cfg: hand the QEMU firmware-config MMIO window to selftest-client so it can read its
    // runtime boot-config (selftest mode/profile, set by the launcher via `-fw_cfg`) WITHOUT a
    // rebuild — the same binary boots in `proof` mode under the harness and `interactive-full`
    // under `just start`. This is a host-config channel, not a policy-gated device, so it is
    // minted + transferred directly (no policyd round-trip) to the fixed slot the client maps
    // (`boot_cfg::FW_CFG_SLOT`). Non-fatal: if the mint/transfer fails the client's `mmio_map`
    // degrades gracefully (runtime_mode → None → the legacy `full` profile + verdict mode off).
    {
        const FW_CFG_BASE: usize = 0x1010_0000; // QEMU virt VIRT_FW_CFG window base.
        const FW_CFG_LEN: usize = 0x1000; // One page (regs live at offset 0/8).
        const FW_CFG_DST_SLOT: u32 = 0x31; // Must match selftest-client `boot_cfg::FW_CFG_SLOT`.
        match nexus_abi::device_mmio_cap_create(FW_CFG_BASE, FW_CFG_LEN, usize::MAX) {
            Ok(cap) => {
                match nexus_abi::cap_transfer_to_slot(
                    selftest_pid,
                    cap,
                    Rights::MAP,
                    FW_CFG_DST_SLOT,
                ) {
                    Ok(_) => debug_write_bytes(b"init: fw_cfg grant ok svc=selftest-client\n"),
                    Err(_) => {
                        debug_write_bytes(b"init: fw_cfg grant xfer FAIL svc=selftest-client\n")
                    }
                }
            }
            Err(_) => debug_write_bytes(b"init: fw_cfg cap_create FAIL svc=selftest-client\n"),
        }
    }

    for (idx, input_slot) in input_slots.iter().copied().enumerate() {
        if let Some(input_slot) = input_slot {
            grant_mmio_with_wait(
                hidrawd_pid,
                "hidrawd",
                "device.mmio.input",
                input_slot,
                INPUT_MMIO_CAP_SLOT_BASE + u32::try_from(idx).unwrap_or(0),
            )?;
        }
    }

    if let Some(virtioblkd_pid) = find_pid(&ctrl_channels, "virtioblkd") {
        let blk_slot = blk_slot.ok_or(InitError::Map("virtio-blk slot not found"))?;
        grant_mmio_with_wait(
            virtioblkd_pid,
            "virtioblkd",
            "device.mmio.blk",
            blk_slot,
            DEVICE_MMIO_CAP_SLOT,
        )?;
    }
    if let Some(statefsd_pid) = find_pid(&ctrl_channels, "statefsd") {
        let blk_slot = blk_slot.ok_or(InitError::Map("virtio-blk slot not found"))?;
        grant_mmio_with_wait(
            statefsd_pid,
            "statefsd",
            "device.mmio.blk",
            blk_slot,
            DEVICE_MMIO_CAP_SLOT,
        )?;
    }
    // Cumulative boot elapsed at the end of the MMIO-grant phase (spawn + resume + early
    // wiring + grants). The gap to `total_ms` is the per-service cap-wiring phase, during which
    // init yields and the resumed services co-run their self-init.
    let grants_done_ms = boot_span.elapsed_ms();

    /// Transfer a capability to a child PID with graceful error handling.
    /// Returns Some(slot) on success, None on failure (logs the error).
    /// On success, emits a `cap:` hop marker for traceability.
    fn try_transfer(pid: u32, cap: u32, rights: Rights, svc: &str, label: &str) -> Option<u32> {
        match nexus_abi::cap_transfer(pid, cap, rights) {
            Ok(slot) => {
                debug_write_bytes(b"cap: route init->");
                debug_write_str(svc);
                debug_write_bytes(b" ");
                debug_write_str(label);
                debug_write_bytes(b" src=0x");
                debug_write_hex(cap as usize);
                debug_write_bytes(b" dst=0x");
                debug_write_hex(slot as usize);
                debug_write_byte(b'\n');
                Some(slot)
            }
            Err(e) => {
                debug_write_bytes(b"init: skip ");
                debug_write_str(svc);
                debug_write_bytes(b" ");
                debug_write_str(label);
                debug_write_bytes(b": ");
                debug_write_str(abi_error_label(e));
                debug_write_byte(b'\n');
                None
            }
        }
    }

    // Services are suspended; they will be resumed atomically at the end
    // after all MMIO and IPC wiring is complete.
    let _ = nexus_abi::yield_();

    for chan in &mut ctrl_channels {
        let pid = chan.pid;
        // Per-service wire-up progress: off by default (probe topic; `INIT_LITE_LOG_TOPICS=probe`).
        if probes_enabled() {
            debug_write_bytes(b"init: wire svc=");
            debug_write_str(chan.svc_name);
            debug_write_bytes(b" pid=0x");
            debug_write_hex(pid as usize);
            debug_write_byte(b'\n');
        }
        match chan.svc_name {
            "netstackd" => {
                // Provide netstackd its own request/response endpoints (server side).
                // #region agent log (netstackd cap transfers)
                debug_write_bytes(b"init: wire netstackd xfer net_req RECV\n");
                // #endregion agent log
                let recv_slot = match nexus_abi::cap_transfer(pid, net_req, Rights::RECV) {
                    Ok(slot) => slot,
                    Err(e) => {
                        // #region agent log (netstackd cap transfer error)
                        debug_write_bytes(b"init: wire netstackd xfer net_req err=abi:");
                        debug_write_str(abi_error_label(e.clone()));
                        debug_write_byte(b'\n');
                        // #endregion agent log
                        return Err(InitError::Abi(e));
                    }
                };

                // #region agent log (netstackd cap transfers)
                debug_write_bytes(b"init: wire netstackd xfer net_rsp SEND\n");
                // #endregion agent log
                let send_slot = match nexus_abi::cap_transfer(pid, net_rsp, Rights::SEND) {
                    Ok(slot) => slot,
                    Err(e) => {
                        // #region agent log (netstackd cap transfer error)
                        debug_write_bytes(b"init: wire netstackd xfer net_rsp err=abi:");
                        debug_write_str(abi_error_label(e.clone()));
                        debug_write_byte(b'\n');
                        // #endregion agent log
                        return Err(InitError::Abi(e));
                    }
                };
                chan.net_send_slot = Some(send_slot);
                chan.net_recv_slot = Some(recv_slot);
                debug_write_bytes(b"init: netstackd svc slots recv=0x");
                debug_write_hex(recv_slot as usize);
                debug_write_bytes(b" send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_byte(b'\n');
            }
            "dsoftbusd" => {
                // Allow dsoftbusd to send requests to netstackd (and optionally receive on a dedicated inbox).
                // Place into fixed slots to match userspace bring-up constants (avoid relying on allocation order).
                let send_slot = nexus_abi::cap_transfer_to_slot(pid, net_req, Rights::SEND, 0x03)
                    .map_err(InitError::Abi)?;
                let recv_slot =
                    nexus_abi::cap_transfer_to_slot(pid, net_dsoft_rsp, Rights::RECV, 0x04)
                        .map_err(InitError::Abi)?;
                chan.net_send_slot = Some(send_slot);
                chan.net_recv_slot = Some(recv_slot);
                debug_write_bytes(b"init: dsoftbusd netstackd slots send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_bytes(b" recv=0x");
                debug_write_hex(recv_slot as usize);
                debug_write_byte(b'\n');

                // Reply inbox: provide both RECV (stay with client) and SEND (to be moved to servers).
                let reply_recv_slot =
                    nexus_abi::cap_transfer_to_slot(pid, dsoft_reply_ep, Rights::RECV, 0x05)
                        .map_err(InitError::Abi)?;
                let reply_send_slot =
                    nexus_abi::cap_transfer_to_slot(pid, dsoft_reply_ep, Rights::SEND, 0x06)
                        .map_err(InitError::Abi)?;
                chan.reply_recv_slot = Some(reply_recv_slot);
                chan.reply_send_slot = Some(reply_send_slot);
                let _ = nexus_abi::cap_close(dsoft_reply_ep);
                debug_write_bytes(b"init: dsoftbusd reply slots recv=0x");
                debug_write_hex(reply_recv_slot as usize);
                debug_write_bytes(b" send=0x");
                debug_write_hex(reply_send_slot as usize);
                debug_write_byte(b'\n');

                // Allow dsoftbusd to call into samgrd/bundlemgrd via CAP_MOVE reply inbox.
                // - send to service request endpoint
                // - receive replies on local reply inbox recv slot
                let send_slot =
                    nexus_abi::cap_transfer(pid, sam_req, Rights::SEND).map_err(InitError::Abi)?;
                chan.sam_send_slot = Some(send_slot);
                chan.sam_recv_slot = Some(reply_recv_slot);
                let send_slot =
                    nexus_abi::cap_transfer(pid, bnd_req, Rights::SEND).map_err(InitError::Abi)?;
                chan.bnd_send_slot = Some(send_slot);
                chan.bnd_recv_slot = Some(reply_recv_slot);
                // TASK-0016: remote packagefs RO path requires dsoftbusd -> packagefsd routing.
                let send_slot =
                    nexus_abi::cap_transfer(pid, pkg_req, Rights::SEND).map_err(InitError::Abi)?;
                let recv_slot =
                    nexus_abi::cap_transfer(pid, pkg_rsp, Rights::RECV).map_err(InitError::Abi)?;
                chan.pkg_send_slot = Some(send_slot);
                chan.pkg_recv_slot = Some(recv_slot);
                // #region agent log
                debug_write_bytes(b"init: dsoftbusd packagefsd slots send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_bytes(b" recv=0x");
                debug_write_hex(recv_slot as usize);
                debug_write_byte(b'\n');
                // #endregion

                // TASK-0017 closeout: allow dsoftbusd to proxy remote statefs via statefsd.
                let send_slot = nexus_abi::cap_transfer(pid, state_req, Rights::SEND)
                    .map_err(InitError::Abi)?;
                let recv_slot = nexus_abi::cap_transfer(pid, state_rsp, Rights::RECV)
                    .map_err(InitError::Abi)?;
                chan.state_send_slot = Some(send_slot);
                chan.state_recv_slot = Some(recv_slot);
                // #region agent log
                debug_write_bytes(b"init: dsoftbusd statefsd slots send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_bytes(b" recv=0x");
                debug_write_hex(recv_slot as usize);
                debug_write_byte(b'\n');
                // #endregion

                // Provide dsoftbusd its own request/response endpoints (server side).
                let recv_slot = nexus_abi::cap_transfer(pid, dsoft_req, Rights::RECV)
                    .map_err(InitError::Abi)?;
                let send_slot = nexus_abi::cap_transfer(pid, dsoft_rsp, Rights::SEND)
                    .map_err(InitError::Abi)?;
                chan.dsoft_send_slot = Some(send_slot);
                chan.dsoft_recv_slot = Some(recv_slot);

                // TASK-0006: allow dsoftbusd to send structured logs to logd via CAP_MOVE (reply inbox).
                if let Some(req) = log_req {
                    let send_slot =
                        nexus_abi::cap_transfer(pid, req, Rights::SEND).map_err(InitError::Abi)?;
                    chan.log_send_slot = Some(send_slot);
                    chan.log_recv_slot = Some(reply_recv_slot);
                }
            }
            "vfsd" => {
                let recv_slot =
                    nexus_abi::cap_transfer(pid, vfs_req, Rights::RECV).map_err(InitError::Abi)?;
                let send_slot =
                    nexus_abi::cap_transfer(pid, vfs_rsp, Rights::SEND).map_err(InitError::Abi)?;
                chan.vfs_send_slot = Some(send_slot);
                chan.vfs_recv_slot = Some(recv_slot);

                // vfsd needs to resolve pkg:/ paths against packagefsd (real data path).
                let send_slot =
                    nexus_abi::cap_transfer(pid, pkg_req, Rights::SEND).map_err(InitError::Abi)?;
                let recv_slot =
                    nexus_abi::cap_transfer(pid, pkg_rsp, Rights::RECV).map_err(InitError::Abi)?;
                chan.pkg_send_slot = Some(send_slot);
                chan.pkg_recv_slot = Some(recv_slot);
            }
            "packagefsd" => {
                let recv_slot =
                    nexus_abi::cap_transfer(pid, pkg_req, Rights::RECV).map_err(InitError::Abi)?;
                let send_slot =
                    nexus_abi::cap_transfer(pid, pkg_rsp, Rights::SEND).map_err(InitError::Abi)?;
                chan.pkg_send_slot = Some(send_slot);
                chan.pkg_recv_slot = Some(recv_slot);

                // Provide a reply inbox for CAP_MOVE replies.
                let reply_recv_slot = nexus_abi::cap_transfer(pid, pkg_reply_ep, Rights::RECV)
                    .map_err(InitError::Abi)?;
                let reply_send_slot = nexus_abi::cap_transfer(pid, pkg_reply_ep, Rights::SEND)
                    .map_err(InitError::Abi)?;
                chan.reply_recv_slot = Some(reply_recv_slot);
                chan.reply_send_slot = Some(reply_send_slot);
                let _ = nexus_abi::cap_close(pkg_reply_ep);

                // Allow packagefsd to talk to bundlemgrd using CAP_MOVE replies:
                // - send to bundlemgrd's request endpoint
                // - receive replies on the local reply inbox recv slot
                let send_slot =
                    nexus_abi::cap_transfer(pid, bnd_req, Rights::SEND).map_err(InitError::Abi)?;
                chan.bnd_send_slot = Some(send_slot);
                chan.bnd_recv_slot = Some(reply_recv_slot);
            }
            "policyd" => {
                // Already priority-wired before MMIO grants — skip re-wiring.
                if chan.pol_send_slot.is_some() && chan.pol_recv_slot.is_some() {
                    debug_write_bytes(b"init: policyd already priority-wired, skip\n");
                    // Still need reply inbox and logd caps.
                    let pid = chan.pid;
                    let reply_ep =
                        nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, 8)
                            .map_err(InitError::Abi)?;
                    let reply_recv_slot = nexus_abi::cap_transfer(pid, reply_ep, Rights::RECV)
                        .map_err(InitError::Abi)?;
                    let reply_send_slot = nexus_abi::cap_transfer(pid, reply_ep, Rights::SEND)
                        .map_err(InitError::Abi)?;
                    chan.reply_recv_slot = Some(reply_recv_slot);
                    chan.reply_send_slot = Some(reply_send_slot);
                    chan.state_recv_slot = Some(reply_recv_slot);
                    let _ = nexus_abi::cap_close(reply_ep);
                    if let Some(req) = log_req {
                        let send_slot = nexus_abi::cap_transfer(pid, req, Rights::SEND)
                            .map_err(InitError::Abi)?;
                        chan.log_send_slot = Some(send_slot);
                        chan.log_recv_slot = Some(reply_recv_slot);
                    }
                } else {
                    let recv_slot = nexus_abi::cap_transfer(pid, pol_req, Rights::RECV)
                        .map_err(InitError::Abi)?;
                    let send_slot = nexus_abi::cap_transfer(pid, pol_rsp, Rights::SEND)
                        .map_err(InitError::Abi)?;
                    chan.pol_send_slot = Some(send_slot);
                    chan.pol_recv_slot = Some(recv_slot);
                    debug_write_bytes(b"init: policyd slots recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_bytes(b" send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_byte(b'\n');

                    // Provide a reply inbox for CAP_MOVE reply routing (used by log sinks).
                    let reply_ep =
                        nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, 8)
                            .map_err(InitError::Abi)?;
                    let reply_recv_slot = nexus_abi::cap_transfer(pid, reply_ep, Rights::RECV)
                        .map_err(InitError::Abi)?;
                    let reply_send_slot = nexus_abi::cap_transfer(pid, reply_ep, Rights::SEND)
                        .map_err(InitError::Abi)?;
                    chan.reply_recv_slot = Some(reply_recv_slot);
                    chan.reply_send_slot = Some(reply_send_slot);
                    chan.state_recv_slot = Some(reply_recv_slot);
                    let _ = nexus_abi::cap_close(reply_ep);
                    debug_write_bytes(b"init: policyd reply slots recv=0x");
                    debug_write_hex(reply_recv_slot as usize);
                    debug_write_bytes(b" send=0x");
                    debug_write_hex(reply_send_slot as usize);
                    debug_write_byte(b'\n');

                    // TASK-0006: allow policyd to send structured logs to logd via CAP_MOVE (reply inbox).
                    if let Some(req) = log_req {
                        let send_slot = nexus_abi::cap_transfer(pid, req, Rights::SEND)
                            .map_err(InitError::Abi)?;
                        chan.log_send_slot = Some(send_slot);
                        chan.log_recv_slot = Some(reply_recv_slot);
                        debug_write_bytes(b"init: policyd logd slots send=0x");
                        debug_write_hex(send_slot as usize);
                        debug_write_bytes(b" recv=0x");
                        debug_write_hex(reply_recv_slot as usize);
                        debug_write_byte(b'\n');
                    }
                }
            }
            "bundlemgrd" => {
                let recv_slot =
                    nexus_abi::cap_transfer(pid, bnd_req, Rights::RECV).map_err(InitError::Abi)?;
                let send_slot =
                    nexus_abi::cap_transfer(pid, bnd_rsp, Rights::SEND).map_err(InitError::Abi)?;
                chan.bnd_send_slot = Some(send_slot);
                chan.bnd_recv_slot = Some(recv_slot);
                debug_write_bytes(b"init: bundlemgrd slots recv=0x");
                debug_write_hex(recv_slot as usize);
                debug_write_bytes(b" send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_byte(b'\n');

                // Allow bundlemgrd to route to execd (policyd may still deny).
                let send_slot = nexus_abi::cap_transfer(pid, bnd_exe_req, Rights::SEND)
                    .map_err(InitError::Abi)?;
                let recv_slot = nexus_abi::cap_transfer(pid, bnd_exe_rsp, Rights::RECV)
                    .map_err(InitError::Abi)?;
                chan.exe_send_slot = Some(send_slot);
                chan.exe_recv_slot = Some(recv_slot);
                let _ = nexus_abi::cap_close(bnd_exe_req);
                let _ = nexus_abi::cap_close(bnd_exe_rsp);

                // Provide a reply inbox for CAP_MOVE reply routing (used by log sinks).
                let reply_ep =
                    nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, 8)
                        .map_err(InitError::Abi)?;
                let reply_recv_slot =
                    nexus_abi::cap_transfer(pid, reply_ep, Rights::RECV).map_err(InitError::Abi)?;
                let reply_send_slot =
                    nexus_abi::cap_transfer(pid, reply_ep, Rights::SEND).map_err(InitError::Abi)?;
                chan.reply_recv_slot = Some(reply_recv_slot);
                chan.reply_send_slot = Some(reply_send_slot);
                let _ = nexus_abi::cap_close(reply_ep);

                // TASK-0006: allow bundlemgrd to send structured logs to logd via CAP_MOVE (reply inbox).
                if let Some(req) = log_req {
                    let send_slot =
                        nexus_abi::cap_transfer(pid, req, Rights::SEND).map_err(InitError::Abi)?;
                    chan.log_send_slot = Some(send_slot);
                    chan.log_recv_slot = Some(reply_recv_slot);
                }
            }
            "updated" => {
                let recv_slot =
                    nexus_abi::cap_transfer(pid, upd_req, Rights::RECV).map_err(InitError::Abi)?;
                let send_slot =
                    nexus_abi::cap_transfer(pid, upd_rsp, Rights::SEND).map_err(InitError::Abi)?;
                chan.upd_send_slot = Some(send_slot);
                chan.upd_recv_slot = Some(recv_slot);
                debug_write_bytes(b"init: updated slots recv=0x");
                debug_write_hex(recv_slot as usize);
                debug_write_bytes(b" send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_byte(b'\n');

                let transfer = |cap: u32, rights: Rights, label: &'static str| -> Option<u32> {
                    match nexus_abi::cap_transfer(pid, cap, rights) {
                        Ok(slot) => Some(slot),
                        Err(err) => {
                            debug_write_bytes(b"init: updated cap transfer fail ");
                            debug_write_str(label);
                            debug_write_bytes(b" err=");
                            debug_write_str(abi_error_label(err.clone()));
                            debug_write_byte(b'\n');
                            None
                        }
                    }
                };

                // Allow updated to call bundlemgrd (slot-aware publication).
                let send_slot = transfer(bnd_req, Rights::SEND, "bundlemgrd send");
                let recv_slot = transfer(bnd_rsp_updated, Rights::RECV, "bundlemgrd recv");
                if let (Some(send_slot), Some(recv_slot)) = (send_slot, recv_slot) {
                    chan.bnd_send_slot = Some(send_slot);
                    chan.bnd_recv_slot = Some(recv_slot);
                }
                let _ = nexus_abi::cap_close(bnd_rsp_updated);

                // Allow updated to call keystored for signature verification.
                let send_slot = transfer(key_req, Rights::SEND, "keystored send");
                let recv_slot = transfer(key_rsp, Rights::RECV, "keystored recv");
                if let (Some(send_slot), Some(recv_slot)) = (send_slot, recv_slot) {
                    chan.key_send_slot = Some(send_slot);
                    chan.key_recv_slot = Some(recv_slot);
                }

                // Allow updated to call statefsd for persistence.
                let send_slot = transfer(state_req, Rights::SEND, "statefsd send");
                if let Some(send_slot) = send_slot {
                    chan.state_send_slot = Some(send_slot);
                    debug_write_bytes(b"init: updated statefsd send slot=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_byte(b'\n');
                }

                // Provide a reply inbox for CAP_MOVE reply routing (used by log sinks).
                let reply_ep =
                    nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, 8)
                        .map_err(InitError::Abi)?;
                let reply_recv_slot = transfer(reply_ep, Rights::RECV, "reply recv");
                let reply_send_slot = transfer(reply_ep, Rights::SEND, "reply send");
                if let (Some(reply_recv_slot), Some(reply_send_slot)) =
                    (reply_recv_slot, reply_send_slot)
                {
                    chan.reply_recv_slot = Some(reply_recv_slot);
                    chan.reply_send_slot = Some(reply_send_slot);
                    chan.state_recv_slot = Some(reply_recv_slot);
                    debug_write_bytes(b"init: updated reply recv slot=0x");
                    debug_write_hex(reply_recv_slot as usize);
                    debug_write_byte(b'\n');
                    debug_write_bytes(b"init: updated reply send slot=0x");
                    debug_write_hex(reply_send_slot as usize);
                    debug_write_byte(b'\n');
                }
                let _ = nexus_abi::cap_close(reply_ep);

                // TASK-0006: allow updated to send structured logs to logd via CAP_MOVE (reply inbox).
                if let Some(req) = log_req {
                    if let Some(send_slot) = transfer(req, Rights::SEND, "logd send") {
                        chan.log_send_slot = Some(send_slot);
                        if let Some(reply_recv_slot) = reply_recv_slot {
                            chan.log_recv_slot = Some(reply_recv_slot);
                        }
                    }
                }
            }
            "samgrd" => {
                let recv_slot =
                    nexus_abi::cap_transfer(pid, sam_req, Rights::RECV).map_err(InitError::Abi)?;
                let send_slot =
                    nexus_abi::cap_transfer(pid, sam_rsp, Rights::SEND).map_err(InitError::Abi)?;
                chan.sam_send_slot = Some(send_slot);
                chan.sam_recv_slot = Some(recv_slot);
                debug_write_bytes(b"init: samgrd slots recv=0x");
                debug_write_hex(recv_slot as usize);
                debug_write_bytes(b" send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_byte(b'\n');

                // Provide a reply inbox for CAP_MOVE reply routing (used by log sinks).
                let reply_ep =
                    nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, 8)
                        .map_err(InitError::Abi)?;
                let reply_recv_slot =
                    nexus_abi::cap_transfer(pid, reply_ep, Rights::RECV).map_err(InitError::Abi)?;
                let reply_send_slot =
                    nexus_abi::cap_transfer(pid, reply_ep, Rights::SEND).map_err(InitError::Abi)?;
                chan.reply_recv_slot = Some(reply_recv_slot);
                chan.reply_send_slot = Some(reply_send_slot);
                let _ = nexus_abi::cap_close(reply_ep);

                // TASK-0006: allow samgrd to send structured logs to logd via CAP_MOVE (reply inbox).
                if let Some(req) = log_req {
                    let send_slot =
                        nexus_abi::cap_transfer(pid, req, Rights::SEND).map_err(InitError::Abi)?;
                    chan.log_send_slot = Some(send_slot);
                    chan.log_recv_slot = Some(reply_recv_slot);
                }
            }
            "execd" => {
                let recv_slot =
                    nexus_abi::cap_transfer(pid, exe_req, Rights::RECV).map_err(InitError::Abi)?;
                let send_slot =
                    nexus_abi::cap_transfer(pid, exe_rsp, Rights::SEND).map_err(InitError::Abi)?;
                chan.exe_send_slot = Some(send_slot);
                chan.exe_recv_slot = Some(recv_slot);

                // Reply inbox: provide both RECV (stay with execd) and SEND (to be moved to servers).
                let reply_recv_slot = nexus_abi::cap_transfer(pid, execd_reply_ep, Rights::RECV)
                    .map_err(InitError::Abi)?;
                let reply_send_slot = nexus_abi::cap_transfer(pid, execd_reply_ep, Rights::SEND)
                    .map_err(InitError::Abi)?;
                chan.reply_recv_slot = Some(reply_recv_slot);
                chan.reply_send_slot = Some(reply_send_slot);
                let _ = nexus_abi::cap_close(execd_reply_ep);
                debug_write_bytes(b"init: execd reply slots recv=0x");
                debug_write_hex(reply_recv_slot as usize);
                debug_write_bytes(b" send=0x");
                debug_write_hex(reply_send_slot as usize);
                debug_write_byte(b'\n');

                // Optional: allow execd to send crash reports to logd via CAP_MOVE (reply inbox).
                if let Some(req) = log_req {
                    let send_slot =
                        nexus_abi::cap_transfer(pid, req, Rights::SEND).map_err(InitError::Abi)?;
                    chan.log_send_slot = Some(send_slot);
                    chan.log_recv_slot = Some(reply_recv_slot);
                    debug_write_bytes(b"init: execd logd slots send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(reply_recv_slot as usize);
                    debug_write_byte(b'\n');
                }
            }
            "keystored" => {
                // #region agent log (keystored arm entry)
                debug_write_bytes(b"init: ks arm\n");
                // #endregion agent log
                // #region agent log (keystored wire-up tracing)
                debug_write_bytes(b"init: wire keystored xfer key_req RECV cap=0x");
                debug_write_hex(key_req as usize);
                debug_write_byte(b'\n');
                // #endregion agent log
                let recv_slot = match nexus_abi::cap_transfer(pid, key_req, Rights::RECV) {
                    Ok(slot) => slot,
                    Err(e) => {
                        // #region agent log (keystored wire-up error)
                        debug_write_bytes(b"init: wire keystored xfer key_req err=abi:");
                        debug_write_str(abi_error_label(e.clone()));
                        debug_write_byte(b'\n');
                        // #endregion agent log
                        return Err(InitError::Abi(e));
                    }
                };

                // #region agent log (keystored wire-up tracing)
                debug_write_bytes(b"init: wire keystored xfer key_rsp SEND cap=0x");
                debug_write_hex(key_rsp as usize);
                debug_write_byte(b'\n');
                // #endregion agent log
                let send_slot = match nexus_abi::cap_transfer(pid, key_rsp, Rights::SEND) {
                    Ok(slot) => slot,
                    Err(e) => {
                        // #region agent log (keystored wire-up error)
                        debug_write_bytes(b"init: wire keystored xfer key_rsp err=abi:");
                        debug_write_str(abi_error_label(e.clone()));
                        debug_write_byte(b'\n');
                        // #endregion agent log
                        return Err(InitError::Abi(e));
                    }
                };
                chan.key_send_slot = Some(send_slot);
                chan.key_recv_slot = Some(recv_slot);

                // Provide a reply inbox for CAP_MOVE reply routing (used by statefsd + log sinks).
                // #region agent log (keystored reply-inbox create)
                debug_write_bytes(b"init: wire keystored create reply_ep\n");
                // #endregion agent log
                let reply_ep =
                    match nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, 8) {
                        Ok(slot) => slot,
                        Err(e) => {
                            // #region agent log (keystored wire-up error)
                            debug_write_bytes(b"init: wire keystored create reply_ep err=abi:");
                            debug_write_str(abi_error_label(e.clone()));
                            debug_write_byte(b'\n');
                            // #endregion agent log
                            return Err(InitError::Abi(e));
                        }
                    };

                // #region agent log (keystored reply-inbox transfer)
                debug_write_bytes(b"init: wire keystored xfer reply_ep RECV cap=0x");
                debug_write_hex(reply_ep as usize);
                debug_write_byte(b'\n');
                // #endregion agent log
                let reply_recv_slot = match nexus_abi::cap_transfer(pid, reply_ep, Rights::RECV) {
                    Ok(slot) => slot,
                    Err(e) => {
                        // #region agent log (keystored wire-up error)
                        debug_write_bytes(b"init: wire keystored xfer reply_ep RECV err=abi:");
                        debug_write_str(abi_error_label(e.clone()));
                        debug_write_byte(b'\n');
                        // #endregion agent log
                        return Err(InitError::Abi(e));
                    }
                };
                // #region agent log (keystored reply-inbox transfer)
                debug_write_bytes(b"init: wire keystored xfer reply_ep SEND cap=0x");
                debug_write_hex(reply_ep as usize);
                debug_write_byte(b'\n');
                // #endregion agent log
                let reply_send_slot = match nexus_abi::cap_transfer(pid, reply_ep, Rights::SEND) {
                    Ok(slot) => slot,
                    Err(e) => {
                        // #region agent log (keystored wire-up error)
                        debug_write_bytes(b"init: wire keystored xfer reply_ep SEND err=abi:");
                        debug_write_str(abi_error_label(e.clone()));
                        debug_write_byte(b'\n');
                        // #endregion agent log
                        return Err(InitError::Abi(e));
                    }
                };
                chan.reply_recv_slot = Some(reply_recv_slot);
                chan.reply_send_slot = Some(reply_send_slot);
                let _ = nexus_abi::cap_close(reply_ep);

                // statefsd SEND cap + use reply inbox for responses
                // #region agent log (keystored statefsd send cap)
                debug_write_bytes(b"init: wire keystored xfer state_req SEND cap=0x");
                debug_write_hex(state_req as usize);
                debug_write_byte(b'\n');
                // #endregion agent log
                let send_slot = match nexus_abi::cap_transfer(pid, state_req, Rights::SEND) {
                    Ok(slot) => slot,
                    Err(e) => {
                        // #region agent log (keystored wire-up error)
                        debug_write_bytes(b"init: wire keystored xfer state_req err=abi:");
                        debug_write_str(abi_error_label(e.clone()));
                        debug_write_byte(b'\n');
                        // #endregion agent log
                        return Err(InitError::Abi(e));
                    }
                };
                chan.state_send_slot = Some(send_slot);
                chan.state_recv_slot = Some(reply_recv_slot);

                if let Some(req) = log_req {
                    let send_slot =
                        nexus_abi::cap_transfer(pid, req, Rights::SEND).map_err(InitError::Abi)?;
                    chan.log_send_slot = Some(send_slot);
                    chan.log_recv_slot = Some(reply_recv_slot);
                }

                // Allow keystored to call policyd (reply via CAP_MOVE/@reply).
                // #region agent log (keystored policyd send cap)
                debug_write_bytes(b"init: wire keystored xfer pol_req SEND cap=0x");
                debug_write_hex(pol_req as usize);
                debug_write_byte(b'\n');
                // #endregion agent log
                let send_slot = match nexus_abi::cap_transfer(pid, pol_req, Rights::SEND) {
                    Ok(slot) => slot,
                    Err(e) => {
                        // #region agent log (keystored wire-up error)
                        debug_write_bytes(b"init: wire keystored xfer pol_req err=abi:");
                        debug_write_str(abi_error_label(e.clone()));
                        debug_write_byte(b'\n');
                        // #endregion agent log
                        return Err(InitError::Abi(e));
                    }
                };
                chan.pol_send_slot = Some(send_slot);
                chan.pol_recv_slot = Some(reply_recv_slot);

                // Allow keystored to send entropy requests to rngd (replies via CAP_MOVE/@reply).
                // #region agent log (keystored rngd send cap)
                debug_write_bytes(b"init: wire keystored xfer rng_req SEND cap=0x");
                debug_write_hex(rng_req as usize);
                debug_write_byte(b'\n');
                // #endregion agent log
                let send_slot = match nexus_abi::cap_transfer(pid, rng_req, Rights::SEND) {
                    Ok(slot) => slot,
                    Err(e) => {
                        // #region agent log (keystored wire-up error)
                        debug_write_bytes(b"init: wire keystored xfer rng_req err=abi:");
                        debug_write_str(abi_error_label(e.clone()));
                        debug_write_byte(b'\n');
                        // #endregion agent log
                        return Err(InitError::Abi(e));
                    }
                };
                chan.rng_send_slot = Some(send_slot);
                // Use reply inbox recv slot for routing responses (CAP_MOVE replies land here).
                chan.rng_recv_slot = Some(reply_recv_slot);
            }
            "statefsd" => {
                let recv_slot = nexus_abi::cap_transfer(pid, state_req, Rights::RECV)
                    .map_err(InitError::Abi)?;
                let send_slot = nexus_abi::cap_transfer(pid, state_rsp, Rights::SEND)
                    .map_err(InitError::Abi)?;
                chan.state_send_slot = Some(send_slot);
                chan.state_recv_slot = Some(recv_slot);
                debug_write_bytes(b"init: statefsd slots recv=0x");
                debug_write_hex(recv_slot as usize);
                debug_write_bytes(b" send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_byte(b'\n');

                // Provide a reply inbox for CAP_MOVE reply routing (policyd checks, logd).
                let reply_ep =
                    nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, 8)
                        .map_err(InitError::Abi)?;
                let reply_recv_slot =
                    nexus_abi::cap_transfer(pid, reply_ep, Rights::RECV).map_err(InitError::Abi)?;
                let reply_send_slot =
                    nexus_abi::cap_transfer(pid, reply_ep, Rights::SEND).map_err(InitError::Abi)?;
                chan.reply_recv_slot = Some(reply_recv_slot);
                chan.reply_send_slot = Some(reply_send_slot);
                let _ = nexus_abi::cap_close(reply_ep);

                // Allow statefsd to call policyd (reply via CAP_MOVE/@reply).
                let send_slot =
                    nexus_abi::cap_transfer(pid, pol_req, Rights::SEND).map_err(InitError::Abi)?;
                chan.pol_send_slot = Some(send_slot);
                chan.pol_recv_slot = Some(reply_recv_slot);
            }
            "rngd" => {
                // Server-side endpoints for rngd.
                let recv_slot =
                    nexus_abi::cap_transfer(pid, rng_req, Rights::RECV).map_err(InitError::Abi)?;
                let send_slot =
                    nexus_abi::cap_transfer(pid, rng_rsp, Rights::SEND).map_err(InitError::Abi)?;
                chan.rng_send_slot = Some(send_slot);
                chan.rng_recv_slot = Some(recv_slot);

                // Provide a reply inbox for CAP_MOVE reply routing (used by clients).
                let reply_ep =
                    nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, 8)
                        .map_err(InitError::Abi)?;
                let reply_recv_slot =
                    nexus_abi::cap_transfer(pid, reply_ep, Rights::RECV).map_err(InitError::Abi)?;
                let reply_send_slot =
                    nexus_abi::cap_transfer(pid, reply_ep, Rights::SEND).map_err(InitError::Abi)?;
                chan.reply_recv_slot = Some(reply_recv_slot);
                chan.reply_send_slot = Some(reply_send_slot);
                let _ = nexus_abi::cap_close(reply_ep);

                if let Some(req) = log_req {
                    let send_slot =
                        nexus_abi::cap_transfer(pid, req, Rights::SEND).map_err(InitError::Abi)?;
                    chan.log_send_slot = Some(send_slot);
                    chan.log_recv_slot = Some(reply_recv_slot);
                }

                // Allow rngd to call policyd (reply via CAP_MOVE/@reply).
                let send_slot =
                    nexus_abi::cap_transfer(pid, pol_req, Rights::SEND).map_err(InitError::Abi)?;
                chan.pol_send_slot = Some(send_slot);
                chan.pol_recv_slot = Some(reply_recv_slot);
            }
            "timed" => {
                let recv_slot = nexus_abi::cap_transfer(pid, timed_req, Rights::RECV)
                    .map_err(InitError::Abi)?;
                let send_slot = nexus_abi::cap_transfer(pid, timed_rsp, Rights::SEND)
                    .map_err(InitError::Abi)?;
                chan.timed_send_slot = Some(send_slot);
                chan.timed_recv_slot = Some(recv_slot);
            }
            "hidrawd" => {
                let send_slot = nexus_abi::cap_transfer(pid, input_req, Rights::SEND)
                    .map_err(InitError::Abi)?;
                let recv_slot = nexus_abi::cap_transfer(pid, input_rsp, Rights::RECV)
                    .map_err(InitError::Abi)?;
                chan.input_send_slot = Some(send_slot);
                chan.input_recv_slot = Some(recv_slot);
                debug_write_bytes(b"init: hidrawd inputd slots send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_bytes(b" recv=0x");
                debug_write_hex(recv_slot as usize);
                debug_write_byte(b'\n');
            }
            "gpud" => {
                let recv_slot = try_transfer(pid, gpud_req, Rights::RECV, "gpud", "RECV");
                let send_slot = try_transfer(pid, gpud_rsp, Rights::SEND, "gpud", "SEND");
                if let (Some(recv), Some(send)) = (recv_slot, send_slot) {
                    chan.gpud_send_slot = Some(send);
                    chan.gpud_recv_slot = Some(recv);
                    debug_write_bytes(b"init: gpud slots recv=0x");
                    debug_write_hex(recv as usize);
                    debug_write_bytes(b" send=0x");
                    debug_write_hex(send as usize);
                    debug_write_byte(b'\n');
                }
            }
            "windowd" => {
                // Already priority-wired before MMIO grants — skip re-wiring.
                if chan.window_send_slot.is_some() && chan.window_recv_slot.is_some() {
                    debug_write_bytes(b"init: windowd already priority-wired, skip\n");
                    // Still need gpud caps.
                    let gpud_send_slot =
                        try_transfer(pid, gpud_req, Rights::SEND, "windowd->gpud", "SEND");
                    let gpud_recv_slot =
                        try_transfer(pid, gpud_rsp, Rights::RECV, "windowd->gpud", "RECV");
                    if let (Some(gpud_send), Some(gpud_recv)) = (gpud_send_slot, gpud_recv_slot) {
                        chan.gpud_send_slot = Some(gpud_send);
                        chan.gpud_recv_slot = Some(gpud_recv);
                    }
                    // RFC-0065 dynamic Apps menu: provision the registry reply-inbox
                    // + bundlemgrd route caps HERE — AFTER the gpud caps, so gpud
                    // keeps the hardcoded fallback slots (5/6) the present handoff
                    // relies on. (Doing this in the priority-wire block shifted gpud
                    // to 8/9 → present handoff `kernel-permission-denied`.)
                    provision_windowd_registry_route(ENDPOINT_FACTORY_CAP_SLOT, pid, bnd_req, chan);
                    continue;
                }
                let recv_slot = nexus_abi::cap_transfer(pid, window_req, Rights::RECV)
                    .map_err(InitError::Abi)?;
                let send_slot = nexus_abi::cap_transfer(pid, window_rsp, Rights::SEND)
                    .map_err(InitError::Abi)?;
                chan.window_send_slot = Some(send_slot);
                chan.window_recv_slot = Some(recv_slot);
                // gpud may have crashed — graceful transfer
                let gpud_send_slot =
                    try_transfer(pid, gpud_req, Rights::SEND, "windowd->gpud", "SEND");
                let gpud_recv_slot =
                    try_transfer(pid, gpud_rsp, Rights::RECV, "windowd->gpud", "RECV");
                if let (Some(gpud_send), Some(gpud_recv)) = (gpud_send_slot, gpud_recv_slot) {
                    chan.gpud_send_slot = Some(gpud_send);
                    chan.gpud_recv_slot = Some(gpud_recv);
                }
                // Registry reply-inbox + bundlemgrd route AFTER gpud (slot-order
                // contract — see the skip path above).
                provision_windowd_registry_route(ENDPOINT_FACTORY_CAP_SLOT, pid, bnd_req, chan);
                debug_write_bytes(b"init: windowd slots recv=0x");
                debug_write_hex(recv_slot as usize);
                debug_write_bytes(b" send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_byte(b'\n');
                if let (Some(gpud_send), Some(gpud_recv)) = (gpud_send_slot, gpud_recv_slot) {
                    debug_write_bytes(b"init: windowd gpud slots send=0x");
                    debug_write_hex(gpud_send as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(gpud_recv as usize);
                    debug_write_byte(b'\n');
                }
            }
            "inputd" => {
                if chan.input_send_slot.is_some() && chan.input_recv_slot.is_some() {
                    debug_write_bytes(b"init: inputd already priority-wired, skip\n");
                    // Still need windowd route for visible-state push.
                    let window_send_slot =
                        try_transfer(pid, window_req, Rights::SEND, "inputd->windowd", "SEND");
                    let window_recv_slot =
                        try_transfer(pid, window_rsp, Rights::RECV, "inputd->windowd", "RECV");
                    if let (Some(window_send), Some(window_recv)) =
                        (window_send_slot, window_recv_slot)
                    {
                        chan.window_send_slot = Some(window_send);
                        chan.window_recv_slot = Some(window_recv);
                        debug_write_bytes(b"init: inputd windowd slots send=0x");
                        debug_write_hex(window_send as usize);
                        debug_write_bytes(b" recv=0x");
                        debug_write_hex(window_recv as usize);
                        debug_write_byte(b'\n');
                    }
                    continue;
                }
                let recv_slot = nexus_abi::cap_transfer(pid, input_req, Rights::RECV)
                    .map_err(InitError::Abi)?;
                let send_slot = nexus_abi::cap_transfer(pid, input_rsp, Rights::SEND)
                    .map_err(InitError::Abi)?;
                chan.input_send_slot = Some(send_slot);
                chan.input_recv_slot = Some(recv_slot);
                let window_send_slot = nexus_abi::cap_transfer(pid, window_req, Rights::SEND)
                    .map_err(InitError::Abi)?;
                let window_recv_slot = nexus_abi::cap_transfer(pid, window_rsp, Rights::RECV)
                    .map_err(InitError::Abi)?;
                chan.window_send_slot = Some(window_send_slot);
                chan.window_recv_slot = Some(window_recv_slot);
                debug_write_bytes(b"init: inputd slots recv=0x");
                debug_write_hex(recv_slot as usize);
                debug_write_bytes(b" send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_byte(b'\n');
                debug_write_bytes(b"init: inputd windowd slots send=0x");
                debug_write_hex(window_send_slot as usize);
                debug_write_bytes(b" recv=0x");
                debug_write_hex(window_recv_slot as usize);
                debug_write_byte(b'\n');
            }
            "metricsd" => {
                if let (Some(req), Some(rsp)) = (metrics_req, metrics_rsp) {
                    let recv_slot =
                        nexus_abi::cap_transfer(pid, req, Rights::RECV).map_err(InitError::Abi)?;
                    let send_slot =
                        nexus_abi::cap_transfer(pid, rsp, Rights::SEND).map_err(InitError::Abi)?;
                    chan.metrics_send_slot = Some(send_slot);
                    chan.metrics_recv_slot = Some(recv_slot);
                    debug_write_bytes(b"init: metricsd slots recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_bytes(b" send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_byte(b'\n');
                }

                // Provide a reply inbox for CAP_MOVE reply routing (used by log sink).
                let reply_ep =
                    nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, 8)
                        .map_err(InitError::Abi)?;
                let reply_recv_slot =
                    nexus_abi::cap_transfer_to_slot(pid, reply_ep, Rights::RECV, 0x05)
                        .map_err(InitError::Abi)?;
                let reply_send_slot =
                    nexus_abi::cap_transfer_to_slot(pid, reply_ep, Rights::SEND, 0x06)
                        .map_err(InitError::Abi)?;
                chan.reply_recv_slot = Some(reply_recv_slot);
                chan.reply_send_slot = Some(reply_send_slot);
                let _ = nexus_abi::cap_close(reply_ep);

                // Allow metricsd to export snapshots/spans via nexus-log -> logd sink.
                if let Some(req) = log_req {
                    let send_slot = nexus_abi::cap_transfer_to_slot(pid, req, Rights::SEND, 0x08)
                        .map_err(InitError::Abi)?;
                    chan.log_send_slot = Some(send_slot);
                    chan.log_recv_slot = Some(reply_recv_slot);
                    debug_write_bytes(b"init: metricsd logd slots send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(reply_recv_slot as usize);
                    debug_write_byte(b'\n');
                }

                // Allow metricsd retention writer to call statefsd via CAP_MOVE/@reply.
                let send_slot = nexus_abi::cap_transfer_to_slot(pid, state_req, Rights::SEND, 0x07)
                    .map_err(InitError::Abi)?;
                chan.state_send_slot = Some(send_slot);
                chan.state_recv_slot = Some(reply_recv_slot);
            }
            "logd" => {
                if let (Some(req), Some(rsp)) = (log_req, log_rsp) {
                    let recv_slot =
                        nexus_abi::cap_transfer(pid, req, Rights::RECV).map_err(InitError::Abi)?;
                    let send_slot =
                        nexus_abi::cap_transfer(pid, rsp, Rights::SEND).map_err(InitError::Abi)?;
                    chan.log_send_slot = Some(send_slot);
                    chan.log_recv_slot = Some(recv_slot);
                    debug_write_bytes(b"init: logd slots recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_bytes(b" send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_byte(b'\n');
                }
            }
            "selftest-client" => {
                let send_slot =
                    nexus_abi::cap_transfer(pid, vfs_req, Rights::SEND).map_err(InitError::Abi)?;
                let recv_slot =
                    nexus_abi::cap_transfer(pid, vfs_rsp, Rights::RECV).map_err(InitError::Abi)?;
                chan.vfs_send_slot = Some(send_slot);
                chan.vfs_recv_slot = Some(recv_slot);
                debug_write_bytes(b"init: selftest vfsd slots send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_bytes(b" recv=0x");
                debug_write_hex(recv_slot as usize);
                debug_write_byte(b'\n');
                let send_slot =
                    nexus_abi::cap_transfer(pid, pkg_req, Rights::SEND).map_err(InitError::Abi)?;
                let recv_slot =
                    nexus_abi::cap_transfer(pid, pkg_rsp, Rights::RECV).map_err(InitError::Abi)?;
                chan.pkg_send_slot = Some(send_slot);
                chan.pkg_recv_slot = Some(recv_slot);
                let send_slot =
                    nexus_abi::cap_transfer(pid, pol_req, Rights::SEND).map_err(InitError::Abi)?;
                let recv_slot =
                    nexus_abi::cap_transfer(pid, pol_rsp, Rights::RECV).map_err(InitError::Abi)?;
                chan.pol_send_slot = Some(send_slot);
                chan.pol_recv_slot = Some(recv_slot);
                debug_write_bytes(b"init: selftest policyd slots send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_bytes(b" recv=0x");
                debug_write_hex(recv_slot as usize);
                debug_write_byte(b'\n');
                let send_slot =
                    nexus_abi::cap_transfer(pid, bnd_req, Rights::SEND).map_err(InitError::Abi)?;
                let recv_slot =
                    nexus_abi::cap_transfer(pid, bnd_rsp, Rights::RECV).map_err(InitError::Abi)?;
                chan.bnd_send_slot = Some(send_slot);
                chan.bnd_recv_slot = Some(recv_slot);
                debug_write_bytes(b"init: selftest bundlemgrd slots send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_bytes(b" recv=0x");
                debug_write_hex(recv_slot as usize);
                debug_write_byte(b'\n');
                let send_slot =
                    nexus_abi::cap_transfer(pid, upd_req, Rights::SEND).map_err(InitError::Abi)?;
                let recv_slot =
                    nexus_abi::cap_transfer(pid, upd_rsp, Rights::RECV).map_err(InitError::Abi)?;
                chan.upd_send_slot = Some(send_slot);
                chan.upd_recv_slot = Some(recv_slot);
                debug_write_bytes(b"init: selftest updated slots send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_bytes(b" recv=0x");
                debug_write_hex(recv_slot as usize);
                debug_write_byte(b'\n');
                let send_slot =
                    nexus_abi::cap_transfer(pid, sam_req, Rights::SEND).map_err(InitError::Abi)?;
                let recv_slot =
                    nexus_abi::cap_transfer(pid, sam_rsp, Rights::RECV).map_err(InitError::Abi)?;
                chan.sam_send_slot = Some(send_slot);
                chan.sam_recv_slot = Some(recv_slot);
                debug_write_bytes(b"init: selftest samgrd slots send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_bytes(b" recv=0x");
                debug_write_hex(recv_slot as usize);
                debug_write_byte(b'\n');
                let send_slot =
                    nexus_abi::cap_transfer(pid, exe_req, Rights::SEND).map_err(InitError::Abi)?;
                let recv_slot =
                    nexus_abi::cap_transfer(pid, exe_rsp, Rights::RECV).map_err(InitError::Abi)?;
                chan.exe_send_slot = Some(send_slot);
                chan.exe_recv_slot = Some(recv_slot);
                debug_write_bytes(b"init: selftest execd slots send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_bytes(b" recv=0x");
                debug_write_hex(recv_slot as usize);
                debug_write_byte(b'\n');
                let send_slot =
                    nexus_abi::cap_transfer(pid, key_req, Rights::SEND).map_err(InitError::Abi)?;
                let recv_slot =
                    nexus_abi::cap_transfer(pid, key_rsp, Rights::RECV).map_err(InitError::Abi)?;
                chan.key_send_slot = Some(send_slot);
                chan.key_recv_slot = Some(recv_slot);
                debug_write_bytes(b"init: selftest keystored slots send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_bytes(b" recv=0x");
                debug_write_hex(recv_slot as usize);
                debug_write_byte(b'\n');

                let send_slot = nexus_abi::cap_transfer(pid, state_req, Rights::SEND)
                    .map_err(InitError::Abi)?;
                let recv_slot = nexus_abi::cap_transfer(pid, state_rsp, Rights::RECV)
                    .map_err(InitError::Abi)?;
                chan.state_send_slot = Some(send_slot);
                chan.state_recv_slot = Some(recv_slot);
                debug_write_bytes(b"init: selftest statefsd slots send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_bytes(b" recv=0x");
                debug_write_hex(recv_slot as usize);
                debug_write_byte(b'\n');

                if let (Some(req), Some(rsp)) = (log_req, log_rsp) {
                    let send_slot =
                        nexus_abi::cap_transfer(pid, req, Rights::SEND).map_err(InitError::Abi)?;
                    let recv_slot =
                        nexus_abi::cap_transfer(pid, rsp, Rights::RECV).map_err(InitError::Abi)?;
                    chan.log_send_slot = Some(send_slot);
                    chan.log_recv_slot = Some(recv_slot);
                    debug_write_bytes(b"init: selftest logd slots send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_byte(b'\n');
                }
                if let (Some(req), Some(rsp)) = (metrics_req, metrics_rsp) {
                    let send_slot = nexus_abi::cap_transfer_to_slot(pid, req, Rights::SEND, 0x21)
                        .map_err(InitError::Abi)?;
                    let recv_slot = nexus_abi::cap_transfer_to_slot(pid, rsp, Rights::RECV, 0x22)
                        .map_err(InitError::Abi)?;
                    chan.metrics_send_slot = Some(send_slot);
                    chan.metrics_recv_slot = Some(recv_slot);
                    debug_write_bytes(b"init: selftest metricsd slots send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_byte(b'\n');
                }

                // Reply inbox: provide both RECV (stay with client) and SEND (to be moved to servers).
                let reply_recv_slot =
                    nexus_abi::cap_transfer(pid, reply_ep, Rights::RECV).map_err(InitError::Abi)?;
                let reply_send_slot =
                    nexus_abi::cap_transfer(pid, reply_ep, Rights::SEND).map_err(InitError::Abi)?;
                chan.reply_recv_slot = Some(reply_recv_slot);
                chan.reply_send_slot = Some(reply_send_slot);
                debug_write_bytes(b"init: selftest reply slots send=0x");
                debug_write_hex(reply_send_slot as usize);
                debug_write_bytes(b" recv=0x");
                debug_write_hex(reply_recv_slot as usize);
                debug_write_byte(b'\n');

                let send_slot = nexus_abi::cap_transfer(pid, input_req, Rights::SEND)
                    .map_err(InitError::Abi)?;
                chan.input_send_slot = Some(send_slot);
                chan.input_recv_slot = Some(reply_recv_slot);
                debug_write_bytes(b"init: selftest inputd slots send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_bytes(b" recv=0x");
                debug_write_hex(reply_recv_slot as usize);
                debug_write_byte(b'\n');

                // Allow selftest-client to send requests to netstackd.
                let send_slot =
                    nexus_abi::cap_transfer(pid, net_req, Rights::SEND).map_err(InitError::Abi)?;
                let recv_slot = nexus_abi::cap_transfer(pid, net_selftest_rsp, Rights::RECV)
                    .map_err(InitError::Abi)?;
                chan.net_send_slot = Some(send_slot);
                chan.net_recv_slot = Some(recv_slot);

                // Allow selftest-client to send requests to dsoftbusd (TASK-0005 remote proxy proof).
                let send_slot = nexus_abi::cap_transfer(pid, dsoft_req, Rights::SEND)
                    .map_err(InitError::Abi)?;
                let recv_slot = nexus_abi::cap_transfer(pid, dsoft_rsp, Rights::RECV)
                    .map_err(InitError::Abi)?;
                chan.dsoft_send_slot = Some(send_slot);
                chan.dsoft_recv_slot = Some(recv_slot);

                // Allow selftest-client to send requests to rngd and receive direct replies.
                let send_slot =
                    nexus_abi::cap_transfer(pid, rng_req, Rights::SEND).map_err(InitError::Abi)?;
                let recv_slot =
                    nexus_abi::cap_transfer(pid, rng_rsp, Rights::RECV).map_err(InitError::Abi)?;
                chan.rng_send_slot = Some(send_slot);
                chan.rng_recv_slot = Some(recv_slot);
                debug_write_bytes(b"init: selftest rngd slots send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_bytes(b" recv=0x");
                debug_write_hex(recv_slot as usize);
                debug_write_byte(b'\n');

                // Allow selftest-client to send requests to timed and receive direct replies.
                let send_slot = nexus_abi::cap_transfer(pid, timed_req, Rights::SEND)
                    .map_err(InitError::Abi)?;
                let recv_slot = nexus_abi::cap_transfer(pid, timed_rsp, Rights::RECV)
                    .map_err(InitError::Abi)?;
                chan.timed_send_slot = Some(send_slot);
                chan.timed_recv_slot = Some(recv_slot);
            }
            // RFC-0066 Phase 3 (incremental): services whose wiring is just "a
            // server endpoint" are provisioned **data-driven** from the declarative
            // `ServiceSpec` (host-tested) via the generic helper below — not a
            // bespoke arm. abilitymgr is the first such service; the complex
            // services keep their bespoke arms until they are migrated too.
            name if crate::service_topology::exposes_server(name.as_bytes())
                && !is_bespoke_wired(name) =>
            {
                use crate::service_topology::ServiceId;
                provision_server_endpoint(ENDPOINT_FACTORY_CAP_SLOT, pid, name.as_bytes());

                // RFC-0066 P3: provision this service's outbound routes **from its
                // declarative `ServiceSpec.routes_to`** (not a bespoke arm). Each
                // route = a CAP_MOVE reply inbox + a send cap to the target's
                // request endpoint; the existing `build_route_table` fields register
                // it. Best-effort: a failure leaves the route unwired, never bricks.
                if let Some(spec) = crate::service_topology::spec_for(name.as_bytes()) {
                    if !spec.routes_to.is_empty() && spec.reply_inbox {
                        if let Ok(reply_ep) =
                            nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, 8)
                        {
                            let rr = nexus_abi::cap_transfer(pid, reply_ep, Rights::RECV);
                            let rs = nexus_abi::cap_transfer(pid, reply_ep, Rights::SEND);
                            let _ = nexus_abi::cap_close(reply_ep);
                            if let (Ok(reply_recv), Ok(reply_send)) = (rr, rs) {
                                chan.reply_recv_slot = Some(reply_recv);
                                chan.reply_send_slot = Some(reply_send);
                                for &to in spec.routes_to {
                                    // Bridge ServiceId → the target's request cap +
                                    // the matching channel field (uniform routing is
                                    // a later refactor; this reuses what exists).
                                    match to {
                                        ServiceId::Bundlemgrd => {
                                            if let Ok(s) =
                                                nexus_abi::cap_transfer(pid, bnd_req, Rights::SEND)
                                            {
                                                chan.bnd_send_slot = Some(s);
                                                chan.bnd_recv_slot = Some(reply_recv);
                                                debug_write_bytes(b"init: ");
                                                debug_write_bytes(name.as_bytes());
                                                debug_write_bytes(b" route->bundlemgrd ok\n");
                                            }
                                        }
                                        // execd route wires with the launch path (later).
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Cumulative boot elapsed just before the display-chain deferred resume. The gap from
    // `grants_done_ms` is the per-service cap-wiring phase; the gap to `total_ms` is the
    // display resume + the updated/bundlemgr OTA handshake tail.
    let wiring_done_ms = boot_span.elapsed_ms();

    // Resume display + input device-driver services after MMIO grants and route wiring.
    // gpud FIRST: the GL-scanout display handoff (OP_SET_FRAMEBUFFER_VMO →
    // scanout) must be ready before windowd presents, or the window stays black.
    // inputd is resumed right after windowd; hidrawd LAST (after inputd) so it finds its
    // virtio-input MMIO already granted and inputd's route already wired — it opens its
    // devices + binds its IRQ immediately, with no startup busy-yield (see `entry_to_ready_ms`).
    for service_name in ["gpud", "windowd", "inputd", "hidrawd"] {
        if let Some(chan) = ctrl_channels.iter().find(|c| c.svc_name == service_name) {
            match nexus_abi::task_resume(chan.pid) {
                Ok(()) => {
                    debug_write_bytes(b"init: deferred resume ");
                    debug_write_str(service_name);
                    debug_write_byte(b'\n');
                }
                Err(e) => {
                    debug_write_bytes(b"init: deferred resume fail svc=");
                    debug_write_str(service_name);
                    debug_write_bytes(b" err=0x");
                    debug_write_hex(e as usize);
                    debug_write_byte(b'\n');
                }
            }
        }
    }

    // Yield after cap distribution so services observe a consistent slot layout.
    let _ = nexus_abi::yield_();

    let mut upd_pending: nexus_ipc::reqrep::FrameStash<8, 16> =
        nexus_ipc::reqrep::FrameStash::new();
    match updated_boot_attempt(&mut upd_pending, upd_req, init_reply_send, pol_ctl_route_rsp) {
        Ok(Some(slot)) => {
            let ok = bundlemgrd_set_active_slot(
                &mut upd_pending,
                bnd_req,
                init_reply_send,
                pol_ctl_route_rsp,
                slot,
            );
            if !ok {
                debug_write_str("init: rollback deferred");
                debug_write_byte(b'\n');
            }
        }
        Ok(None) => {}
        Err(_) => {
            debug_write_str("init: boot attempt fail");
            debug_write_byte(b'\n');
        }
    }

    let route_table = route_builder::build_route_table(&ctrl_channels);
    route_builder::populate_samgrd_registry(init_sam_send, init_sam_recv, &route_table);
    // Boot-timing table (Phase 3): one compact line locating where boot time went. `grant_wait`
    // is the time spent yielding for policyd MMIO grants — the prime "services waiting" suspect.
    let total_ms = boot_span.elapsed_ms();
    let timing = alloc::format!(
        "init: timing spawn_ms={} grants_at_ms={} wiring_at_ms={} total_ms={} (grant_wait_ms={} grants={} wiring_ms={} tail_ms={})",
        spawn_ms,
        grants_done_ms,
        wiring_done_ms,
        total_ms,
        grant_wait_ns.get() / 1_000_000,
        grant_count.get(),
        wiring_done_ms.saturating_sub(grants_done_ms),
        total_ms.saturating_sub(wiring_done_ms)
    );
    debug_write_str(&timing);
    debug_write_byte(b'\n');

    Ok(BootstrapState {
        ctrl_channels,
        route_table,
        pol_ctl_route_req,
        pol_ctl_route_rsp,
        pol_ctl_exec_req,
        pol_ctl_exec_rsp,
        upd_req,
        upd_reply_send: init_reply_send,
        upd_reply_recv: pol_ctl_route_rsp,
        upd_pending,
    })
}

/// Provisions windowd's RFC-0065 dynamic-Apps-menu route caps: a CAP_MOVE reply
/// inbox + a SEND cap to bundlemgrd's request endpoint, so windowd's
/// `route_blocking("bundlemgrd")` / `route_blocking("@reply")` resolve (declared in
/// `service_topology` as Windowd→Bundlemgrd; granted `bundle.query`+`ipc.core` in
/// base.toml). MUST be called AFTER windowd's gpud caps are transferred so the
/// present handoff's hardcoded fallback slots (5/6 = gpud) are not displaced.
/// Best-effort: a failure leaves the route unwired (the menu falls back to its
/// seed), never bricks boot.
fn provision_windowd_registry_route(
    factory_slot: u32,
    pid: u32,
    bnd_req: u32,
    chan: &mut CtrlChannel,
) {
    let Ok(reply_ep) = nexus_abi::ipc_endpoint_create_for(factory_slot, pid, 8) else {
        return;
    };
    let rr = nexus_abi::cap_transfer(pid, reply_ep, Rights::RECV);
    let rs = nexus_abi::cap_transfer(pid, reply_ep, Rights::SEND);
    let _ = nexus_abi::cap_close(reply_ep);
    if let (Ok(reply_recv), Ok(reply_send)) = (rr, rs) {
        chan.reply_recv_slot = Some(reply_recv);
        chan.reply_send_slot = Some(reply_send);
        if let Ok(s) = nexus_abi::cap_transfer(pid, bnd_req, Rights::SEND) {
            chan.bnd_send_slot = Some(s);
            chan.bnd_recv_slot = Some(reply_recv);
            debug_write_bytes(b"init: windowd route->bundlemgrd ok\n");
        }
    }
}

/// `true` if `name` has a bespoke wiring arm in the orchestrator (complex
/// services with routes/reply-inboxes). RFC-0066 Phase 3: services NOT in this set
/// whose `ServiceSpec.exposes_server` is true are provisioned generically from the
/// declarative topology instead of a hand-written arm. As bespoke services are
/// migrated to `ServiceSpec`, they are removed from this set.
fn is_bespoke_wired(name: &str) -> bool {
    matches!(
        name,
        "netstackd"
            | "dsoftbusd"
            | "vfsd"
            | "packagefsd"
            | "policyd"
            | "bundlemgrd"
            | "updated"
            | "samgrd"
            | "execd"
            | "keystored"
            | "statefsd"
            | "rngd"
            | "timed"
            | "hidrawd"
            | "gpud"
            | "windowd"
            | "inputd"
            | "metricsd"
            | "logd"
            | "selftest-client"
    )
}

/// Provisions a plain server endpoint for a service (recv/send land at the
/// deterministic fallback slots 3/4 the service expects), driven by the
/// declarative [`crate::service_topology::ServiceSpec`]. Best-effort: a failure
/// leaves the service unwired rather than aborting init — it must never brick boot.
fn provision_server_endpoint(factory_slot: u32, pid: u32, name: &[u8]) {
    match nexus_abi::ipc_endpoint_create_for(factory_slot, pid, 8) {
        Ok(ep) => {
            let recv = nexus_abi::cap_transfer(pid, ep, Rights::RECV);
            let send = nexus_abi::cap_transfer(pid, ep, Rights::SEND);
            let _ = nexus_abi::cap_close(ep);
            match (recv, send) {
                (Ok(recv_slot), Ok(send_slot)) => {
                    debug_write_bytes(b"init: ");
                    debug_write_bytes(name);
                    debug_write_bytes(b" slots recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_bytes(b" send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_byte(b'\n');
                }
                _ => {
                    debug_write_bytes(b"init: ");
                    debug_write_bytes(name);
                    debug_write_bytes(b" slot xfer skip\n");
                }
            }
        }
        Err(_) => {
            debug_write_bytes(b"init: ");
            debug_write_bytes(name);
            debug_write_bytes(b" endpoint skip\n");
        }
    }
}
