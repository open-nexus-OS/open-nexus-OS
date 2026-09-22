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
use crate::bootstrap::endpoints;
use crate::bootstrap::route_builder;
use crate::bootstrap::route_provision::grant_rtc_mmio_to_timed;
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
    let grant_stats = crate::bootstrap::core_plane::GrantStats::default();
    log_str_ptr("init-msg", "init: start");
    debug_write_str("init: start");
    debug_write_byte(b'\n');
    // TASK-0289-B: an armed fault-fixture TRIAL boot parks here — before
    // any service spawns — so the loader's tries-exhaustion backstop is
    // what recovers the device, not userspace (see fault_fixture.rs).
    crate::bootstrap::fault_fixture::park_if_armed();
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
    // Response endpoints are owned by their receiver, so they are created after spawning once
    // the requester PID is known. Private init-lite -> policyd response channels first.
    let pol_ctl_route_rsp =
        nexus_abi::ipc_endpoint_create_v2(ENDPOINT_FACTORY_CAP_SLOT, 8).map_err(InitError::Abi)?;
    let init_pid = nexus_abi::pid().map_err(InitError::Abi)?;
    // init's OWN ask inbox (P2-d): one endpoint for five protocols made its policyd readers fail CLOSED.
    let init_ask_rsp =
        nexus_abi::ipc_endpoint_create_v2(ENDPOINT_FACTORY_CAP_SLOT, 8).map_err(InitError::Abi)?;
    let init_ask_send =
        nexus_abi::cap_transfer(init_pid, init_ask_rsp, Rights::SEND).map_err(InitError::Abi)?;
    let init_ask = nexus_ipc::SlotPair::new(init_ask_send, init_ask_rsp);
    let pol_ctl_exec_rsp =
        nexus_abi::ipc_endpoint_create_v2(ENDPOINT_FACTORY_CAP_SLOT, 8).map_err(InitError::Abi)?;

    // ADR-0062: ONE monotone boot-stage fence per boot. init keeps `MANAGE | WAIT` and is the
    // only task that can advance it; every child gets a WAIT-only copy at spawn.
    let stage_fence = nexus_abi::fence_create().map_err(InitError::Abi)?;

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
                // Private control endpoints (REQ/RSP) at the child's slots 1/2 —
                // shared with the volume spawn pass (TASK-0321).
                let (ctrl, child_send_slot, child_recv_slot) =
                    crate::bootstrap::spawn::attach_ctrl_channel(image.name, pid, stage_fence)?;
                if image.name == "updated" && iw(&mut init_wire, init_fold, "init:updated") {
                    debug_write_bytes(b"init: updated ctrl slots send=0x");
                    debug_write_hex(child_send_slot as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(child_recv_slot as usize);
                    debug_write_byte(b'\n');
                }
                ctrl_channels.push(ctrl);
                if probes_enabled() {
                    debug_write_bytes(b"!spawn ok pid=0x");
                    debug_write_hex(pid as usize);
                    debug_write_byte(b'\n');
                }
                spawn_tally.record(nexus_event::Status::Ok, nexus_abi::nsec().unwrap_or(0));
                // `init: up <svc>` moved to the responder's `@ready` arm (RFC-0093 §2):
                // spawn success is NOT readiness (RFC-0013 A1).
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
    // Wave 0 (policyd/virtioblkd/bundlemgrd) is resumed INSIDE the CORE-plane
    // stage below, right after its server pairs exist; the rest of the
    // always-on core (wave 1) resumes after the bulk server-pair
    // distribution — see `resume::PLANE` for why nothing runs earlier.

    // Second pass: create request endpoints owned by the target service PID and distribute caps.
    fn find_pid(ctrls: &[CtrlChannel], name: &str) -> Option<u32> {
        ctrls.iter().find(|c| c.svc_name == name).map(|c| c.pid)
    }
    let selftest_pid = find_pid(&ctrl_channels, "selftest-client").ok_or(InitError::MissingElf)?;
    let policyd_pid = find_pid(&ctrl_channels, "policyd").ok_or(InitError::MissingElf)?;
    let bundlemgrd_pid = find_pid(&ctrl_channels, "bundlemgrd").ok_or(InitError::MissingElf)?;

    // TASK-0321 P4: the CORE control plane + block plane stage, then the
    // volume spawn pass — BEFORE any per-pid endpoint mint below, so a
    // service on the volume is wired exactly like an embedded one.
    let crate::bootstrap::core_plane::CorePlane {
        pol_req,
        pol_rsp,
        bnd_req,
        bnd_rsp,
        vblk_req,
        vblk_rsp,
        pol_ctl_route_req,
        pol_ctl_exec_req,
        devices,
        volume: volume_spawned,
        volume_ms,
    } = crate::bootstrap::core_plane::bring_up(
        &mut ctrl_channels,
        selftest_pid,
        policyd_pid,
        bundlemgrd_pid,
        pol_ctl_route_rsp,
        pol_ctl_exec_rsp,
        init_ask,
        &grant_stats,
        init_fold,
        &mut init_wire,
        stage_fence,
    )?;
    let pol_route = (pol_ctl_route_req, pol_ctl_route_rsp);
    let vfsd_pid = find_pid(&ctrl_channels, "vfsd").ok_or(InitError::MissingElf)?;
    let packagefsd_pid = find_pid(&ctrl_channels, "packagefsd").ok_or(InitError::MissingElf)?;
    let netstackd_pid = find_pid(&ctrl_channels, "netstackd").ok_or(InitError::MissingElf)?;
    let dsoftbusd_pid = find_pid(&ctrl_channels, "dsoftbusd").ok_or(InitError::MissingElf)?;
    let updated_pid = find_pid(&ctrl_channels, "updated").ok_or(InitError::MissingElf)?;
    let samgrd_pid = find_pid(&ctrl_channels, "samgrd").ok_or(InitError::MissingElf)?;
    let execd_pid = find_pid(&ctrl_channels, "execd").ok_or(InitError::MissingElf)?;
    let _keystored_pid = find_pid(&ctrl_channels, "keystored").ok_or(InitError::MissingElf)?;
    let _statefsd_pid = find_pid(&ctrl_channels, "statefsd").ok_or(InitError::MissingElf)?;
    let rngd_pid = find_pid(&ctrl_channels, "rngd").ok_or(InitError::MissingElf)?;
    let socd_pid = find_pid(&ctrl_channels, "socd").ok_or(InitError::MissingElf)?;
    let timed_pid = find_pid(&ctrl_channels, "timed").ok_or(InitError::MissingElf)?;
    let imed_pid = find_pid(&ctrl_channels, "imed").ok_or(InitError::MissingElf)?;
    let hidrawd_pid = find_pid(&ctrl_channels, "hidrawd").ok_or(InitError::MissingElf)?;
    let windowd_pid = find_pid(&ctrl_channels, "windowd").ok_or(InitError::MissingElf)?;
    let inputd_pid = find_pid(&ctrl_channels, "inputd").ok_or(InitError::MissingElf)?;
    let gpud_pid = find_pid(&ctrl_channels, "gpud").ok_or(InitError::MissingElf)?;
    let logd_pid = find_pid(&ctrl_channels, "logd");
    let metricsd_pid = find_pid(&ctrl_channels, "metricsd");

    // selftest-client <-> service endpoint pairs.
    // Owned-endpoint mint helper (owner pid, queue depth).
    let mint = |pid: u32, depth: usize| {
        nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, depth)
            .map_err(InitError::Abi)
    };
    let vfs_req = mint(vfsd_pid, 8)?;
    let vfs_rsp = mint(selftest_pid, 8)?;
    let pkg_req = mint(packagefsd_pid, 8)?;
    let pkg_rsp = mint(selftest_pid, 8)?;
    let upd_req = mint(updated_pid, 8)?;
    let upd_rsp = mint(selftest_pid, 8)?;
    let sam_req = mint(samgrd_pid, 8)?;
    let sam_rsp = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, selftest_pid, 8)
        .map_err(InitError::Abi)?;
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
    let rng_req = mint(rngd_pid, 8)?;
    let rng_rsp = mint(selftest_pid, 8)?;
    // socd <-> clients (RFC-0106): the same shape — the request endpoint is socd's, the
    // response endpoint the harness's (its NotNeeded proof is the first consumer).
    let soc_req = mint(socd_pid, 8)?;
    let soc_rsp = mint(selftest_pid, 8)?;
    let timed_req = mint(timed_pid, 8)?;
    let timed_rsp = mint(selftest_pid, 8)?;
    let imed_req = mint(imed_pid, 8)?;
    let imed_rsp = mint(inputd_pid, 8)?;
    let imed_osk = mint(imed_pid, 8)?; // OSK ep (RFC-0075 P2): RECV to imed, SEND to execd + selftest
    let inputd_watch_ep = mint(inputd_pid, 8)?;
    let windowd_watch_ep = mint(windowd_pid, 8)?;
    let windowd_session_watch_ep = mint(windowd_pid, 8)?;
    let window_req = mint(windowd_pid, 32)?;
    let window_rsp = mint(windowd_pid, 8)?;
    let input_req = mint(inputd_pid, 8)?;
    let input_rsp = mint(hidrawd_pid, 8)?;

    // Priority-wire display services early (right after their endpoints exist)
    // so they get scheduled by the existing yield after MMIO grants.

    let gpud_req = mint(gpud_pid, 8)?;
    let gpud_rsp = mint(gpud_pid, 8)?;

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

    // Selftest reply-inbox endpoint:
    // - owned by selftest-client (receiver)
    // - selftest-client holds RECV to await replies and a SEND cap that it can CAP_MOVE to a server
    let reply_ep = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, selftest_pid, 8)
        .map_err(InitError::Abi)?;

    // execd reply-inbox endpoint (for CAP_MOVE request/reply, e.g. logd crash append).
    let execd_reply_ep =
        nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, execd_pid, 8)
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

    // packagefsd reply-inbox endpoint (for CAP_MOVE request/reply to other services, e.g. bundlemgrd):
    let pkg_reply_ep =
        nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, packagefsd_pid, 8)
            .map_err(InitError::Abi)?;

    // sessiond server endpoints (request/response), pre-minted (TASK-0065B):
    // sessiond spawns LAST, but windowd's greeter route and abilitymgr's launch
    // gate are wired much earlier — the pair must exist from bootstrap.
    let sessiond_pid = find_pid(&ctrl_channels, "sessiond");
    let (sess_req, sess_rsp) = if let Some(pid) = sessiond_pid {
        let req = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, 8)
            .map_err(InitError::Abi)?;
        let rsp = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, 8)
            .map_err(InitError::Abi)?;
        (Some(req), Some(rsp))
    } else {
        (None, None)
    };
    // Ability-lifecycle route (TASK-0080D launch path): pre-mint abilitymgr's
    // server pair — like sessiond's — so windowd's launch-request route can
    // be granted from the SAME endpoints the declarative arm hands abilitymgr
    // (a fresh per-arm pair would orphan the client side).
    let abilitymgr_pid = find_pid(&ctrl_channels, "abilitymgr");
    let (abil_req, abil_rsp) = if let Some(pid) = abilitymgr_pid {
        let req = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, 8)
            .map_err(InitError::Abi)?;
        let rsp = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, 8)
            .map_err(InitError::Abi)?;
        (Some(req), Some(rsp))
    } else {
        (None, None)
    };
    // Settings authority: pre-mint settingsd's server pair — like sessiond's —
    // so windowd's theme route and execd's per-app `svc.settings` grants clone
    // the SAME endpoints settingsd serves. `server_pair(Settingsd)` returned
    // None before this, so BOTH routes silently never wired ("theme default
    // (settingsd unavailable)" + "execd: FAIL app route resolve svc=settings").
    let settingsd_pid = find_pid(&ctrl_channels, "settingsd");
    let (sett_req, sett_rsp) = if let Some(pid) = settingsd_pid {
        let req = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, 8)
            .map_err(InitError::Abi)?;
        let rsp = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, 8)
            .map_err(InitError::Abi)?;
        (Some(req), Some(rsp))
    } else {
        (None, None)
    };

    let boot_req = nexus_abi::ipc_endpoint_create_v2(ENDPOINT_FACTORY_CAP_SLOT, 8).ok(); // TASK-0050: init-owned (init calls the early handshake)
    let boot_rsp = nexus_abi::ipc_endpoint_create_v2(ENDPOINT_FACTORY_CAP_SLOT, 8).ok();
    let (pinch_req, pinch_rsp) =
        crate::bootstrap::endpoints::mint_pinched_pair(&ctrl_channels, selftest_pid)?;
    // Inbound gateway (RFC-0092 / TASK-0052 P3): pre-mint ingressd's server
    // pair — like sessiond's — so the selftest's intent route clones the
    // SAME endpoints the declarative arm hands the gateway.
    let ingressd_pid = find_pid(&ctrl_channels, "ingressd");
    let (ingress_req, ingress_rsp) = if let Some(pid) = ingressd_pid {
        let req = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, 8)
            .map_err(InitError::Abi)?;
        let rsp = nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, pid, 8)
            .map_err(InitError::Abi)?;
        (Some(req), Some(rsp))
    } else {
        (None, None)
    };

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
        vfs_req,
        vfs_rsp,
        pkg_req,
        pkg_rsp,
        pkg_reply_ep,
        pol_req,
        pol_rsp,
        bnd_req,
        bnd_rsp,
        upd_req,
        upd_rsp,
        sam_req,
        sam_rsp,
        exe_req,
        exe_rsp,
        key_req,
        key_rsp,
        state_req,
        vblk_req,
        vblk_rsp,
        state_rsp,
        rng_req,
        rng_rsp,
        soc_req,
        soc_rsp,
        timed_req,
        timed_rsp,
        imed_req,
        imed_rsp,
        imed_osk,
        inputd_watch_ep,
        windowd_watch_ep,
        windowd_session_watch_ep,
        window_req,
        window_rsp,
        input_req,
        input_rsp,
        gpud_req,
        gpud_rsp,
        net_req,
        net_rsp,
        dsoft_req,
        dsoft_rsp,
        execd_reply_ep,
        reply_ep,
        log_req,
        log_rsp,
        metrics_req,
        metrics_rsp,
        sess_req,
        sess_rsp,
        abil_req,
        abil_rsp,
        sett_req,
        sett_rsp,
        boot_req,
        boot_rsp,
        pinch_req,
        pinch_rsp,
        ingress_req,
        ingress_rsp,
    };
    crate::bootstrap::distribute::distribute_server_pairs(&mut ctrl_channels, &eps);
    // TASK-0324 P4f-5: the proof harness runs from wave 1 on, so its declared legs are pinned
    // BEFORE it first runs (RFC-0093 §4) — not by transfer order while it already runs.
    crate::bootstrap::declared_routes::wire_proof_harness(&mut ctrl_channels, &eps);
    // Wave 1 (TASK-0050 PR-5): the rest of the always-on CORE graph — the
    // boot target is unknown until the bootctld handshake below; the core is
    // exactly what that handshake (and any recovery boot) needs. Every
    // service now holds its server pair, so nothing retries a route probe.
    // ADR-0062: no yield here. A yield is not a barrier — it hands the CPU over once and
    // guarantees nothing about what the resumed services got done. What init sends next either
    // parks until its target can answer (routing v2) or waits on the stage fence.
    // Priority-wire windowd + inputd. The minted pairs are pinned directly: a transfer duplicates,
    // and the later legs (execd, hidrawd, inputd → windowd) pin the same endpoints again.
    {
        if let Some(chan) = ctrl_channels.iter_mut().find(|c| c.svc_name == "windowd") {
            let pid = chan.pid;
            // TASK-0324 P4a: PINNED to the declared slots — provisioning order no longer
            // decides where a capability lands, so the "do not provision the registry route
            // here or gpud shifts to 8/9" note this block used to carry is obsolete.
            let slots = crate::bootstrap::declared_slots::pin_server_pair(
                pid,
                ServiceId::Windowd,
                window_req,
                window_rsp,
            )
            .ok_or(InitError::Map("windowd server slots"))?;
            chan.set_recv(ServiceId::Windowd, slots.recv);
            chan.set_send(ServiceId::Windowd, slots.send);
            if iw(&mut init_wire, init_fold, "init:windowd") {
                debug_write_bytes(b"init: windowd priority-wired\n");
            }
        }
        if let Some(chan) = ctrl_channels.iter_mut().find(|c| c.svc_name == "inputd") {
            let pid = chan.pid;
            // TASK-0324 P4b: pinned to the declared slots (see windowd above).
            let slots = crate::bootstrap::declared_slots::pin_server_pair(
                pid,
                ServiceId::Inputd,
                input_req,
                input_rsp,
            )
            .ok_or(InitError::Map("inputd server slots"))?;
            chan.set_recv(ServiceId::Inputd, slots.recv);
            chan.set_send(ServiceId::Inputd, slots.send);
            if iw(&mut init_wire, init_fold, "init:inputd") {
                debug_write_bytes(b"init: inputd priority-wired\n");
            }
        }
    }

    // Per-service cap-distribution (bespoke `match` + declarative arm): reply inboxes, routes,
    // timer-notify pairs and the announce markers — pure cap transfers into SUSPENDED tasks, so
    // it runs BEFORE any resume (TASK-0324 P7-d). A service's first exchange (bootctld's
    // record attach at start-up) finds its declared slots in place; the retry cadences that
    // used to cover a leg pinned after the task ran are gone.
    crate::bootstrap::wiring::wire_services(&mut ctrl_channels, &eps, init_fold, &mut init_wire)?;
    // Cap-table hygiene (RFC-0075/0078): a parked full table broke @mint-pair.
    endpoints::close_wired_eps(&eps);
    crate::bootstrap::resume::resume_core(&ctrl_channels);

    // RFC-0098 C3: every grant below carries a window + PLIC line read from the
    // device's tree node (ADR-0044: blk[0] = the ONE disk, granted in the CORE-plane
    // stage; a second blk device is nobody's — `/data` is a partition on the one disk).
    let net = devices.net.ok_or(InitError::Map("virtio-net not in the device tree"))?;
    let rng = devices.rng.ok_or(InitError::Map("virtio-rng not in the device tree"))?;
    crate::bootstrap::core_plane::grant_mmio_with_wait(
        &grant_stats,
        pol_route,
        netstackd_pid,
        "netstackd",
        "device.mmio.net",
        net,
        DEVICE_MMIO_CAP_SLOT,
    )?;
    crate::bootstrap::core_plane::grant_mmio_with_wait(
        &grant_stats,
        pol_route,
        rngd_pid,
        "rngd",
        "device.mmio.rng",
        rng,
        DEVICE_MMIO_CAP_SLOT,
    )?;
    // RFC-0076: RTC window → timed (own anchor read; no rtcd). Best-effort.
    grant_rtc_mmio_to_timed(timed_pid, pol_ctl_route_req, pol_ctl_route_rsp)?;
    let gpu = devices.gpu.ok_or(InitError::Map("virtio-gpu not in the device tree"))?;
    crate::bootstrap::core_plane::grant_mmio_with_wait(
        &grant_stats,
        pol_route,
        gpud_pid,
        "gpud",
        "device.mmio.gpu",
        gpu,
        DEVICE_MMIO_CAP_SLOT,
    )?;
    crate::bootstrap::core_plane::grant_mmio_with_wait(
        &grant_stats,
        pol_route,
        selftest_pid,
        "selftest-client",
        "device.mmio.net",
        net,
        DEVICE_MMIO_CAP_SLOT,
    )?;

    // RFC-0106: the SoC glue owner — the tree alias and every provider window by compatible.
    crate::bootstrap::soc_glue::provision(
        socd_pid,
        &grant_stats,
        pol_route,
        &mut init_wire,
        init_fold,
    )?;

    // The device tree, read-only, to selftest-client: the harness reads its runtime
    // boot-config (`/chosen/nexus,boot-mode` / `boot-profile`, written by nxboot from
    // the launcher's knobs) WITHOUT a rebuild — the same binary boots in `proof` mode
    // under the harness and `interactive-full` under `just start`. Not a device: the
    // alias the kernel gave init is pinned directly (no policyd round-trip) into the
    // slot the topology declares (`NamedSlot::DeviceTree`). Non-fatal: without it the
    // client's `runtime_mode` is None → the legacy `full` profile + verdict mode off.
    {
        let pinned = crate::bootstrap::declared_slots::pin_named(
            selftest_pid,
            ServiceId::SelftestClient,
            crate::service_topology::NamedSlot::DeviceTree,
            nexus_abi::INIT_DEVICE_TREE_SLOT,
            Rights::MAP,
        );
        if pinned.is_some() && iw(&mut init_wire, init_fold, "init:selftest-client") {
            debug_write_bytes(b"init: device tree grant ok svc=selftest-client\n");
        }
    }

    for (idx, input) in devices.input.iter().copied().enumerate() {
        if let Some(input) = input {
            crate::bootstrap::core_plane::grant_mmio_with_wait(
                &grant_stats,
                pol_route,
                hidrawd_pid,
                "hidrawd",
                "device.mmio.input",
                input,
                INPUT_MMIO_CAP_SLOT_BASE + u32::try_from(idx).unwrap_or(0),
            )?;
        }
    }

    // TASK-0315: statefsd is a blockproto CLIENT — the double MMIO grant
    // (the ADR-0044 one-owner violation) is gone; virtioblkd above is the
    // only holder.
    // Boot elapsed after the MMIO-grant phase (spawn + resume + early wiring
    // + grants); the gap to `total_ms` is the co-run cap-wiring phase.
    let grants_done_ms = boot_span.elapsed_ms();

    // The cap-distribution pass ran BEFORE the core resume (TASK-0324 P7-d): every service
    // holds its declared legs the moment it first runs.

    // Boot elapsed before the display-chain deferred resume; the gap to
    // `total_ms` is that resume + the OTA handshake tail.
    let wiring_done_ms = boot_span.elapsed_ms();

    // Resume display + input device-driver services after MMIO grants and route wiring.
    // Resolve the boot target FIRST (bootctld runs since wave 1): the
    // one-shot next_boot decides which of the fully provisioned services
    // wave 2 + the display/input drivers actually resume (RFC-0087 §4).
    let boot_graph = crate::bootstrap::handshake::boot_attempt_handshake(
        boot_req,
        init_ask,
        bnd_req,
        &mut init_misc,
        init_fold,
    );
    crate::bootstrap::resume::materialize_graph(
        &ctrl_channels,
        boot_graph,
        init_fold,
        &mut init_misc,
    );

    let route_table = route_builder::build_route_table(&ctrl_channels);
    // init keeps the minted pair itself (transfers duplicate), so the registry population uses it.
    route_builder::populate_samgrd_registry(sam_req, sam_rsp, &route_table);

    // ADR-0062: the stage markers are NOT printed here any more. A stage is reached when its
    // barrier is satisfied — announced readiness plus windowd's own report — and the responder
    // prints each one AT THE SIGNAL SITE, right where it advances the kernel fence. Printing a
    // stage at a fixed point in init's code path is precisely the "order is the contract"
    // pattern this task deletes.
    // Boot-timing table (Phase 3): one compact line locating where boot time went. `grant_wait`
    // is the time spent yielding for policyd MMIO grants — the prime "services waiting" suspect.
    let total_ms = boot_span.elapsed_ms();
    let timing = alloc::format!(
        "init: timing spawn_ms={} volume_ms={} grants_at_ms={} wiring_at_ms={} total_ms={} (grant_wait_ms={} grants={} wiring_ms={} tail_ms={})",
        spawn_ms,
        volume_ms,
        grants_done_ms,
        wiring_done_ms,
        total_ms,
        grant_stats.wait_ns.get() / 1_000_000,
        grant_stats.count.get(),
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

    let statefs_req =
        eps.server_pair(crate::service_topology::ServiceId::Statefsd).map(|(req, _)| req);
    Ok(BootstrapState {
        respawn: crate::bootstrap::respawn::RespawnContext::new(
            images,
            volume_spawned,
            selftest_pid,
            pinch_rsp,
            statefs_req,
            init_ask,
            stage_fence,
        ),
        ctrl_channels,
        route_table,
        pol_ctl_route_req,
        pol_ctl_route_rsp,
        pol_ctl_exec_req,
        pol_ctl_exec_rsp,
        upd_req,
        ask: init_ask,
        stage_fence,
        boot_graph,
    })
}
