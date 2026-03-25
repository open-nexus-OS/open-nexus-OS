// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Cross-VM DSoftBus orchestration runner (discovery + session establishment)
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Host session seam tests + QEMU 2-VM marker proof (`tools/os2vm.sh`)
//! ADR: docs/adr/0005-dsoftbus-architecture.md

use alloc::string::String;
use alloc::vec::Vec;
use nexus_abi::yield_;
use nexus_discovery_packet::{decode_announce_v1, encode_announce_v1, AnnounceV1};
use nexus_ipc::reqrep::ReplyBuffer;
use nexus_ipc::KernelClient;
use nexus_peer_lru::{PeerEntry, PeerLru};

use crate::os::discovery::state::{get_peer_ip, set_peer_ip, DISC_PORT, MCAST_IP};
use crate::os::entry::{DSOFT_REPLY_RECV_SLOT, DSOFT_REPLY_SEND_SLOT};
use crate::os::entry_pure::{OS2VM_NODE_A_IP, OS2VM_NODE_B_IP, QEMU_USERNET_FALLBACK_IP};
use crate::os::netstack::{
    next_nonce, rpc_nonce, tcp_listen, udp_bind, udp_send_to, CrossVmTransport, SessionId,
    UdpSocketId, STATUS_IO, STATUS_WOULD_BLOCK,
};
use crate::os::session::fsm::SessionFsm;
use crate::os::session::handshake::derive_test_secret;
use crate::os::session::steps::{
    identity_binding_matches, on_handshake_failure, should_poll_discovery, should_send_announce,
};

const MAGIC0: u8 = b'N';
const MAGIC1: u8 = b'S';
const VERSION: u8 = 1;
const OP_UDP_RECV_FROM: u8 = 8;
const STATUS_OK: u8 = 0;
const STATUS_NOT_FOUND: u8 = 1;
const STATUS_MALFORMED: u8 = 2;
const STATUS_TIMED_OUT: u8 = 5;

