// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Routing responder loop — extracted from os_payload.rs per RFC-0061.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os)
//! ADR: docs/adr/0017-service-architecture.md
//! RFC: docs/rfcs/RFC-0061-selftest-observer-init-refactoring.md
//!
//! Runs the init-lite control-channel responder: processes route-get, health-ok,
//! and exec-check requests from spawned services, consulting policyd for gating.

use crate::bootstrap::responder_clock;
use crate::bootstrap::route_reply;
use crate::bootstrap::CtrlChannel;
use crate::route_table::RouteTable;
use alloc::vec::Vec;

/// Run the routing responder loop forever. Only returns via `fatal()` on watchdog expiry.
pub(crate) fn run_responder_loop(
    mut ctrl_channels: Vec<CtrlChannel>,
    respawn_ctx: crate::bootstrap::respawn::RespawnContext,
    mut route_table: RouteTable,
    pol_ctl_route_req: u32,
    pol_ctl_route_rsp: u32,
    pol_ctl_exec_req: u32,
    pol_ctl_exec_rsp: u32,
    upd_req: u32,
    ask: nexus_ipc::SlotPair,
    stage_fence: u32,
    boot_graph: crate::boot_graph::BootGraph,
) -> ! {
    use crate::bootstrap::policyd::{policyd_exec_allowed, policyd_route_allowed};
    use crate::os_payload::*;

    let watchdog = watchdog_limit_ticks();
    let mut ticks: usize = 0;
    // Reactive idle: a waitset over every control-channel request endpoint lets the responder
    // SLEEP until one has a message, instead of busy-polling all channels every scheduler round
    // (the pre-RFC-0033 pattern). The full NONBLOCK sweep below is unchanged and still drains
    // every channel on each wake, so the waitset is purely a "stop spinning while idle" layer —
    // a failed add or a missed wake only costs the 1s safety-net latency, never a dropped request.
    // TASK-0324 P8: the responder waits on ONE waitset — every control channel plus init's own
    // timer-notify endpoint. A child's death reaches it through the kernel's EOF latch on that
    // child's control endpoint; a scheduled respawn through the one-shot timer armed at its due
    // time. No idle cadence, no safety net: nothing pending means zero wakes.
    let mut clock = responder_clock::ResponderClock::new();
    let waitset = responder_clock::build_ctrl_waitset(&ctrl_channels, clock.notify_ep());
    // TASK-0049B: init is the ONLY possible reaper of its service children
    // (`wait` is parent-bound) — one bounded sweep per round announces every
    // death with kernel truth; the one-shot probe proves the sweep each boot.
    let mut supervision = crate::bootstrap::supervision::SupervisionSweep::start();
    let mut respawner = crate::bootstrap::respawn::Respawner::new(respawn_ctx);
    // RFC-0093 §2 / ADR-0062: `init: up <svc>` is emitted from the `@ready` arm below and
    // nowhere else — a service is ready when it says so, not when init resumed it.
    let mut ready = crate::ready_table::ReadyTable::new();
    // RFC-0093 §1 (TASK-0324 P3): an ask whose target is momentarily unresolvable (the
    // supervisor marked it stale and is restarting it) is PARKED and answered exactly once
    // when the route is re-provisioned — the client never re-asks in a loop.
    let mut park = crate::route_park::RoutePark::new();
    // ADR-0062: the boot-stage ladder. By the time the responder runs, init has committed the
    // boot state and populated the registry — the `SessionStart` milestone — so it is recorded
    // once here rather than guessed at later. Every rung after that is opened by EVIDENCE:
    // `@ready` from the barrier's members and windowd's own `@stage` report.
    let mut ladder = crate::stage::StageLadder::new(boot_graph);
    ladder.record_milestones();
    crate::bootstrap::stage_signal::advance(&mut ladder, &ctrl_channels, &ready, stage_fence);
    let init_fold = nexus_abi::boot_should_fold_verdicts();
    loop {
        supervision.sweep(
            &mut ctrl_channels,
            &mut route_table,
            &mut respawner,
            &mut ready,
            &mut park,
        );
        route_reply::answer_parked_routes(&mut park, &ctrl_channels, &route_table);
        for (chan_idx, chan) in ctrl_channels.iter().enumerate() {
            let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
            let mut buf = [0u8; 64];
            let n = match nexus_abi::ipc_recv_v1(
                chan.ctrl_req_parent_slot,
                &mut hdr,
                &mut buf,
                nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
                0,
            ) {
                Ok(n) => n as usize,
                Err(nexus_abi::IpcError::QueueEmpty) => continue,
                // A control channel that fails for any other reason is a witness, never a
                // silent skip: a frame the kernel refused to hand over is a frame the ladder
                // will wait for forever (TASK-0327B P4 H0d — measured on the board).
                Err(err) => {
                    crate::bootstrap::diag::emit_marker_atomic(
                        &[
                            b"init: ctrl recv err svc=",
                            chan.svc_name.as_bytes(),
                            b" err=",
                            ipc_error_label(err).as_bytes(),
                        ],
                        None,
                    );
                    continue;
                }
            };
            if chan.svc_name == "updated" {
                debug_write_bytes(b"init: ctrl req from updated\n");
            }
            // Health gate: allow selftest-client to notify init.
            if chan.svc_name == "selftest-client" && decode_init_health_ok_req(&buf[..n]) {
                let nonce = decode_init_health_ok_req_with_optional_nonce(&buf[..n]).flatten();
                let status = match updated_health_ok(upd_req, ask) {
                    Ok(slot) => {
                        debug_write_str("init: health ok (slot ");
                        debug_write_byte(slot);
                        debug_write_str(")");
                        debug_write_byte(b'\n');
                        INIT_HEALTH_STATUS_OK
                    }
                    Err(err) => {
                        debug_write_str("init: health fail ");
                        match err {
                            InitError::Map(msg) => debug_write_str(msg),
                            InitError::Abi(code) => debug_write_str(abi_error_label(code)),
                            InitError::Ipc(code) => debug_write_str(ipc_error_label(code)),
                            InitError::Elf(msg) => debug_write_str(msg),
                            InitError::MissingElf => debug_write_str("missing-elf"),
                        }
                        debug_write_byte(b'\n');
                        INIT_HEALTH_STATUS_FAILED
                    }
                };
                if nonce.is_some() {
                    let rsp = encode_init_health_ok_rsp_with_optional_nonce(status, nonce);
                    let rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, rsp.len() as u32);
                    let _ = nexus_abi::ipc_send_v1(
                        chan.ctrl_rsp_parent_slot,
                        &rh,
                        &rsp,
                        nexus_abi::IPC_SYS_NONBLOCK,
                        0,
                    );
                } else {
                    let rsp = encode_init_health_ok_rsp(status);
                    let rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, rsp.len() as u32);
                    let _ = nexus_abi::ipc_send_v1(
                        chan.ctrl_rsp_parent_slot,
                        &rh,
                        &rsp,
                        nexus_abi::IPC_SYS_NONBLOCK,
                        0,
                    );
                }
                continue;
            }

            let (name, route_nonce) = match decode_route_get_with_optional_nonce(&buf[..n]) {
                Some((name, nonce)) => (name, nonce),
                None => {
                    if let Some((nonce, requester, image_id)) =
                        nexus_abi::policy::decode_exec_check(&buf[..n])
                    {
                        if chan.svc_name != "execd" {
                            let rsp = nexus_abi::policy::encode_exec_check_rsp(
                                nonce,
                                nexus_abi::policy::STATUS_DENY,
                            );
                            let rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, rsp.len() as u32);
                            let _ = nexus_abi::ipc_send_v1(
                                chan.ctrl_rsp_parent_slot,
                                &rh,
                                &rsp,
                                nexus_abi::IPC_SYS_NONBLOCK,
                                0,
                            );
                            continue;
                        }
                        let allowed = policyd_exec_allowed(
                            pol_ctl_exec_req,
                            pol_ctl_exec_rsp,
                            requester,
                            image_id,
                        )
                        .unwrap_or(true);
                        let status = if allowed {
                            nexus_abi::policy::STATUS_ALLOW
                        } else {
                            nexus_abi::policy::STATUS_DENY
                        };
                        let rsp = nexus_abi::policy::encode_exec_check_rsp(nonce, status);
                        let rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, rsp.len() as u32);
                        let _ = nexus_abi::ipc_send_v1(
                            chan.ctrl_rsp_parent_slot,
                            &rh,
                            &rsp,
                            nexus_abi::IPC_SYS_NONBLOCK,
                            0,
                        );
                        continue;
                    }
                    // Neither a route ask nor an exec check: the frame is NAMED, with its
                    // length and first bytes, instead of vanishing — a control verb that does
                    // not decode is exactly what a lost `@ready` would look like from here
                    // (TASK-0327B P4 H0d). Bounded: one line, eight head bytes.
                    crate::bootstrap::diag::emit_ctrl_frame_unknown(chan.svc_name, &buf[..n]);
                    continue;
                }
            };
            if name == b"samgrd" && chan.svc_name == "selftest-client" {
                debug_write_bytes(b"init: route samgrd from selftest-client\n");
            }
            if name == b"statefsd" {
                debug_write_bytes(b"init: route statefsd from ");
                debug_write_str(chan.svc_name);
                debug_write_byte(b'\n');
            }
            if name == b"vfsd" {
                debug_write_bytes(b"init: route vfsd from ");
                debug_write_str(chan.svc_name);
                debug_write_byte(b'\n');
            }
            if name == b"@ready" {
                // One-way readiness announce (nexus_service_entry::ready). No reply frame:
                // a stale RSP in the child's control queue is the confused-waiter class
                // routing v2 removes. Refusals are loud and name the service.
                let known = ctrl_channels.iter().any(|c| c.pid == chan.pid);
                // One atomic write per line: a marker torn against another process's
                // UART write is a red ladder gate (`init: up keystoredgpud: …` was seen).
                match ready.announce(chan.pid, known) {
                    Ok(()) => {
                        if !init_fold
                            || crate::bootstrap::diag::expanded("init_spawn")
                            || crate::bootstrap::diag::expanded(chan.svc_name)
                        {
                            crate::bootstrap::diag::emit_marker_atomic(
                                &[b"init: up ", chan.svc_name.as_bytes()],
                                None,
                            );
                        }
                    }
                    Err(err) => {
                        crate::bootstrap::diag::emit_marker_atomic(
                            &[
                                b"init: FAIL ready ",
                                err.label().as_bytes(),
                                b" svc=",
                                chan.svc_name.as_bytes(),
                            ],
                            None,
                        );
                    }
                }
                crate::bootstrap::stage_signal::advance(
                    &mut ladder,
                    &ctrl_channels,
                    &ready,
                    stage_fence,
                );
                continue;
            }
            if let Some(label) = name.strip_prefix(b"@stage ".as_slice()) {
                // RFC-0093 §2: only windowd may report a display stage, and identity is the
                // CONTROL CHANNEL this frame arrived on — never a name in the payload. A report
                // from anyone else is refused loudly instead of advancing the whole fleet.
                if chan.svc_name != "windowd" {
                    crate::bootstrap::diag::emit_marker_atomic(
                        &[b"!stage-deny: ", chan.svc_name.as_bytes(), b" -> ", label],
                        None,
                    );
                    continue;
                }
                match crate::service_topology::Stage::from_label(label) {
                    Some(stage) => ladder.record_stage_report(stage),
                    None => crate::bootstrap::diag::emit_marker_atomic(
                        &[b"!stage-unknown: ", label],
                        None,
                    ),
                }
                crate::bootstrap::stage_signal::advance(
                    &mut ladder,
                    &ctrl_channels,
                    &ready,
                    stage_fence,
                );
                continue;
            }
            // RFC-0093 §1: every ask that gets an ANSWER must carry a nonce — without it
            // the answer can be consumed by the wrong waiter (the class that made windowd
            // bind its own inbox and packagefsd fall back to a RAM seed). `@ready` above is
            // exempt by construction: it is one-way and has nothing to correlate.
            let Some(route_nonce) = route_nonce else {
                crate::bootstrap::diag::emit_marker_atomic(
                    &[
                        b"!route-malformed: ",
                        chan.svc_name.as_bytes(),
                        b" -> ",
                        name,
                        b" (route ask without nonce)",
                    ],
                    None,
                );
                route_reply::send_route_malformed(chan);
                continue;
            };
            if name == b"@mint-pair" {
                // Dynamic per-launch endpoint mint (correlation fix,
                // production-grade): execd asks; init — the EndpointFactory
                // holder (non-duplicable security floor) — mints a FRESH pair
                // and transfers BOTH halves to execd. Used for the child's
                // event channel AND its private reply inbox (`@reply` returns
                // execd's PERSISTENT shared inbox — never grant that to
                // children: shared queue = reply theft across processes). No
                // pre-sized pool, no slot-order contract; execd does
                // mint→grant→close per launch (zero cap-table accumulation).
                // Identity-gated allowlist: execd (app launches) and the
                // selftest harness (RFC-0078 watch probe mints its private
                // push channel). Everything else stays denied.
                let (status, send_slot, recv_slot) = if chan.svc_name == "execd"
                    || chan.svc_name == "selftest-client"
                {
                    match nexus_abi::ipc_endpoint_create_for(ENDPOINT_FACTORY_CAP_SLOT, chan.pid, 8)
                    {
                        Ok(ep) => {
                            let send =
                                nexus_abi::cap_transfer(chan.pid, ep, nexus_abi::Rights::SEND);
                            let recv =
                                nexus_abi::cap_transfer(chan.pid, ep, nexus_abi::Rights::RECV);
                            let _ = nexus_abi::cap_close(ep);
                            match (send, recv) {
                                (Ok(s), Ok(r)) => (nexus_abi::routing::STATUS_OK, s, r),
                                _ => {
                                    debug_write_bytes(b"init: FAIL mint-pair transfer\n");
                                    (nexus_abi::routing::STATUS_NOT_FOUND, 0, 0)
                                }
                            }
                        }
                        Err(_) => {
                            debug_write_bytes(b"init: FAIL mint-pair create\n");
                            (nexus_abi::routing::STATUS_NOT_FOUND, 0, 0)
                        }
                    }
                } else {
                    debug_write_bytes(b"init: mint-pair denied (not allowlisted)\n");
                    (nexus_abi::routing::STATUS_NOT_FOUND, 0, 0)
                };
                route_reply::send_route_rsp(chan, status, send_slot, recv_slot, route_nonce);
                continue;
            }
            if name == b"@reply" {
                let status = if chan.reply_send_slot.is_some() && chan.reply_recv_slot.is_some() {
                    nexus_abi::routing::STATUS_OK
                } else {
                    nexus_abi::routing::STATUS_NOT_FOUND
                };
                let send_slot = chan.reply_send_slot.unwrap_or(0);
                let recv_slot = chan.reply_recv_slot.unwrap_or(0);
                route_reply::send_route_rsp(chan, status, send_slot, recv_slot, route_nonce);
                continue;
            }
            let allowed = if name == chan.svc_name.as_bytes() {
                true
            } else if chan.svc_name == "policyd" {
                true
            } else if chan.svc_name == "bundlemgrd" && name == b"execd" {
                policyd_route_allowed(pol_ctl_route_req, pol_ctl_route_rsp, chan.svc_name, name)
                    .unwrap_or(false)
            } else {
                // RFC-0093 §1: fail-closed. An unreachable policy authority used to mean
                // "allow" — privilege by outage. Now it denies and NAMES the outage.
                match policyd_route_allowed(
                    pol_ctl_route_req,
                    pol_ctl_route_rsp,
                    chan.svc_name,
                    name,
                ) {
                    Some(verdict) => verdict,
                    None => {
                        if route_deny_first_time(chan.svc_name, name) {
                            debug_write_bytes(b"!route-deny: ");
                            debug_write_str(chan.svc_name);
                            debug_write_bytes(b" -> ");
                            debug_write_bytes(name);
                            debug_write_bytes(b" (policy unavailable)\n");
                        }
                        false
                    }
                }
            };
            if !allowed {
                // Direct, greppable route-denial error (RFC-0066): a policy-denied
                // route used to fail silently as a downstream "unreachable" that had
                // to be hunted. Now it names the requester + target at the source —
                // and only ONCE per (from -> to) pair, so a retrying client does not
                // bury the log in identical lines.
                if route_deny_first_time(chan.svc_name, name) {
                    debug_write_bytes(b"!route-deny: ");
                    debug_write_str(chan.svc_name);
                    debug_write_bytes(b" -> ");
                    debug_write_bytes(name);
                    debug_write_bytes(b" (policy: missing ipc.core grant in base.toml?)\n");
                }
                route_reply::send_route_rsp(
                    chan,
                    nexus_abi::routing::STATUS_DENIED,
                    0,
                    0,
                    route_nonce,
                );
                continue;
            }

            let (status, send_slot, recv_slot) =
                match route_table.lookup_by_name(chan.svc_name.as_bytes(), name) {
                    Ok(route) => (nexus_abi::routing::STATUS_OK, route.send.slot, route.recv.slot),
                    // ADR-0057 + RFC-0093 §1: a dead-but-supervised target is not answered
                    // at all — the ask is PARKED and answered once the supervisor
                    // re-provisions the route. Answering STALE here is what made clients
                    // re-ask in a loop and flood init's control queue (P2 finding).
                    Err(crate::route_table::RouteError::TargetStale) => {
                        match crate::service_topology::ServiceId::from_name(name) {
                            Some(target) => {
                                let ask = crate::route_park::ParkedRoute {
                                    chan: chan_idx as u16,
                                    target,
                                    nonce: route_nonce,
                                };
                                if park.park(ask).is_ok() {
                                    continue;
                                }
                                crate::bootstrap::diag::emit_marker_atomic(
                                    &[
                                        b"init: FAIL route park overflow svc=",
                                        chan.svc_name.as_bytes(),
                                        b" -> ",
                                        name,
                                    ],
                                    None,
                                );
                                (nexus_abi::routing::STATUS_STALE, 0u32, 0u32)
                            }
                            None => (nexus_abi::routing::STATUS_STALE, 0u32, 0u32),
                        }
                    }
                    Err(_) => (nexus_abi::routing::STATUS_NOT_FOUND, 0u32, 0u32),
                };
            if name == b"statefsd" {
                // Persist diagnosis: the request log alone can't tell a
                // NOT_FOUND lookup from a downstream reply loss.
                if status == nexus_abi::routing::STATUS_OK {
                    debug_write_bytes(b"init: route statefsd OK send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_byte(b'\n');
                } else {
                    debug_write_bytes(b"init: route statefsd NOT_FOUND\n");
                }
            }
            if name == b"samgrd" && chan.svc_name == "selftest-client" {
                debug_write_bytes(b"init: route samgrd rsp status=0x");
                debug_write_hex(status as usize);
                debug_write_bytes(b" send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_bytes(b" recv=0x");
                debug_write_hex(recv_slot as usize);
                debug_write_byte(b'\n');
            }
            if name == b"rngd" && chan.svc_name == "selftest-client" {
                debug_write_bytes(b"init: route rngd rsp status=0x");
                debug_write_hex(status as usize);
                debug_write_bytes(b" send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_bytes(b" recv=0x");
                debug_write_hex(recv_slot as usize);
                debug_write_byte(b'\n');
            }
            if name == b"logd" && chan.svc_name == "selftest-client" {
                debug_write_bytes(b"init: route logd rsp status=0x");
                debug_write_hex(status as usize);
                debug_write_bytes(b" send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_bytes(b" recv=0x");
                debug_write_hex(recv_slot as usize);
                debug_write_byte(b'\n');
            }
            if name == b"updated" && chan.svc_name == "selftest-client" {
                debug_write_bytes(b"init: route updated rsp status=0x");
                debug_write_hex(status as usize);
                debug_write_bytes(b" send=0x");
                debug_write_hex(send_slot as usize);
                debug_write_bytes(b" recv=0x");
                debug_write_hex(recv_slot as usize);
                debug_write_byte(b'\n');
            }
            route_reply::send_route_rsp(chan, status, send_slot, recv_slot, route_nonce);
        }
        let next_due = match (respawner.next_due_ns(), supervision.next_due_ns()) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        clock.arm(next_due);
        responder_clock::responder_idle(waitset);
        clock.drain();
        if let Some(limit) = watchdog {
            ticks = ticks.saturating_add(1);
            if ticks >= limit {
                fatal("init-lite: watchdog fired");
            }
        }
    }
}

/// Build a waitset over every control-channel request endpoint so the responder can block on
/// all of them at once. Returns `None` if waitsets are unavailable (host build, or the kernel
/// rejects creation) — the caller then falls back to a cooperative yield. Adds are best-effort:
/// a channel that fails to add is still serviced by the full NONBLOCK sweep on each wake.
/// Returns `true` only the first time a given `(svc -> target)` route denial is
/// seen, so the `!route-deny` marker logs once per pair instead of once per retry
/// (RFC-0066 "clean errors"). Bounded, lock-free, fail-open (logs if the table is
/// full — better a little extra noise than a swallowed error).
fn route_deny_first_time(svc: &str, target: &[u8]) -> bool {
    use core::sync::atomic::{AtomicU64, Ordering};
    const N: usize = 64;
    static SEEN: [AtomicU64; N] = [const { AtomicU64::new(0) }; N];

    // FNV-1a of `svc` + '>' + `target`.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in svc.as_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h ^= b'>' as u64;
    h = h.wrapping_mul(0x0000_0100_0000_01b3);
    for &b in target {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    if h == 0 {
        h = 1; // 0 is the "empty slot" sentinel
    }

    for slot in SEEN.iter() {
        let v = slot.load(Ordering::Relaxed);
        if v == h {
            return false; // already logged this pair
        }
        if v == 0 {
            match slot.compare_exchange(0, h, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => return true,                         // claimed → first time
                Err(claimed) if claimed == h => return false, // raced, same pair
                Err(_) => {} // claimed by a different pair → keep probing
            }
        }
    }
    true // table full → log anyway (fail-open)
}
