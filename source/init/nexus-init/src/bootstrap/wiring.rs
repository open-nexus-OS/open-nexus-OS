// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Per-service capability distribution — the bespoke + declarative
//! wiring phase init-lite runs after MMIO grants. Extracted verbatim from
//! `orchestrator::run_bootstrap` (RFC-0061 follow-up, task #100).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! ADR: docs/adr/0017-service-architecture.md

use crate::bootstrap::diag::iw;
use crate::bootstrap::endpoints::Endpoints;
use crate::bootstrap::CtrlChannel;
use crate::os_payload::*;

/// Distribute capabilities to every spawned service (the bespoke per-service
/// `match` + the declarative generic arm). Mutates each `CtrlChannel`'s slot
/// fields in place; the caller builds the route table from them afterward.
pub(crate) fn wire_services(
    ctrls: &mut [CtrlChannel],
    eps: &Endpoints,
    init_fold: bool,
    init_wire: &mut nexus_event::SpanTally,
) -> Result<()> {
    let Endpoints {
        vfs_req, vfs_rsp, pkg_req, pkg_rsp, pkg_reply_ep, pol_req, pol_rsp, bnd_req, bnd_rsp,
        bnd_rsp_updated, bnd_exe_req, bnd_exe_rsp, upd_req, upd_rsp, sam_req, sam_rsp, exe_req,
        exe_rsp, key_req, key_rsp, state_req, state_rsp, rng_req, rng_rsp, timed_req, timed_rsp,
        window_req, window_rsp, input_req, input_rsp, gpud_req, gpud_rsp, net_req, net_rsp,
        net_selftest_rsp, net_dsoft_rsp, dsoft_req, dsoft_rsp, dsoft_reply_ep, execd_reply_ep,
        reply_ep, log_req, log_rsp, metrics_req, metrics_rsp, ..
    } = *eps;

    // Services are suspended; they will be resumed atomically at the end
    // after all MMIO and IPC wiring is complete.
    let _ = nexus_abi::yield_();

    for chan in ctrls.iter_mut() {
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
                if iw(init_wire, init_fold, "init:netstackd") {
                    debug_write_bytes(b"init: wire netstackd xfer net_req RECV\n");
                }
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
                if iw(init_wire, init_fold, "init:netstackd") {
                    debug_write_bytes(b"init: wire netstackd xfer net_rsp SEND\n");
                }
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
                if iw(init_wire, init_fold, "init:netstackd") {
                    debug_write_bytes(b"init: netstackd svc slots recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_bytes(b" send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_byte(b'\n');
                }
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
                if iw(init_wire, init_fold, "init:dsoftbusd") {
                    debug_write_bytes(b"init: dsoftbusd netstackd slots send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_byte(b'\n');
                }

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
                if iw(init_wire, init_fold, "init:dsoftbusd") {
                    debug_write_bytes(b"init: dsoftbusd reply slots recv=0x");
                    debug_write_hex(reply_recv_slot as usize);
                    debug_write_bytes(b" send=0x");
                    debug_write_hex(reply_send_slot as usize);
                    debug_write_byte(b'\n');
                }

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
                if iw(init_wire, init_fold, "init:dsoftbusd") {
                    debug_write_bytes(b"init: dsoftbusd packagefsd slots send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_byte(b'\n');
                }
                // #endregion

                // TASK-0017 closeout: allow dsoftbusd to proxy remote statefs via statefsd.
                let send_slot = nexus_abi::cap_transfer(pid, state_req, Rights::SEND)
                    .map_err(InitError::Abi)?;
                let recv_slot = nexus_abi::cap_transfer(pid, state_rsp, Rights::RECV)
                    .map_err(InitError::Abi)?;
                chan.state_send_slot = Some(send_slot);
                chan.state_recv_slot = Some(recv_slot);
                // #region agent log
                if iw(init_wire, init_fold, "init:dsoftbusd") {
                    debug_write_bytes(b"init: dsoftbusd statefsd slots send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_byte(b'\n');
                }
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
                    if iw(init_wire, init_fold, "init:policyd") {
                        debug_write_bytes(b"init: policyd already priority-wired, skip\n");
                    }
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
                    if iw(init_wire, init_fold, "init:policyd") {
                        debug_write_bytes(b"init: policyd slots recv=0x");
                        debug_write_hex(recv_slot as usize);
                        debug_write_bytes(b" send=0x");
                        debug_write_hex(send_slot as usize);
                        debug_write_byte(b'\n');
                    }

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
                    if iw(init_wire, init_fold, "init:policyd") {
                        debug_write_bytes(b"init: policyd reply slots recv=0x");
                        debug_write_hex(reply_recv_slot as usize);
                        debug_write_bytes(b" send=0x");
                        debug_write_hex(reply_send_slot as usize);
                        debug_write_byte(b'\n');
                    }

                    // TASK-0006: allow policyd to send structured logs to logd via CAP_MOVE (reply inbox).
                    if let Some(req) = log_req {
                        let send_slot = nexus_abi::cap_transfer(pid, req, Rights::SEND)
                            .map_err(InitError::Abi)?;
                        chan.log_send_slot = Some(send_slot);
                        chan.log_recv_slot = Some(reply_recv_slot);
                        if iw(init_wire, init_fold, "init:policyd") {
                            debug_write_bytes(b"init: policyd logd slots send=0x");
                            debug_write_hex(send_slot as usize);
                            debug_write_bytes(b" recv=0x");
                            debug_write_hex(reply_recv_slot as usize);
                            debug_write_byte(b'\n');
                        }
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
                if iw(init_wire, init_fold, "init:bundlemgrd") {
                    debug_write_bytes(b"init: bundlemgrd slots recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_bytes(b" send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_byte(b'\n');
                }

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
                if iw(init_wire, init_fold, "init:updated") {
                    debug_write_bytes(b"init: updated slots recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_bytes(b" send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_byte(b'\n');
                }

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
                    if iw(init_wire, init_fold, "init:updated") {
                        debug_write_bytes(b"init: updated statefsd send slot=0x");
                        debug_write_hex(send_slot as usize);
                        debug_write_byte(b'\n');
                    }
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
                    if iw(init_wire, init_fold, "init:updated") {
                        debug_write_bytes(b"init: updated reply recv slot=0x");
                        debug_write_hex(reply_recv_slot as usize);
                        debug_write_byte(b'\n');
                    }
                    if iw(init_wire, init_fold, "init:updated") {
                        debug_write_bytes(b"init: updated reply send slot=0x");
                        debug_write_hex(reply_send_slot as usize);
                        debug_write_byte(b'\n');
                    }
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
                if iw(init_wire, init_fold, "init:samgrd") {
                    debug_write_bytes(b"init: samgrd slots recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_bytes(b" send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_byte(b'\n');
                }

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
                if iw(init_wire, init_fold, "init:execd") {
                    debug_write_bytes(b"init: execd reply slots recv=0x");
                    debug_write_hex(reply_recv_slot as usize);
                    debug_write_bytes(b" send=0x");
                    debug_write_hex(reply_send_slot as usize);
                    debug_write_byte(b'\n');
                }

                // Optional: allow execd to send crash reports to logd via CAP_MOVE (reply inbox).
                if let Some(req) = log_req {
                    let send_slot =
                        nexus_abi::cap_transfer(pid, req, Rights::SEND).map_err(InitError::Abi)?;
                    chan.log_send_slot = Some(send_slot);
                    chan.log_recv_slot = Some(reply_recv_slot);
                    if iw(init_wire, init_fold, "init:execd") {
                        debug_write_bytes(b"init: execd logd slots send=0x");
                        debug_write_hex(send_slot as usize);
                        debug_write_bytes(b" recv=0x");
                        debug_write_hex(reply_recv_slot as usize);
                        debug_write_byte(b'\n');
                    }
                }
            }
            "keystored" => {
                // #region agent log (keystored arm entry)
                if iw(init_wire, init_fold, "init:keystored") {
                    debug_write_bytes(b"init: ks arm\n");
                }
                // #endregion agent log
                // #region agent log (keystored wire-up tracing)
                if iw(init_wire, init_fold, "init:keystored") {
                    debug_write_bytes(b"init: wire keystored xfer key_req RECV cap=0x");
                    debug_write_hex(key_req as usize);
                    debug_write_byte(b'\n');
                }
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
                if iw(init_wire, init_fold, "init:keystored") {
                    debug_write_bytes(b"init: wire keystored xfer key_rsp SEND cap=0x");
                    debug_write_hex(key_rsp as usize);
                    debug_write_byte(b'\n');
                }
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
                if iw(init_wire, init_fold, "init:keystored") {
                    debug_write_bytes(b"init: wire keystored create reply_ep\n");
                }
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
                if iw(init_wire, init_fold, "init:keystored") {
                    debug_write_bytes(b"init: wire keystored xfer reply_ep RECV cap=0x");
                    debug_write_hex(reply_ep as usize);
                    debug_write_byte(b'\n');
                }
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
                if iw(init_wire, init_fold, "init:keystored") {
                    debug_write_bytes(b"init: wire keystored xfer reply_ep SEND cap=0x");
                    debug_write_hex(reply_ep as usize);
                    debug_write_byte(b'\n');
                }
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
                if iw(init_wire, init_fold, "init:keystored") {
                    debug_write_bytes(b"init: wire keystored xfer state_req SEND cap=0x");
                    debug_write_hex(state_req as usize);
                    debug_write_byte(b'\n');
                }
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
                if iw(init_wire, init_fold, "init:keystored") {
                    debug_write_bytes(b"init: wire keystored xfer pol_req SEND cap=0x");
                    debug_write_hex(pol_req as usize);
                    debug_write_byte(b'\n');
                }
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
                if iw(init_wire, init_fold, "init:keystored") {
                    debug_write_bytes(b"init: wire keystored xfer rng_req SEND cap=0x");
                    debug_write_hex(rng_req as usize);
                    debug_write_byte(b'\n');
                }
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
                if iw(init_wire, init_fold, "init:statefsd") {
                    debug_write_bytes(b"init: statefsd slots recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_bytes(b" send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_byte(b'\n');
                }

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
                if iw(init_wire, init_fold, "init:hidrawd") {
                    debug_write_bytes(b"init: hidrawd inputd slots send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_byte(b'\n');
                }
            }
            "gpud" => {
                let recv_slot = try_transfer(pid, gpud_req, Rights::RECV, "gpud", "RECV");
                let send_slot = try_transfer(pid, gpud_rsp, Rights::SEND, "gpud", "SEND");
                if let (Some(recv), Some(send)) = (recv_slot, send_slot) {
                    chan.gpud_send_slot = Some(send);
                    chan.gpud_recv_slot = Some(recv);
                    if iw(init_wire, init_fold, "init:gpud") {
                        debug_write_bytes(b"init: gpud slots recv=0x");
                        debug_write_hex(recv as usize);
                        debug_write_bytes(b" send=0x");
                        debug_write_hex(send as usize);
                        debug_write_byte(b'\n');
                    }
                }
            }
            "windowd" => {
                // Already priority-wired before MMIO grants — skip re-wiring.
                if chan.window_send_slot.is_some() && chan.window_recv_slot.is_some() {
                    if iw(init_wire, init_fold, "init:windowd") {
                        debug_write_bytes(b"init: windowd already priority-wired, skip\n");
                    }
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
                if iw(init_wire, init_fold, "init:windowd") {
                    debug_write_bytes(b"init: windowd slots recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_bytes(b" send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_byte(b'\n');
                }
                if let (Some(gpud_send), Some(gpud_recv)) = (gpud_send_slot, gpud_recv_slot) {
                    if iw(init_wire, init_fold, "init:windowd") {
                        debug_write_bytes(b"init: windowd gpud slots send=0x");
                        debug_write_hex(gpud_send as usize);
                        debug_write_bytes(b" recv=0x");
                        debug_write_hex(gpud_recv as usize);
                        debug_write_byte(b'\n');
                    }
                }
            }
            "inputd" => {
                if chan.input_send_slot.is_some() && chan.input_recv_slot.is_some() {
                    if iw(init_wire, init_fold, "init:inputd") {
                        debug_write_bytes(b"init: inputd already priority-wired, skip\n");
                    }
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
                        if iw(init_wire, init_fold, "init:inputd") {
                            debug_write_bytes(b"init: inputd windowd slots send=0x");
                            debug_write_hex(window_send as usize);
                            debug_write_bytes(b" recv=0x");
                            debug_write_hex(window_recv as usize);
                            debug_write_byte(b'\n');
                        }
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
                if iw(init_wire, init_fold, "init:inputd") {
                    debug_write_bytes(b"init: inputd slots recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_bytes(b" send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_byte(b'\n');
                }
                if iw(init_wire, init_fold, "init:inputd") {
                    debug_write_bytes(b"init: inputd windowd slots send=0x");
                    debug_write_hex(window_send_slot as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(window_recv_slot as usize);
                    debug_write_byte(b'\n');
                }
            }
            "metricsd" => {
                if let (Some(req), Some(rsp)) = (metrics_req, metrics_rsp) {
                    let recv_slot =
                        nexus_abi::cap_transfer(pid, req, Rights::RECV).map_err(InitError::Abi)?;
                    let send_slot =
                        nexus_abi::cap_transfer(pid, rsp, Rights::SEND).map_err(InitError::Abi)?;
                    chan.metrics_send_slot = Some(send_slot);
                    chan.metrics_recv_slot = Some(recv_slot);
                    if iw(init_wire, init_fold, "init:metricsd") {
                        debug_write_bytes(b"init: metricsd slots recv=0x");
                        debug_write_hex(recv_slot as usize);
                        debug_write_bytes(b" send=0x");
                        debug_write_hex(send_slot as usize);
                        debug_write_byte(b'\n');
                    }
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
                    if iw(init_wire, init_fold, "init:metricsd") {
                        debug_write_bytes(b"init: metricsd logd slots send=0x");
                        debug_write_hex(send_slot as usize);
                        debug_write_bytes(b" recv=0x");
                        debug_write_hex(reply_recv_slot as usize);
                        debug_write_byte(b'\n');
                    }
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
                    if iw(init_wire, init_fold, "init:logd") {
                        debug_write_bytes(b"init: logd slots recv=0x");
                        debug_write_hex(recv_slot as usize);
                        debug_write_bytes(b" send=0x");
                        debug_write_hex(send_slot as usize);
                        debug_write_byte(b'\n');
                    }
                }
            }
            "selftest-client" => {
                let send_slot =
                    nexus_abi::cap_transfer(pid, vfs_req, Rights::SEND).map_err(InitError::Abi)?;
                let recv_slot =
                    nexus_abi::cap_transfer(pid, vfs_rsp, Rights::RECV).map_err(InitError::Abi)?;
                chan.vfs_send_slot = Some(send_slot);
                chan.vfs_recv_slot = Some(recv_slot);
                if iw(init_wire, init_fold, "init:selftest-client") {
                    debug_write_bytes(b"init: selftest vfsd slots send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_byte(b'\n');
                }
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
                if iw(init_wire, init_fold, "init:selftest-client") {
                    debug_write_bytes(b"init: selftest policyd slots send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_byte(b'\n');
                }
                let send_slot =
                    nexus_abi::cap_transfer(pid, bnd_req, Rights::SEND).map_err(InitError::Abi)?;
                let recv_slot =
                    nexus_abi::cap_transfer(pid, bnd_rsp, Rights::RECV).map_err(InitError::Abi)?;
                chan.bnd_send_slot = Some(send_slot);
                chan.bnd_recv_slot = Some(recv_slot);
                if iw(init_wire, init_fold, "init:selftest-client") {
                    debug_write_bytes(b"init: selftest bundlemgrd slots send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_byte(b'\n');
                }
                let send_slot =
                    nexus_abi::cap_transfer(pid, upd_req, Rights::SEND).map_err(InitError::Abi)?;
                let recv_slot =
                    nexus_abi::cap_transfer(pid, upd_rsp, Rights::RECV).map_err(InitError::Abi)?;
                chan.upd_send_slot = Some(send_slot);
                chan.upd_recv_slot = Some(recv_slot);
                if iw(init_wire, init_fold, "init:selftest-client") {
                    debug_write_bytes(b"init: selftest updated slots send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_byte(b'\n');
                }
                let send_slot =
                    nexus_abi::cap_transfer(pid, sam_req, Rights::SEND).map_err(InitError::Abi)?;
                let recv_slot =
                    nexus_abi::cap_transfer(pid, sam_rsp, Rights::RECV).map_err(InitError::Abi)?;
                chan.sam_send_slot = Some(send_slot);
                chan.sam_recv_slot = Some(recv_slot);
                if iw(init_wire, init_fold, "init:selftest-client") {
                    debug_write_bytes(b"init: selftest samgrd slots send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_byte(b'\n');
                }
                let send_slot =
                    nexus_abi::cap_transfer(pid, exe_req, Rights::SEND).map_err(InitError::Abi)?;
                let recv_slot =
                    nexus_abi::cap_transfer(pid, exe_rsp, Rights::RECV).map_err(InitError::Abi)?;
                chan.exe_send_slot = Some(send_slot);
                chan.exe_recv_slot = Some(recv_slot);
                if iw(init_wire, init_fold, "init:selftest-client") {
                    debug_write_bytes(b"init: selftest execd slots send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_byte(b'\n');
                }
                let send_slot =
                    nexus_abi::cap_transfer(pid, key_req, Rights::SEND).map_err(InitError::Abi)?;
                let recv_slot =
                    nexus_abi::cap_transfer(pid, key_rsp, Rights::RECV).map_err(InitError::Abi)?;
                chan.key_send_slot = Some(send_slot);
                chan.key_recv_slot = Some(recv_slot);
                if iw(init_wire, init_fold, "init:selftest-client") {
                    debug_write_bytes(b"init: selftest keystored slots send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_byte(b'\n');
                }

                let send_slot = nexus_abi::cap_transfer(pid, state_req, Rights::SEND)
                    .map_err(InitError::Abi)?;
                let recv_slot = nexus_abi::cap_transfer(pid, state_rsp, Rights::RECV)
                    .map_err(InitError::Abi)?;
                chan.state_send_slot = Some(send_slot);
                chan.state_recv_slot = Some(recv_slot);
                if iw(init_wire, init_fold, "init:selftest-client") {
                    debug_write_bytes(b"init: selftest statefsd slots send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_byte(b'\n');
                }

                if let (Some(req), Some(rsp)) = (log_req, log_rsp) {
                    let send_slot =
                        nexus_abi::cap_transfer(pid, req, Rights::SEND).map_err(InitError::Abi)?;
                    let recv_slot =
                        nexus_abi::cap_transfer(pid, rsp, Rights::RECV).map_err(InitError::Abi)?;
                    chan.log_send_slot = Some(send_slot);
                    chan.log_recv_slot = Some(recv_slot);
                    if iw(init_wire, init_fold, "init:selftest-client") {
                        debug_write_bytes(b"init: selftest logd slots send=0x");
                        debug_write_hex(send_slot as usize);
                        debug_write_bytes(b" recv=0x");
                        debug_write_hex(recv_slot as usize);
                        debug_write_byte(b'\n');
                    }
                }
                if let (Some(req), Some(rsp)) = (metrics_req, metrics_rsp) {
                    let send_slot = nexus_abi::cap_transfer_to_slot(pid, req, Rights::SEND, 0x21)
                        .map_err(InitError::Abi)?;
                    let recv_slot = nexus_abi::cap_transfer_to_slot(pid, rsp, Rights::RECV, 0x22)
                        .map_err(InitError::Abi)?;
                    chan.metrics_send_slot = Some(send_slot);
                    chan.metrics_recv_slot = Some(recv_slot);
                    if iw(init_wire, init_fold, "init:selftest-client") {
                        debug_write_bytes(b"init: selftest metricsd slots send=0x");
                        debug_write_hex(send_slot as usize);
                        debug_write_bytes(b" recv=0x");
                        debug_write_hex(recv_slot as usize);
                        debug_write_byte(b'\n');
                    }
                }

                // Reply inbox: provide both RECV (stay with client) and SEND (to be moved to servers).
                let reply_recv_slot =
                    nexus_abi::cap_transfer(pid, reply_ep, Rights::RECV).map_err(InitError::Abi)?;
                let reply_send_slot =
                    nexus_abi::cap_transfer(pid, reply_ep, Rights::SEND).map_err(InitError::Abi)?;
                chan.reply_recv_slot = Some(reply_recv_slot);
                chan.reply_send_slot = Some(reply_send_slot);
                if iw(init_wire, init_fold, "init:selftest-client") {
                    debug_write_bytes(b"init: selftest reply slots send=0x");
                    debug_write_hex(reply_send_slot as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(reply_recv_slot as usize);
                    debug_write_byte(b'\n');
                }

                let send_slot = nexus_abi::cap_transfer(pid, input_req, Rights::SEND)
                    .map_err(InitError::Abi)?;
                chan.input_send_slot = Some(send_slot);
                chan.input_recv_slot = Some(reply_recv_slot);
                if iw(init_wire, init_fold, "init:selftest-client") {
                    debug_write_bytes(b"init: selftest inputd slots send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(reply_recv_slot as usize);
                    debug_write_byte(b'\n');
                }

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
                if iw(init_wire, init_fold, "init:selftest-client") {
                    debug_write_bytes(b"init: selftest rngd slots send=0x");
                    debug_write_hex(send_slot as usize);
                    debug_write_bytes(b" recv=0x");
                    debug_write_hex(recv_slot as usize);
                    debug_write_byte(b'\n');
                }

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
    Ok(())
}

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
            // (emitted from a post-bootstrap helper, outside run_bootstrap's init_wire scope — left raw)
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