pub(crate) fn run_cross_vm_main(
    net: &KernelClient,
    local_ip: [u8; 4],
) -> core::result::Result<(), ()> {
    let (device_id, listen_port, peer_ip, peer_port, peer_device_id, key_tag_self, key_tag_peer) =
        if local_ip == OS2VM_NODE_A_IP {
            ("node-a", 34_567u16, OS2VM_NODE_B_IP, 34_568u16, "node-b", 0xD0u8, 0xD1u8)
        } else {
            ("node-b", 34_568u16, OS2VM_NODE_A_IP, 34_567u16, "node-a", 0xD1u8, 0xD0u8)
        };

    let mut nonce_ctr: u64 = 1;
    let mut pending_replies: ReplyBuffer<16, 512> = ReplyBuffer::new();

    let udp_id = {
        let mut out: Option<UdpSocketId> = None;
        for _ in 0..50_000 {
            if let Ok(id) = udp_bind(
                &mut pending_replies,
                &mut nonce_ctr,
                net,
                local_ip,
                DISC_PORT,
                DSOFT_REPLY_RECV_SLOT,
                DSOFT_REPLY_SEND_SLOT,
            ) {
                out = Some(id);
                break;
            }
            let _ = yield_();
        }
        out.ok_or(())?
    };
    let _ = nexus_abi::debug_println("dsoftbusd: discovery cross-vm up");

    let mut peers = PeerLru::with_default_capacity();
    let mut peer_ips: Vec<(String, [u8; 4])> = Vec::new();

    let lid = tcp_listen(
        &mut pending_replies,
        &mut nonce_ctr,
        net,
        local_ip,
        listen_port,
        DSOFT_REPLY_RECV_SLOT,
        DSOFT_REPLY_SEND_SLOT,
    )?;

    let is_initiator = device_id == "node-a";
    let mut fsm: SessionFsm<SessionId> = SessionFsm::new();
    fsm.set_listening();
    let mut announced_once = false;
    let mut announce_send_failed = false;
    let mut udp_recv_failed = false;
    let mut dial_logged = false;
    let mut accept_logged = false;
    let mut dial_attempts: u32 = 0;
    let mut accept_attempts: u32 = 0;
    let mut dial_fallback_logged = false;
    let mut dial_status_would_block_logged = false;
    let mut dial_status_io_logged = false;
    let mut dial_status_rpc_err_logged = false;
    let mut dial_status_parse_err_logged = false;
    let mut dial_status_not_found_logged = false;
    let mut dial_status_malformed_logged = false;
    let mut dial_status_timed_out_logged = false;
    let mut dial_status_other_logged = false;
    let mut dial_target_mode_logged = false;
    let mut dial_target_ip_logged = false;
    let mut dial_target_port_logged = false;
    let mut accept_pending_logged = false;
    let mut dial_connected_logged = false;
    let mut accept_connected_logged = false;
    let mut hs_init_write_msg1_fail_logged = false;
    let mut hs_init_read_msg2_fail_logged = false;
    let mut hs_resp_read_msg1_fail_logged = false;
    let mut hs_resp_write_msg2_fail_logged = false;
    let session_setup_start_ns = nexus_abi::nsec().ok().unwrap_or(0);
    let mut session_fail_counted = false;

    let mut transport = 'session_setup: loop {
        loop {
            let now = (nexus_abi::nsec().unwrap_or(0) / 1_000_000) as u64;
            if should_send_announce(announced_once, now) {
                let ann = AnnounceV1 {
                    device_id: String::from(device_id),
                    port: listen_port,
                    // SECURITY: bring-up test keys, NOT production custody
                    noise_static: nexus_noise_xk::StaticKeypair::from_secret(derive_test_secret(
                        key_tag_self,
                        listen_port,
                    ))
                    .public,
                    services: alloc::vec!["samgrd".into(), "bundlemgrd".into()],
                };
                if let Ok(bytes) = encode_announce_v1(&ann) {
                    let ok1 = udp_send_to(
                        &mut pending_replies,
                        &mut nonce_ctr,
                        net,
                        udp_id,
                        MCAST_IP,
                        DISC_PORT,
                        &bytes,
                        DSOFT_REPLY_RECV_SLOT,
                        DSOFT_REPLY_SEND_SLOT,
                    )
                    .is_ok();
                    let ok2 = udp_send_to(
                        &mut pending_replies,
                        &mut nonce_ctr,
                        net,
                        udp_id,
                        peer_ip,
                        DISC_PORT,
                        &bytes,
                        DSOFT_REPLY_RECV_SLOT,
                        DSOFT_REPLY_SEND_SLOT,
                    )
                    .is_ok();
                    if !(ok1 && ok2) && !announce_send_failed {
                        announce_send_failed = true;
                    }
                    if !announced_once {
                        let _ = nexus_abi::debug_println("dsoftbusd: discovery announce sent");
                        announced_once = true;
                    }
                }
            }

            let peer_known = peers.peek(peer_device_id).is_some()
                && get_peer_ip(&peer_ips, peer_device_id).is_some();
            let should_poll_discovery = should_poll_discovery(peer_known, now);
            if should_poll_discovery {
                let nonce = next_nonce(&mut nonce_ctr);
                let mut r = [0u8; 18];
                r[0] = MAGIC0;
                r[1] = MAGIC1;
                r[2] = VERSION;
                r[3] = OP_UDP_RECV_FROM;
                r[4..8].copy_from_slice(&udp_id.as_raw().to_le_bytes());
                r[8..10].copy_from_slice(&(256u16).to_le_bytes());
                r[10..18].copy_from_slice(&nonce.to_le_bytes());
                if let Ok(rsp) = rpc_nonce(
                    &mut pending_replies,
                    net,
                    &r,
                    OP_UDP_RECV_FROM | 0x80,
                    nonce,
                    DSOFT_REPLY_RECV_SLOT,
                    DSOFT_REPLY_SEND_SLOT,
                ) {
                    if rsp[0] == MAGIC0
                        && rsp[1] == MAGIC1
                        && rsp[2] == VERSION
                        && rsp[3] == (OP_UDP_RECV_FROM | 0x80)
                    {
                        if rsp[4] == STATUS_OK {
                            let n = u16::from_le_bytes([rsp[5], rsp[6]]) as usize;
                            let from_ip = [rsp[7], rsp[8], rsp[9], rsp[10]];
                            let base = 13;
                            if n <= 256 && base + n <= rsp.len() {
                                let payload = &rsp[base..base + n];
                                match decode_announce_v1(payload) {
                                    Ok(pkt) => {
                                        let entry = PeerEntry::new(
                                            pkt.device_id.clone(),
                                            pkt.port,
                                            pkt.noise_static,
                                            pkt.services,
                                        );
                                        peers.insert(entry);
                                        set_peer_ip(&mut peer_ips, &pkt.device_id, from_ip);
                                        if peers.peek(peer_device_id).is_some()
                                            && get_peer_ip(&peer_ips, peer_device_id).is_some()
                                        {
                                            let _ = nexus_abi::debug_println(
                                                "dsoftbusd: discovery peer learned",
                                            );
                                        }
                                    }
                                    Err(_) => {
                                        let _ = nexus_abi::debug_println(
                                            "dsoftbusd: announce ignored (malformed)",
                                        );
                                    }
                                }
                            }
                        } else if rsp[4] == STATUS_IO && !udp_recv_failed {
                            let _ = nexus_abi::debug_println("dsoftbusd: discovery recv FAIL");
                            udp_recv_failed = true;
                        }
                    }
                }
            }

            if fsm.sid().is_none() {
                if is_initiator {
                    fsm.set_dialing();
                    let mut ip = peer_ip;
                    let mut port = peer_port;
                    let mut used_discovery_mapping = false;
                    if let Some(peer) = peers.peek(peer_device_id) {
                        port = peer.port;
                        if let Some(mapped_ip) = get_peer_ip(&peer_ips, peer_device_id) {
                            ip = mapped_ip;
                            used_discovery_mapping = true;
                        }
                    }
                    if !used_discovery_mapping && !dial_fallback_logged {
                        dial_fallback_logged = true;
                        let _ =
                            nexus_abi::debug_println("dbg:dsoftbusd: dial fallback no-discovery");
                    }
                    if !dial_target_mode_logged {
                        dial_target_mode_logged = true;
                        // #region agent log
                        let _ = if used_discovery_mapping {
                            nexus_abi::debug_println("dbg:dsoftbusd: dial target mode discovery")
                        } else {
                            nexus_abi::debug_println("dbg:dsoftbusd: dial target mode fallback")
                        };
                        // #endregion
                    }
                    if !dial_target_ip_logged {
                        dial_target_ip_logged = true;
                        // #region agent log
                        let _ = if ip == OS2VM_NODE_A_IP {
                            nexus_abi::debug_println("dbg:dsoftbusd: dial target ip 10.42.0.10")
                        } else if ip == OS2VM_NODE_B_IP {
                            nexus_abi::debug_println("dbg:dsoftbusd: dial target ip 10.42.0.11")
                        } else if ip == QEMU_USERNET_FALLBACK_IP {
                            nexus_abi::debug_println("dbg:dsoftbusd: dial target ip 10.0.2.15")
                        } else {
                            nexus_abi::debug_println("dbg:dsoftbusd: dial target ip other")
                        };
                        // #endregion
                    }
                    if !dial_target_port_logged {
                        dial_target_port_logged = true;
                        // #region agent log
                        let _ = if port == 34_567 {
                            nexus_abi::debug_println("dbg:dsoftbusd: dial target port 34567")
                        } else if port == 34_568 {
                            nexus_abi::debug_println("dbg:dsoftbusd: dial target port 34568")
                        } else {
                            nexus_abi::debug_println("dbg:dsoftbusd: dial target port other")
                        };
                        // #endregion
                    }
                    if !dial_logged {
                        let _ = nexus_abi::debug_println("dsoftbusd: cross-vm dial start");
                        dial_logged = true;
                    }
                    dial_attempts = dial_attempts.wrapping_add(1);
                    if dial_attempts == 1 {
                        // #region agent log
                        let _ = nexus_abi::debug_println("dbg:dsoftbusd: dial attempts 1");
                        // #endregion
                    } else if dial_attempts == 512 {
                        // #region agent log
                        let _ = nexus_abi::debug_println("dbg:dsoftbusd: dial attempts 512");
                        // #endregion
                    } else if dial_attempts == 4096 {
                        // #region agent log
                        let _ = nexus_abi::debug_println("dbg:dsoftbusd: dial attempts 4096");
                        // #endregion
                    }
                    let mut netio = CrossVmTransport::new(
                        &mut pending_replies,
                        &mut nonce_ctr,
                        net,
                        DSOFT_REPLY_RECV_SLOT,
                        DSOFT_REPLY_SEND_SLOT,
                    );
                    match netio.connect(ip, port) {
                        Ok(s) => {
                            if !dial_connected_logged {
                                dial_connected_logged = true;
                                // #region agent log
                                let _ = nexus_abi::debug_println("dbg:dsoftbusd: dial connected");
                                // #endregion
                            }
                            fsm.set_connected(s);
                        }
                        Err(status) => {
                            match status {
                                STATUS_WOULD_BLOCK => {
                                    if !dial_status_would_block_logged {
                                        dial_status_would_block_logged = true;
                                        let _ = nexus_abi::debug_println(
                                            "dbg:dsoftbusd: dial status would_block",
                                        );
                                    }
                                }
                                STATUS_IO => {
                                    if !dial_status_io_logged {
                                        dial_status_io_logged = true;
                                        let _ = nexus_abi::debug_println(
                                            "dbg:dsoftbusd: dial status io",
                                        );
                                    }
                                }
                                0xfd => {
                                    if !dial_status_rpc_err_logged {
                                        dial_status_rpc_err_logged = true;
                                        let _ = nexus_abi::debug_println(
                                            "dbg:dsoftbusd: dial status rpc_err",
                                        );
                                    }
                                }
                                0xfe => {
                                    if !dial_status_parse_err_logged {
                                        dial_status_parse_err_logged = true;
                                        let _ = nexus_abi::debug_println(
                                            "dbg:dsoftbusd: dial status parse_err",
                                        );
                                    }
                                }
                                STATUS_NOT_FOUND => {
                                    if !dial_status_not_found_logged {
                                        dial_status_not_found_logged = true;
                                        // #region agent log
                                        let _ = nexus_abi::debug_println(
                                            "dbg:dsoftbusd: dial status not_found",
                                        );
                                        // #endregion
                                    }
                                }
                                STATUS_MALFORMED => {
                                    if !dial_status_malformed_logged {
                                        dial_status_malformed_logged = true;
                                        // #region agent log
                                        let _ = nexus_abi::debug_println(
                                            "dbg:dsoftbusd: dial status malformed",
                                        );
                                        // #endregion
                                    }
                                }
                                STATUS_TIMED_OUT => {
                                    if !dial_status_timed_out_logged {
                                        dial_status_timed_out_logged = true;
                                        // #region agent log
                                        let _ = nexus_abi::debug_println(
                                            "dbg:dsoftbusd: dial status timed_out",
                                        );
                                        // #endregion
                                    }
                                }
                                _ => {
                                    if !dial_status_other_logged {
                                        dial_status_other_logged = true;
                                        // #region agent log
                                        let _ = nexus_abi::debug_println(
                                            "dbg:dsoftbusd: dial status other",
                                        );
                                        // #endregion
                                    }
                                }
                            }
                        }
                    }
                } else {
                    fsm.set_accepting();
                    if !accept_logged {
                        let _ = nexus_abi::debug_println("dsoftbusd: cross-vm accept wait");
                        accept_logged = true;
                    }
                    accept_attempts = accept_attempts.wrapping_add(1);
                    if accept_attempts == 1 {
                        // #region agent log
                        let _ = nexus_abi::debug_println("dbg:dsoftbusd: accept attempts 1");
                        // #endregion
                    } else if accept_attempts == 512 {
                        // #region agent log
                        let _ = nexus_abi::debug_println("dbg:dsoftbusd: accept attempts 512");
                        // #endregion
                    } else if accept_attempts == 4096 {
                        // #region agent log
                        let _ = nexus_abi::debug_println("dbg:dsoftbusd: accept attempts 4096");
                        // #endregion
                    }
                    let mut netio = CrossVmTransport::new(
                        &mut pending_replies,
                        &mut nonce_ctr,
                        net,
                        DSOFT_REPLY_RECV_SLOT,
                        DSOFT_REPLY_SEND_SLOT,
                    );
                    match netio.accept(lid) {
                        Ok(s) => {
                            if !accept_connected_logged {
                                accept_connected_logged = true;
                                // #region agent log
                                let _ = nexus_abi::debug_println("dbg:dsoftbusd: accept connected");
                                // #endregion
                            }
                            fsm.set_connected(s);
                        }
                        Err(()) => {
                            if !accept_pending_logged {
                                accept_pending_logged = true;
                                let _ = nexus_abi::debug_println("dbg:dsoftbusd: accept pending");
                            }
                        }
                    }
                }
            }

            if fsm.sid().is_some() {
                break;
            }
            let _ = yield_();
        }
        let session_id = fsm.sid().ok_or(())?;
        fsm.set_handshaking();

        use nexus_noise_xk::{
            StaticKeypair, Transport, XkInitiator, XkResponder, MSG1_LEN, MSG2_LEN, MSG3_LEN,
        };

        let self_static = StaticKeypair::from_secret(derive_test_secret(key_tag_self, listen_port));
        let self_eph_seed = derive_test_secret(0xE0, listen_port);
        let peer_expected_pub =
            StaticKeypair::from_secret(derive_test_secret(key_tag_peer, peer_port)).public;

        let transport_attempt = (|| -> core::result::Result<Transport, ()> {
            let discovered = peers.peek(peer_device_id).map(|peer_entry| peer_entry.noise_static);
            if !identity_binding_matches(discovered, peer_expected_pub) {
                let _ = nexus_abi::debug_println("dsoftbusd: identity mismatch peer=crossvm");
                return Err(());
            }

            let transport = if is_initiator {
                let mut initiator = XkInitiator::new(self_static, peer_expected_pub, self_eph_seed);
                let mut msg1 = [0u8; MSG1_LEN];
                initiator.write_msg1(&mut msg1);
                let mut netio = CrossVmTransport::new(
                    &mut pending_replies,
                    &mut nonce_ctr,
                    net,
                    DSOFT_REPLY_RECV_SLOT,
                    DSOFT_REPLY_SEND_SLOT,
                );
                if netio.write_all(session_id, &msg1).is_err() {
                    if !hs_init_write_msg1_fail_logged {
                        hs_init_write_msg1_fail_logged = true;
                        let _ = nexus_abi::debug_println("dbg:dsoftbusd: hs init write msg1 fail");
                    }
                    return Err(());
                }

                let mut msg2 = [0u8; MSG2_LEN];
                let mut netio = CrossVmTransport::new(
                    &mut pending_replies,
                    &mut nonce_ctr,
                    net,
                    DSOFT_REPLY_RECV_SLOT,
                    DSOFT_REPLY_SEND_SLOT,
                );
                if netio.read_exact(session_id, &mut msg2).is_err() {
                    if !hs_init_read_msg2_fail_logged {
                        hs_init_read_msg2_fail_logged = true;
                        let _ = nexus_abi::debug_println("dbg:dsoftbusd: hs init read msg2 fail");
                    }
                    return Err(());
                }

                let mut msg3 = [0u8; MSG3_LEN];
                let keys = match initiator.read_msg2_write_msg3(&msg2, &mut msg3) {
                    Ok(k) => k,
                    Err(_) => return Err(()),
                };
                let mut netio = CrossVmTransport::new(
                    &mut pending_replies,
                    &mut nonce_ctr,
                    net,
                    DSOFT_REPLY_RECV_SLOT,
                    DSOFT_REPLY_SEND_SLOT,
                );
                if netio.write_all(session_id, &msg3).is_err() {
                    return Err(());
                }
                Transport::new(keys)
            } else {
                let mut responder = XkResponder::new(self_static, peer_expected_pub, self_eph_seed);
                let mut msg1 = [0u8; MSG1_LEN];
                let mut netio = CrossVmTransport::new(
                    &mut pending_replies,
                    &mut nonce_ctr,
                    net,
                    DSOFT_REPLY_RECV_SLOT,
                    DSOFT_REPLY_SEND_SLOT,
                );
                if netio.read_exact(session_id, &mut msg1).is_err() {
                    if !hs_resp_read_msg1_fail_logged {
                        hs_resp_read_msg1_fail_logged = true;
                        let _ = nexus_abi::debug_println("dbg:dsoftbusd: hs resp read msg1 fail");
                    }
                    return Err(());
                }
                let mut msg2 = [0u8; MSG2_LEN];
                if responder.read_msg1_write_msg2(&msg1, &mut msg2).is_err() {
                    return Err(());
                }
                let mut netio = CrossVmTransport::new(
                    &mut pending_replies,
                    &mut nonce_ctr,
                    net,
                    DSOFT_REPLY_RECV_SLOT,
                    DSOFT_REPLY_SEND_SLOT,
                );
                if netio.write_all(session_id, &msg2).is_err() {
                    if !hs_resp_write_msg2_fail_logged {
                        hs_resp_write_msg2_fail_logged = true;
                        let _ = nexus_abi::debug_println("dbg:dsoftbusd: hs resp write msg2 fail");
                    }
                    return Err(());
                }
                let mut msg3 = [0u8; MSG3_LEN];
                let mut netio = CrossVmTransport::new(
                    &mut pending_replies,
                    &mut nonce_ctr,
                    net,
                    DSOFT_REPLY_RECV_SLOT,
                    DSOFT_REPLY_SEND_SLOT,
                );
                if netio.read_exact(session_id, &mut msg3).is_err() {
                    return Err(());
                }
                let keys = match responder.read_msg3_finish(&msg3) {
                    Ok(k) => k,
                    Err(_) => return Err(()),
                };
                Transport::new(keys)
            };

            Ok(transport)
        })();

        match transport_attempt {
            Ok(transport) => {
                fsm.set_ready();
                let now = nexus_abi::nsec().ok().unwrap_or(session_setup_start_ns);
                crate::os::observability::metrics_counter_inc_best_effort("dsoftbusd.session.ok");
                crate::os::observability::metrics_hist_observe_best_effort(
                    "dsoftbusd.handshake.duration_ns",
                    now.saturating_sub(session_setup_start_ns),
                );
                break 'session_setup transport;
            }
            Err(()) => {
                if !session_fail_counted {
                    session_fail_counted = true;
                    crate::os::observability::metrics_counter_inc_best_effort(
                        "dsoftbusd.session.fail",
                    );
                }
                let action = on_handshake_failure(&mut fsm);
                if let Some(old_sid) = action.close_sid {
                    let mut netio = CrossVmTransport::new(
                        &mut pending_replies,
                        &mut nonce_ctr,
                        net,
                        DSOFT_REPLY_RECV_SLOT,
                        DSOFT_REPLY_SEND_SLOT,
                    );
                    let _ = netio.close(old_sid);
                    let _ = nexus_abi::debug_println("dbg:dsoftbusd: hs sid close");
                }
                if action.retry {
                    let _ = nexus_abi::debug_println("dbg:dsoftbusd: hs retry");
                    let _ = yield_();
                    continue 'session_setup;
                }
                return Err(());
            }
        }
    };
    let _ = nexus_abi::debug_println("dbg:dsoftbusd: hs transport ready");
    let sid = fsm.sid().ok_or(())?;

    let mut sess_buf = [0u8; 64];
    let mut pos = 0usize;
    let prefix = b"dsoftbusd: cross-vm session ok ";
    sess_buf[pos..pos + prefix.len()].copy_from_slice(prefix);
    pos += prefix.len();
    let peer_bytes = peer_device_id.as_bytes();
    let n = core::cmp::min(peer_bytes.len(), sess_buf.len() - pos);
    sess_buf[pos..pos + n].copy_from_slice(&peer_bytes[..n]);
    pos += n;
    if let Ok(s) = core::str::from_utf8(&sess_buf[..pos]) {
        let _ = nexus_abi::debug_println(s);
    }

    if !is_initiator {
        return crate::os::gateway::remote_proxy::run_remote_proxy_loop(
            &mut transport,
            &mut pending_replies,
            &mut nonce_ctr,
            net,
            sid,
            DSOFT_REPLY_RECV_SLOT,
            DSOFT_REPLY_SEND_SLOT,
        );
    }

    crate::os::gateway::local_ipc::run_local_ipc_loop(
        &mut transport,
        &mut pending_replies,
        &mut nonce_ctr,
        net,
        sid,
        DSOFT_REPLY_RECV_SLOT,
        DSOFT_REPLY_SEND_SLOT,
    )
}
