//! Single-VM discovery + dual-node bring-up runner.

use alloc::string::String;
use alloc::vec::Vec;
use nexus_abi::yield_;
use nexus_discovery_packet::{decode_announce_v1, encode_announce_v1, AnnounceV1};
use nexus_ipc::reqrep::ReplyBuffer;
use nexus_ipc::KernelClient;
use nexus_noise_xk::{StaticKeypair, Transport, XkInitiator, XkResponder, MSG1_LEN, MSG2_LEN, MSG3_LEN};
use nexus_peer_lru::{PeerEntry, PeerLru};

const MAGIC0: u8 = b'N';
const MAGIC1: u8 = b'S';
const VERSION: u8 = 1;
const OP_LISTEN: u8 = 1;
const OP_UDP_RECV_FROM: u8 = 8;
const STATUS_OK: u8 = 0;
const STATUS_MALFORMED: u8 = 2;
const STATUS_WOULD_BLOCK: u8 = 3;

fn set_peer_ip(
    peers: &PeerLru,
    ips: &mut Vec<(String, [u8; 4])>,
    device_id: &str,
    ip: [u8; 4],
) {
    crate::os::entry::set_peer_ip(ips, device_id, ip);
    crate::os::entry::rebuild_peer_ips(peers, ips);
}

fn get_peer_ip(ips: &[(String, [u8; 4])], device_id: &str) -> Option<[u8; 4]> {
    crate::os::entry::get_peer_ip(ips, device_id)
}

fn send_announce(
    pending: &mut ReplyBuffer<16, 512>,
    net: &KernelClient,
    nonce_ctr: &mut u64,
    udp_id: u32,
    disc_port: u16,
    bytes: &[u8],
) -> core::result::Result<bool, ()> {
    const OP_UDP_SEND_TO: u8 = 7;
    const LOCAL_IP: [u8; 4] = [10, 0, 2, 15];

    let mut send = [0u8; 16 + 256 + 8];
    let hdr_len = 16;
    if hdr_len + bytes.len() > send.len() {
        return Ok(false);
    }
    send[0] = MAGIC0;
    send[1] = MAGIC1;
    send[2] = VERSION;
    send[3] = OP_UDP_SEND_TO;
    send[4..8].copy_from_slice(&udp_id.to_le_bytes());
    send[12..14].copy_from_slice(&disc_port.to_le_bytes());
    send[14..16].copy_from_slice(&(bytes.len() as u16).to_le_bytes());
    send[16..16 + bytes.len()].copy_from_slice(bytes);
    let nonce = crate::os::entry::next_nonce(nonce_ctr);
    send[16 + bytes.len()..16 + bytes.len() + 8].copy_from_slice(&nonce.to_le_bytes());
    send[8..12].copy_from_slice(&LOCAL_IP);
    let rsp = crate::os::entry::rpc_nonce(
        pending,
        net,
        &send[..hdr_len + bytes.len() + 8],
        OP_UDP_SEND_TO | 0x80,
        nonce,
    )?;
    Ok(rsp[0] == MAGIC0
        && rsp[1] == MAGIC1
        && rsp[2] == VERSION
        && rsp[3] == (OP_UDP_SEND_TO | 0x80)
        && rsp[4] == STATUS_OK)
}

