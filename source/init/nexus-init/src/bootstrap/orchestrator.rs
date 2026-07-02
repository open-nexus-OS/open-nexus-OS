// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Service bootstrap orchestrator — extracted from os_payload.rs per RFC-0061.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os)
//! ADR: docs/adr/0017-service-architecture.md
//! RFC: docs/rfcs/RFC-0061-selftest-observer-init-refactoring.md

use crate::bootstrap::diag::{expanded, il, iw};
use crate::bootstrap::route_builder;
use crate::bootstrap::{BootstrapState, CtrlChannel};
use crate::os_payload::*;
use crate::service_topology::ServiceId;
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
    // `ENDPOINT_FACTORY_CAP_SLOT` (init-lite's EndpointFactory cap, slot 1) is a crate const.
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
    // RFC-0068: in interactive boots fold the unconditional `init: start/up X` spawn ladder (~50
    // lines) into ONE `init:spawn N/N <ms>` verdict via the shared SSOT. Proof boots don't fold
    // (`boot_should_fold_verdicts()` is false), so the raw lines stay for verify-uart; a spawn
    // FAILURE is fatal and always prints live.
    let init_fold = nexus_abi::boot_should_fold_verdicts();
    let mut spawn_tally = nexus_event::SpanTally::new();
    // RFC-0068: ONE compact `init_caps` verdict for all cap-wiring diagnostics (`#region agent log` traces —
    // NOT proof markers, not harness-grepped; each precedes a real cap_transfer). The per-SUBJECT
    // detail is recalled at DEBUG time via `NEXUS_LOG_EXPAND=<svc>` (see `iw`/`subject_expanded`),
    // which reveals that subject's init lines together with its own service markers — so the default
    // grid stays compact while debugging stays subject-scoped. Flushed once at the end of bootstrap.
    let mut init_wire = nexus_event::SpanTally::new();
    // RFC-0068: the `lifecycle` group (entry/timing/deferred-resume/probe/rollback) — named for WHAT
    // happens, not the `init` emitter; separate from `init_caps` wiring. `init: ready` stays raw
    // (harness stop marker).
    let mut init_misc = nexus_event::SpanTally::new();
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
        spawn_tally.start(nexus_abi::nsec().unwrap_or(0));
        if !init_fold || expanded("init_spawn") || expanded(image.name) {
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
        }
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
                if image.name == "updated" && iw(&mut init_wire, init_fold, "init:updated") {
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

                let ctrl = CtrlChannel::new(
                    image.name,
                    pid,
                    ctrl_req_parent_slot,
                    ctrl_rsp_parent_slot,
                );
                ctrl_channels.push(ctrl);
                if probes_enabled() {
                    debug_write_bytes(b"!spawn ok pid=0x");
                    debug_write_hex(pid as usize);
                    debug_write_byte(b'\n');
                }
                spawn_tally.record(nexus_event::Status::Ok, nexus_abi::nsec().unwrap_or(0));
                if !init_fold || expanded("init_spawn") || expanded(image.name) {
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
    // RFC-0068: emit the folded spawn-ladder verdict (interactive only; paired with the suppression
    // above so no folded `start/up` line is ever dropped without this verdict).
    if init_fold && !spawn_tally.is_empty() {
        let now = nexus_abi::nsec().unwrap_or(0);
        // Self-contained span (first start → last up), so the duration is the spawn work itself.
        let v = spawn_tally.verdict_self();
        let mut line = [0u8; 96];
        let n = nexus_event::render_verdict_line(&mut line, now, "init_spawn", v);
        let _ = nexus_abi::debug_write(&line[..n]);
    }

    notifier.notify();
    debug_write_str("init: ready");
    debug_write_byte(b'\n');
    debug_write_bytes(b"!init-lite ready\n");
    // Resume all spawned services (except the device drivers) now so policyd can
    // handle MMIO policy checks during the grant phase. IPC wiring happens after grants.
    crate::bootstrap::resume::resume_non_drivers(&ctrl_channels);
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
        if il(&mut init_misc, init_fold, "keystored") {
            debug_write_bytes(b"init: probe key_req self-xfer pid=0x");
            debug_write_hex(me as usize);
            debug_write_bytes(b" cap=0x");
            debug_write_hex(key_req as usize);
            debug_write_byte(b'\n');
        }
        match nexus_abi::cap_transfer(me, key_req, Rights::SEND) {
            Ok(slot) => {
                if il(&mut init_misc, init_fold, "keystored") {
                    debug_write_bytes(b"init: probe key_req self-xfer SEND ok slot=0x");
                    debug_write_hex(slot as usize);
                    debug_write_byte(b'\n');
                }
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
                if il(&mut init_misc, init_fold, "keystored") {
                    debug_write_bytes(b"init: probe key_req self-xfer RECV ok slot=0x");
                    debug_write_hex(slot as usize);
                    debug_write_byte(b'\n');
                }
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

    // Bundle the minted endpoint caps NOW — before the policy-gated grant phase —
    // and distribute every declared service's server pair immediately (RFC-0069
    // phase semantics, task #123 fix): the services' deterministic fallback
    // slots (3/4) must exist BEFORE anything policyd-gated can delay the boot.
    // The historical hazard: a slow policyd stalled the grants, wire_services ran
    // late, and services whose route-probe fell back to the fixed slots hit an
    // EMPTY slot with their first recv — the whole early fleet died and init
    // then wired caps into dead PIDs (the `capability-denied` abort).
    // `wire_services` (after grants) still owns the reply inboxes, routes and
    // the announce markers — byte-identical boot logs.
    let eps = crate::bootstrap::endpoints::Endpoints {
        vfs_req, vfs_rsp, pkg_req, pkg_rsp, pkg_reply_ep, pol_req, pol_rsp, bnd_req, bnd_rsp,
        bnd_rsp_updated, bnd_exe_req, bnd_exe_rsp, upd_req, upd_rsp, sam_req, sam_rsp,
        exe_req, exe_rsp, key_req, key_rsp, state_req, state_rsp,
        rng_req, rng_rsp, timed_req, timed_rsp, window_req, window_rsp, input_req, input_rsp,
        gpud_req, gpud_rsp, net_req, net_rsp, net_selftest_rsp, net_dsoft_rsp, dsoft_req,
        dsoft_rsp, dsoft_reply_ep, execd_reply_ep, reply_ep, log_req, log_rsp, metrics_req,
        metrics_rsp,
    };
    crate::bootstrap::wiring::distribute_server_pairs(&mut ctrl_channels, &eps);

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
            chan.set_recv(
                ServiceId::Policyd,
                nexus_abi::cap_transfer(pid, pol_req_clone, Rights::RECV)
                    .map_err(InitError::Abi)?,
            );
            chan.set_send(
                ServiceId::Policyd,
                nexus_abi::cap_transfer(pid, pol_rsp_clone, Rights::SEND)
                    .map_err(InitError::Abi)?,
            );
            if iw(&mut init_wire, init_fold, "init:policyd") {
                debug_write_bytes(b"init: policyd priority-wired\n");
            }
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
            chan.set_recv(
                ServiceId::Windowd,
                nexus_abi::cap_transfer(pid, window_req_clone, Rights::RECV)
                    .map_err(InitError::Abi)?,
            );
            chan.set_send(
                ServiceId::Windowd,
                nexus_abi::cap_transfer(pid, window_rsp_clone, Rights::SEND)
                    .map_err(InitError::Abi)?,
            );
            if iw(&mut init_wire, init_fold, "init:windowd") {
                debug_write_bytes(b"init: windowd priority-wired\n");
            }
            // NOTE: windowd's registry reply-inbox + bundlemgrd route caps are
            // provisioned LATE (after the gpud caps land at the fallback slots
            // 5/6 the display handoff hardcodes) — see the windowd block after the
            // wiring loop. Provisioning them HERE shifted gpud to slots 8/9 and
            // broke the present handoff with kernel-permission-denied.
        }
        if let Some(chan) = ctrl_channels.iter_mut().find(|c| c.svc_name == "inputd") {
            let pid = chan.pid;
            chan.set_recv(
                ServiceId::Inputd,
                nexus_abi::cap_transfer(pid, input_req_clone, Rights::RECV)
                    .map_err(InitError::Abi)?,
            );
            chan.set_send(
                ServiceId::Inputd,
                nexus_abi::cap_transfer(pid, input_rsp_clone, Rights::SEND)
                    .map_err(InitError::Abi)?,
            );
            if iw(&mut init_wire, init_fold, "init:inputd") {
                debug_write_bytes(b"init: inputd priority-wired\n");
            }
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
                    Ok(_) => {
                        if iw(&mut init_wire, init_fold, "init:selftest-client") {
                            debug_write_bytes(b"init: fw_cfg grant ok svc=selftest-client\n");
                        }
                    }
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

    // Per-service cap-distribution phase (the bespoke `match` + the declarative
    // generic arm). Server pairs for declared services were already distributed
    // pre-grants (see `distribute_server_pairs` above); this pass adds reply
    // inboxes, routes and the announce markers.
    crate::bootstrap::wiring::wire_services(&mut ctrl_channels, &eps, init_fold, &mut init_wire)?;

    // Cumulative boot elapsed just before the display-chain deferred resume. The gap from
    // `grants_done_ms` is the per-service cap-wiring phase; the gap to `total_ms` is the
    // display resume + the updated/bundlemgr OTA handshake tail.
    let wiring_done_ms = boot_span.elapsed_ms();

    // Resume display + input device-driver services after MMIO grants and route wiring.
    crate::bootstrap::resume::resume_drivers(&ctrl_channels, init_fold, &mut init_misc);

    // Yield after cap distribution so services observe a consistent slot layout.
    let _ = nexus_abi::yield_();

    // RFC-0069 §4 boot stage: init's part of the display contract is complete —
    // the display+input chain is granted, wired and resumed (the visible reveal
    // itself is gpud's own contract, ADR-0041). The session track docks onto
    // these named stages: the greeter/login later slots between `display-ready`
    // and `session-start`.
    if il(&mut init_misc, init_fold, "init") {
        debug_write_str("stage: display-ready");
        debug_write_byte(b'\n');
    }

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
            if !ok && il(&mut init_misc, init_fold, "init") {
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

    // RFC-0069 §4 boot stage: boot state is committed (OTA handshake done) and
    // routing is live — the session may begin. Today this transition is
    // automatic (default session = the shell as before); `sessiond` takes
    // ownership of it in Batch S, and login/auth docks in front of it later.
    if il(&mut init_misc, init_fold, "init") {
        debug_write_str("stage: session-start");
        debug_write_byte(b'\n');
    }
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
    if il(&mut init_misc, init_fold, "init") {
        debug_write_str(&timing);
        debug_write_byte(b'\n');
    }

    // RFC-0068: flush the folded per-subject wiring verdicts (interactive only). Paired with the
    // per-trace suppression so no folded line is dropped without its verdict.
    if init_fold && !init_wire.is_empty() {
        // ONE compact `init_caps` verdict for all cap-wiring; recalled via NEXUS_LOG_EXPAND=init_caps
        // (whole group) or =<svc> (one service's lines). Self-contained span (the wiring itself), not
        // the wait until this end-of-bootstrap drain.
        let now = nexus_abi::nsec().unwrap_or(0);
        let v = init_wire.verdict_self();
        let mut line = [0u8; 96];
        let n = nexus_event::render_verdict_line(&mut line, now, "init_caps", v);
        let _ = nexus_abi::debug_write(&line[..n]);
    }
    // The `init` lifecycle verdict (entry/timing/deferred-resume/probe/rollback). Recalled via
    // NEXUS_LOG_EXPAND=init, or =<svc> for a service-tagged line (e.g. deferred resume gpud).
    if init_fold && !init_misc.is_empty() {
        // Lifecycle GRAB-BAG: these markers are spread across the whole boot (early probe → late
        // timing), so a span/`slow` flag would be a false alarm. Show pass/total only (ms=0); a real
        // failure still surfaces as ERROR.
        let raw = init_misc.verdict_self();
        let v = nexus_event::verdict_from(raw.total, raw.total.saturating_sub(raw.passed), None, 0);
        let now = nexus_abi::nsec().unwrap_or(0);
        let mut line = [0u8; 96];
        let n = nexus_event::render_verdict_line(&mut line, now, "lifecycle", v);
        let _ = nexus_abi::debug_write(&line[..n]);
    }

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
