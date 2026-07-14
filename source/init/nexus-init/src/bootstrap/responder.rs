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

use crate::bootstrap::CtrlChannel;
use crate::route_table::RouteTable;
use alloc::vec::Vec;
use nexus_ipc::reqrep::FrameStash;

/// Run the routing responder loop forever. Only returns via `fatal()` on watchdog expiry.
pub(crate) fn run_responder_loop(
    ctrl_channels: Vec<CtrlChannel>,
    route_table: RouteTable,
    pol_ctl_route_req: u32,
    pol_ctl_route_rsp: u32,
    pol_ctl_exec_req: u32,
    pol_ctl_exec_rsp: u32,
    upd_req: u32,
    upd_reply_send: u32,
    upd_reply_recv: u32,
    mut upd_pending: FrameStash<8, 16>,
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
    let waitset = build_ctrl_waitset(&ctrl_channels);
    loop {
        for chan in &ctrl_channels {
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
                Err(_) => continue,
            };
            if chan.svc_name == "updated" {
                debug_write_bytes(b"init: ctrl req from updated\n");
            }
            // Health gate: allow selftest-client to notify init.
            if chan.svc_name == "selftest-client" && decode_init_health_ok_req(&buf[..n]) {
                let nonce = decode_init_health_ok_req_with_optional_nonce(&buf[..n]).flatten();
                let status = match updated_health_ok(
                    &mut upd_pending,
                    upd_req,
                    upd_reply_send,
                    upd_reply_recv,
                ) {
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
                    }
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
                // Identity-gated: execd only.
                let (status, send_slot, recv_slot) = if chan.svc_name == "execd" {
                    match nexus_abi::ipc_endpoint_create_for(
                        ENDPOINT_FACTORY_CAP_SLOT,
                        chan.pid,
                        8,
                    ) {
                        Ok(ep) => {
                            let send = nexus_abi::cap_transfer(
                                chan.pid,
                                ep,
                                nexus_abi::Rights::SEND,
                            );
                            let recv = nexus_abi::cap_transfer(
                                chan.pid,
                                ep,
                                nexus_abi::Rights::RECV,
                            );
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
                    debug_write_bytes(b"init: mint-pair denied (not execd)\n");
                    (nexus_abi::routing::STATUS_NOT_FOUND, 0, 0)
                };
                if let Some(nonce) = route_nonce {
                    let base = nexus_abi::routing::encode_route_rsp(status, send_slot, recv_slot);
                    let mut rsp = [0u8; 17];
                    rsp[..13].copy_from_slice(&base);
                    rsp[13..17].copy_from_slice(&nonce.to_le_bytes());
                    let rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, rsp.len() as u32);
                    let _ = nexus_abi::ipc_send_v1(
                        chan.ctrl_rsp_parent_slot,
                        &rh,
                        &rsp,
                        nexus_abi::IPC_SYS_NONBLOCK,
                        0,
                    );
                } else {
                    let rsp = nexus_abi::routing::encode_route_rsp(status, send_slot, recv_slot);
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
            if name == b"@reply" {
                let status = if chan.reply_send_slot.is_some() && chan.reply_recv_slot.is_some() {
                    nexus_abi::routing::STATUS_OK
                } else {
                    nexus_abi::routing::STATUS_NOT_FOUND
                };
                let send_slot = chan.reply_send_slot.unwrap_or(0);
                let recv_slot = chan.reply_recv_slot.unwrap_or(0);
                if let Some(nonce) = route_nonce {
                    let base = nexus_abi::routing::encode_route_rsp(status, send_slot, recv_slot);
                    let mut rsp = [0u8; 17];
                    rsp[..13].copy_from_slice(&base);
                    rsp[13..17].copy_from_slice(&nonce.to_le_bytes());
                    let rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, rsp.len() as u32);
                    let _ = nexus_abi::ipc_send_v1(
                        chan.ctrl_rsp_parent_slot,
                        &rh,
                        &rsp,
                        nexus_abi::IPC_SYS_NONBLOCK,
                        0,
                    );
                } else {
                    let rsp = nexus_abi::routing::encode_route_rsp(status, send_slot, recv_slot);
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
            let allowed = if name == chan.svc_name.as_bytes() {
                true
            } else if chan.svc_name == "policyd" {
                true
            } else if chan.svc_name == "bundlemgrd" && name == b"execd" {
                policyd_route_allowed(pol_ctl_route_req, pol_ctl_route_rsp, chan.svc_name, name)
                    .unwrap_or(false)
            } else {
                policyd_route_allowed(pol_ctl_route_req, pol_ctl_route_rsp, chan.svc_name, name)
                    .unwrap_or(true)
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
                if let Some(nonce) = route_nonce {
                    let base = nexus_abi::routing::encode_route_rsp(
                        nexus_abi::routing::STATUS_DENIED,
                        0,
                        0,
                    );
                    let mut rsp = [0u8; 17];
                    rsp[..13].copy_from_slice(&base);
                    rsp[13..17].copy_from_slice(&nonce.to_le_bytes());
                    let rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, rsp.len() as u32);
                    let _ = nexus_abi::ipc_send_v1(
                        chan.ctrl_rsp_parent_slot,
                        &rh,
                        &rsp,
                        nexus_abi::IPC_SYS_NONBLOCK,
                        0,
                    );
                } else {
                    let rsp = nexus_abi::routing::encode_route_rsp(
                        nexus_abi::routing::STATUS_DENIED,
                        0,
                        0,
                    );
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

            let (status, send_slot, recv_slot) =
                match route_table.lookup_by_name(chan.svc_name.as_bytes(), name) {
                    Ok(route) => (nexus_abi::routing::STATUS_OK, route.send.slot, route.recv.slot),
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
            if let Some(nonce) = route_nonce {
                let base = nexus_abi::routing::encode_route_rsp(status, send_slot, recv_slot);
                let mut rsp = [0u8; 17];
                rsp[..13].copy_from_slice(&base);
                rsp[13..17].copy_from_slice(&nonce.to_le_bytes());
                let rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, rsp.len() as u32);
                let _ = nexus_abi::ipc_send_v1(
                    chan.ctrl_rsp_parent_slot,
                    &rh,
                    &rsp,
                    nexus_abi::IPC_SYS_NONBLOCK,
                    0,
                );
            } else {
                let rsp = nexus_abi::routing::encode_route_rsp(status, send_slot, recv_slot);
                let rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, rsp.len() as u32);
                let _ = nexus_abi::ipc_send_v1(
                    chan.ctrl_rsp_parent_slot,
                    &rh,
                    &rsp,
                    nexus_abi::IPC_SYS_NONBLOCK,
                    0,
                );
            }
        }
        responder_idle(waitset);
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
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn build_ctrl_waitset(ctrl_channels: &[CtrlChannel]) -> Option<nexus_abi::Cap> {
    let ws = nexus_abi::waitset_create().ok()?;
    for chan in ctrl_channels {
        let _ = nexus_abi::waitset_add(ws, chan.ctrl_req_parent_slot);
    }
    Some(ws)
}

#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
fn build_ctrl_waitset(_ctrl_channels: &[CtrlChannel]) -> Option<u32> {
    None
}

/// Reactive idle for the responder loop: block until a control channel is ready (bounded by a
/// 1s safety-net deadline, since the sweep already drains every channel), or fall back to a
/// cooperative yield when no waitset is available.
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn responder_idle(waitset: Option<nexus_abi::Cap>) {
    const IDLE_SAFETY_NET_NS: u64 = 1_000_000_000;
    match waitset {
        Some(ws) => {
            let _ = nexus_abi::waitset_wait(ws, IDLE_SAFETY_NET_NS);
        }
        None => {
            let _ = nexus_abi::yield_();
        }
    }
}

#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
fn responder_idle(_waitset: Option<u32>) {
    let _ = nexus_abi::yield_();
}

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
                Ok(_) => return true,                       // claimed → first time
                Err(claimed) if claimed == h => return false, // raced, same pair
                Err(_) => {} // claimed by a different pair → keep probing
            }
        }
    }
    true // table full → log anyway (fail-open)
}