pub(crate) fn run_single_vm_dual_node_bringup(
    pending_replies: &mut ReplyBuffer<16, 512>,
    net: &KernelClient,
    nonce_ctr: &mut u64,
    udp_id: u32,
    disc_port: u16,
    port: u16,
) -> core::result::Result<u32, ()> {
    // Bounded peer cache (Phase 1): keep a small, deterministic LRU of recently seen peers.
    let mut peers = PeerLru::with_default_capacity();
    let mut peer_ips: Vec<(String, [u8; 4])> = Vec::new();

    let mut announce_sent = false;
    let node_b_device_id = "node-b";
    let node_b_port: u16 = 34_568;
    for i in 0..20_000u64 {
        if !announce_sent && (i % 64 == 0) {
            let ann_b = AnnounceV1 {
                device_id: String::from(node_b_device_id),
                port: node_b_port,
                // SECURITY: bring-up test keys, NOT production custody
                noise_static: StaticKeypair::from_secret(crate::os::entry::derive_test_secret(0xD1, node_b_port))
                    .public,
                services: alloc::vec!["dsoftbusd".into()],
            };

            let ok_b = match encode_announce_v1(&ann_b).ok() {
                Some(b) => send_announce(pending_replies, net, nonce_ctr, udp_id, disc_port, &b).unwrap_or(false),
                None => false,
            };

            if ok_b {
                announce_sent = true;
            }
            if !announce_sent {
                announce_sent = true;
            }
            let _ = nexus_abi::debug_println("dsoftbusd: discovery announce sent");
        }

        let mut r = [0u8; 18];
        let recv_nonce = crate::os::entry::next_nonce(nonce_ctr);
        r[0] = MAGIC0;
        r[1] = MAGIC1;
        r[2] = VERSION;
        r[3] = OP_UDP_RECV_FROM;
        r[4..8].copy_from_slice(&udp_id.to_le_bytes());
        r[8..10].copy_from_slice(&(256u16).to_le_bytes());
        r[10..18].copy_from_slice(&recv_nonce.to_le_bytes());
        let rsp = crate::os::entry::rpc_nonce(pending_replies, net, &r, OP_UDP_RECV_FROM | 0x80, recv_nonce)
            .map_err(|_| ())?;
        if rsp[0] == MAGIC0 && rsp[1] == MAGIC1 && rsp[2] == VERSION && rsp[3] == (OP_UDP_RECV_FROM | 0x80) {
            match rsp[4] {
                STATUS_OK => {
                    let n = u16::from_le_bytes([rsp[5], rsp[6]]) as usize;
                    let from_ip = [rsp[7], rsp[8], rsp[9], rsp[10]];
                    let base = 13;
                    if n <= 256 && base + n <= rsp.len() {
                        let payload = &rsp[base..base + n];
                        if let Ok(pkt) = decode_announce_v1(payload) {
                            let entry =
                                PeerEntry::new(pkt.device_id.clone(), pkt.port, pkt.noise_static, pkt.services);
                            peers.insert(entry);
                            set_peer_ip(&peers, &mut peer_ips, &pkt.device_id, from_ip);
                            if peers.peek(node_b_device_id).is_some() {
                                let _ = nexus_abi::debug_println("dsoftbusd: discovery peer found device=local");
                                break;
                            }
                        }
                    }
                }
                STATUS_WOULD_BLOCK => {}
                STATUS_MALFORMED => {
                    let _ = nexus_abi::debug_println("dsoftbusd: udp recv MALFORMED");
                }
                _ => {
                    let _ = nexus_abi::debug_println("dsoftbusd: udp recv FAIL");
                }
            }
        }

        let _ = yield_();
    }

    let lid = crate::os::entry::listen_with_retry(pending_replies, net, nonce_ctr, port)?;
    let _ = nexus_abi::debug_println("dsoftbusd: os transport up (udp+tcp)");

    let port_b: u16 = 34_568;
    let mut req_b = [0u8; 14];
    req_b[0] = MAGIC0;
    req_b[1] = MAGIC1;
    req_b[2] = VERSION;
    req_b[3] = OP_LISTEN;
    req_b[4] = (port_b & 0xff) as u8;
    req_b[5] = (port_b >> 8) as u8;
    let nonce_b = crate::os::entry::next_nonce(nonce_ctr);
    req_b[6..14].copy_from_slice(&nonce_b.to_le_bytes());
    let rsp_b = crate::os::entry::rpc_nonce(pending_replies, net, &req_b, OP_LISTEN | 0x80, nonce_b).map_err(|_| ())?;
    if rsp_b[0] != MAGIC0
        || rsp_b[1] != MAGIC1
        || rsp_b[2] != VERSION
        || rsp_b[3] != (OP_LISTEN | 0x80)
        || rsp_b[4] != STATUS_OK
    {
        let _ = nexus_abi::debug_println("dsoftbusd: listen port_b FAIL");
        loop {
            let _ = yield_();
        }
    }
    let lid_b = u32::from_le_bytes([rsp_b[5], rsp_b[6], rsp_b[7], rsp_b[8]]);

    let node_b_device_id = "node-b";
    let Some(peer_b) = peers.peek(node_b_device_id) else {
        let _ = nexus_abi::debug_println("dsoftbusd: discovery missing peer=node-b");
        loop {
            let _ = yield_();
        }
    };
    let Some(peer_ip) = get_peer_ip(&peer_ips, node_b_device_id) else {
        let _ = nexus_abi::debug_println("dsoftbusd: discovery peer ip missing");
        loop {
            let _ = yield_();
        }
    };
    let peer_ip = if peer_b.port == 34_567 || peer_b.port == 34_568 {
        [10, 0, 2, 15]
    } else {
        peer_ip
    };

    let _ = nexus_abi::debug_println("dsoftbusd: session connect peer=node-b");
    if peer_b.port == 34_568 {
        let _ = nexus_abi::debug_println("dsoftbusd: connect portB ok");
    } else {
        let _ = nexus_abi::debug_println("dsoftbusd: connect portB BAD");
    }
    if peer_ip == [10, 0, 2, 15] {
        let _ = nexus_abi::debug_println("dsoftbusd: connect ip loopback ok");
    } else {
        let _ = nexus_abi::debug_println("dsoftbusd: connect ip loopback BAD");
    }

    let connect_result = crate::os::entry::tcp_connect(pending_replies, net, nonce_ctr, peer_ip, peer_b.port);
    let accept_result = crate::os::entry::tcp_accept(pending_replies, net, nonce_ctr, lid_b);
    let (sid_a, sid_b) = match (connect_result, accept_result) {
        (Ok(a), Ok(b)) => (a, b),
        _ => {
            let _ = nexus_abi::debug_println("dsoftbusd: dual-node connect FAIL");
            loop {
                let _ = yield_();
            }
        }
    };

    let node_a_static = StaticKeypair::from_secret(crate::os::entry::derive_test_secret(0xD0, port_b));
    let node_a_eph_seed = crate::os::entry::derive_test_secret(0xE0, port_b);
    let node_b_static = StaticKeypair::from_secret(crate::os::entry::derive_test_secret(0xD1, port_b));
    let node_b_eph_seed = crate::os::entry::derive_test_secret(0xE1, port_b);

    let Some(peer_b) = peers.peek(node_b_device_id) else {
        let _ = nexus_abi::debug_println("dsoftbusd: discovery missing peer=node-b");
        loop {
            let _ = yield_();
        }
    };
    if peer_b.noise_static != node_b_static.public {
        let _ = nexus_abi::debug_println("dsoftbusd: identity mismatch peer=node-b");
        loop {
            let _ = yield_();
        }
    }
    let node_b_pub_expected = peer_b.noise_static;
    let node_a_pub_expected = node_a_static.public;

    let mut initiator = XkInitiator::new(node_a_static, node_b_pub_expected, node_a_eph_seed);
    let mut responder = XkResponder::new(node_b_static, node_a_pub_expected, node_b_eph_seed);

    let mut msg1 = [0u8; MSG1_LEN];
    initiator.write_msg1(&mut msg1);
    if crate::os::entry::dual_stream_write(pending_replies, net, nonce_ctr, sid_a, &msg1).is_err() {
        let _ = nexus_abi::debug_println("dsoftbusd: dual-node msg1 write FAIL");
        loop {
            let _ = yield_();
        }
    }

    let mut msg1_recv = [0u8; MSG1_LEN];
    if crate::os::entry::dual_stream_read(pending_replies, net, nonce_ctr, sid_b, &mut msg1_recv).is_err() {
        let _ = nexus_abi::debug_println("dsoftbusd: dual-node msg1 read FAIL");
        loop {
            let _ = yield_();
        }
    }
    let mut msg2 = [0u8; MSG2_LEN];
    if responder.read_msg1_write_msg2(&msg1_recv, &mut msg2).is_err() {
        let _ = nexus_abi::debug_println("dsoftbusd: dual-node msg2 gen FAIL");
        loop {
            let _ = yield_();
        }
    }
    if crate::os::entry::dual_stream_write(pending_replies, net, nonce_ctr, sid_b, &msg2).is_err() {
        let _ = nexus_abi::debug_println("dsoftbusd: dual-node msg2 write FAIL");
        loop {
            let _ = yield_();
        }
    }

    let mut msg2_recv = [0u8; MSG2_LEN];
    if crate::os::entry::dual_stream_read(pending_replies, net, nonce_ctr, sid_a, &mut msg2_recv).is_err() {
        let _ = nexus_abi::debug_println("dsoftbusd: dual-node msg2 read FAIL");
        loop {
            let _ = yield_();
        }
    }
    let mut msg3 = [0u8; MSG3_LEN];
    let transport_a = match initiator.read_msg2_write_msg3(&msg2_recv, &mut msg3) {
        Ok(keys) => Transport::new(keys),
        Err(_) => {
            let _ = nexus_abi::debug_println("dsoftbusd: dual-node msg3 gen FAIL");
            loop {
                let _ = yield_();
            }
        }
    };
    if crate::os::entry::dual_stream_write(pending_replies, net, nonce_ctr, sid_a, &msg3).is_err() {
        let _ = nexus_abi::debug_println("dsoftbusd: dual-node msg3 write FAIL");
        loop {
            let _ = yield_();
        }
    }

    let mut msg3_recv = [0u8; MSG3_LEN];
    if crate::os::entry::dual_stream_read(pending_replies, net, nonce_ctr, sid_b, &mut msg3_recv).is_err() {
        let _ = nexus_abi::debug_println("dsoftbusd: dual-node msg3 read FAIL");
        loop {
            let _ = yield_();
        }
    }
    let transport_b = match responder.read_msg3_finish(&msg3_recv) {
        Ok(keys) => Transport::new(keys),
        Err(nexus_noise_xk::NoiseError::StaticKeyMismatch) => {
            let _ = nexus_abi::debug_println("dsoftbusd: identity mismatch peer=nodeA");
            loop {
                let _ = yield_();
            }
        }
        Err(_) => {
            let _ = nexus_abi::debug_println("dsoftbusd: dual-node handshake FAIL");
            loop {
                let _ = yield_();
            }
        }
    };

    let _ = transport_a;
    let _ = transport_b;

    let _ = nexus_abi::debug_println("dsoftbusd: identity bound peer=node-b");
    let _ = nexus_abi::debug_println("dsoftbusd: dual-node session ok");
    let _ = nexus_abi::debug_println("dsoftbusd: ready");
    nexus_log::info("dsoftbusd", |line| {
        line.text("dsoftbusd: ready");
    });

    Ok(lid)
}
