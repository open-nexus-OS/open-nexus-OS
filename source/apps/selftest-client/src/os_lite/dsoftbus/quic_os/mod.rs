// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: DSoftBus OS QUIC-v2 transport probe used by the selftest-client to
//! drive the deterministic Noise XK + QUIC datagram session against
//! `dsoftbusd`. Extracted verbatim from the previous monolithic `os_lite`
//! block in `main.rs` (TASK-0023B / RFC-0038 phase 1, cut 1). No behavior,
//! marker, or reject-path change.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal (binary crate)
//! TEST_COVERAGE: QEMU marker ladder via `just test-os` (REQUIRE_DSOFTBUS=1).
//!   Required markers (emitted by callers / dsoftbusd, not this probe):
//!     - dsoftbusd: transport selected quic
//!     - dsoftbusd: auth ok
//!     - dsoftbusd: os session ok
//!     - SELFTEST: quic session ok
//! ADR: docs/adr/0017-service-architecture.md, docs/rfcs/RFC-0038-*.md

use nexus_abi::yield_;
use nexus_ipc::KernelClient;

use super::super::ipc::clients::{cached_netstackd_client, cached_reply_client};

pub(crate) fn dsoftbus_os_transport_probe() -> core::result::Result<(), ()> {
    const MAGIC0: u8 = b'N';
    const MAGIC1: u8 = b'S';
    const VERSION: u8 = 1;
    const OP_UDP_BIND: u8 = 6;
    const OP_UDP_SEND_TO: u8 = 7;
    const OP_UDP_RECV_FROM: u8 = 8;
    const STATUS_OK: u8 = 0;
    const STATUS_WOULD_BLOCK: u8 = 3;
    const QUIC_FRAME_MAGIC0: u8 = b'Q';
    const QUIC_FRAME_MAGIC1: u8 = b'D';
    const QUIC_FRAME_VERSION: u8 = 1;
    const QUIC_FRAME_HEADER_LEN: usize = 10;
    const QUIC_OP_MSG1: u8 = 1;
    const QUIC_OP_MSG2: u8 = 2;
    const QUIC_OP_MSG3: u8 = 3;
    const QUIC_OP_PING: u8 = 4;
    const QUIC_OP_PONG: u8 = 5;
    const SESSION_NONCE: u32 = 0x5155_4943;

    fn rpc(client: &KernelClient, req: &[u8]) -> core::result::Result<[u8; 512], ()> {
        let reply = cached_reply_client().map_err(|_| ())?;
        let (reply_send_slot, reply_recv_slot) = reply.slots();
        let reply_send_clone = nexus_abi::cap_clone(reply_send_slot).map_err(|_| ())?;
        client.send_with_cap_move(req, reply_send_clone).map_err(|_| ())?;
        let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 512];
        for _ in 0..5_000 {
            match nexus_abi::ipc_recv_v1(
                reply_recv_slot,
                &mut hdr,
                &mut buf,
                nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
                0,
            ) {
                Ok(_n) => return Ok(buf),
                Err(nexus_abi::IpcError::QueueEmpty) => {
                    let _ = yield_();
                }
                Err(_) => return Err(()),
            }
        }
        Err(())
    }

    fn encode_quic_frame(
        op: u8,
        session_nonce: u32,
        payload: &[u8],
        out: &mut [u8; 256],
    ) -> Option<usize> {
        if payload.len() > out.len().saturating_sub(QUIC_FRAME_HEADER_LEN) {
            return None;
        }
        out[0] = QUIC_FRAME_MAGIC0;
        out[1] = QUIC_FRAME_MAGIC1;
        out[2] = QUIC_FRAME_VERSION;
        out[3] = op;
        out[4..8].copy_from_slice(&session_nonce.to_le_bytes());
        out[8..10].copy_from_slice(&(payload.len() as u16).to_le_bytes());
        out[10..10 + payload.len()].copy_from_slice(payload);
        Some(QUIC_FRAME_HEADER_LEN + payload.len())
    }

    fn decode_quic_frame(buf: &[u8], n: usize) -> Option<(u8, u32, &[u8])> {
        if n < QUIC_FRAME_HEADER_LEN {
            return None;
        }
        if buf[0] != QUIC_FRAME_MAGIC0
            || buf[1] != QUIC_FRAME_MAGIC1
            || buf[2] != QUIC_FRAME_VERSION
        {
            return None;
        }
        let op = buf[3];
        let session_nonce = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
        let payload_len = u16::from_le_bytes([buf[8], buf[9]]) as usize;
        let payload_end = QUIC_FRAME_HEADER_LEN.checked_add(payload_len)?;
        if payload_end > n || payload_end > buf.len() {
            return None;
        }
        Some((op, session_nonce, &buf[QUIC_FRAME_HEADER_LEN..payload_end]))
    }

    let net = cached_netstackd_client().map_err(|_| ())?;

    // QUIC-v2 over UDP datagrams against dsoftbusd session endpoint.
    let port: u16 = 34_567;
    let server_ip = [10, 0, 2, 15];

    let mut bind_req = [0u8; 10];
    bind_req[0] = MAGIC0;
    bind_req[1] = MAGIC1;
    bind_req[2] = VERSION;
    bind_req[3] = OP_UDP_BIND;
    bind_req[4..8].copy_from_slice(&[0, 0, 0, 0]);
    bind_req[8..10].copy_from_slice(&34_569u16.to_le_bytes());
    let bind_rsp = rpc(&net, &bind_req)?;
    if bind_rsp[0] != MAGIC0
        || bind_rsp[1] != MAGIC1
        || bind_rsp[2] != VERSION
        || bind_rsp[3] != (OP_UDP_BIND | 0x80)
        || bind_rsp[4] != STATUS_OK
    {
        return Err(());
    }
    let udp_id = u32::from_le_bytes([bind_rsp[5], bind_rsp[6], bind_rsp[7], bind_rsp[8]]);

    // ============================================================
    // REAL Noise XK Handshake (RFC-0008) - Initiator side
    // ============================================================
    use nexus_noise_xk::{StaticKeypair, Transport, XkInitiator, MSG1_LEN, MSG2_LEN, MSG3_LEN};

    // SECURITY: bring-up test keys, NOT production custody
    // These keys are deterministic and derived from port for reproducibility only.
    // Phase 2 integrates with keystored for real key provisioning.
    fn derive_test_secret(tag: u8, port: u16) -> [u8; 32] {
        let mut seed = [0u8; 32];
        seed[0] = tag;
        seed[1] = (port >> 8) as u8;
        seed[2] = (port & 0xff) as u8;
        // Fill rest with deterministic pattern
        for i in 3..32 {
            seed[i] = ((tag as u16).wrapping_mul(port).wrapping_add(i as u16) & 0xff) as u8;
        }
        seed
    }

    // Client (initiator) static keypair - derived from port with tag 0xB0
    // SECURITY: bring-up test keys, NOT production custody
    let client_static = StaticKeypair::from_secret(derive_test_secret(0xB0, port));
    // Client ephemeral seed - derived from port with tag 0xD0
    // SECURITY: bring-up test keys, NOT production custody
    let client_eph_seed = derive_test_secret(0xD0, port);
    // Expected server static public key (server uses tag 0xA0)
    // SECURITY: bring-up test keys, NOT production custody
    let server_static_expected =
        StaticKeypair::from_secret(derive_test_secret(0xA0, port)).public;

    let mut initiator =
        XkInitiator::new(client_static, server_static_expected, client_eph_seed);

    fn udp_send_frame(
        net: &KernelClient,
        udp_id: u32,
        dst_ip: [u8; 4],
        dst_port: u16,
        payload: &[u8],
    ) -> core::result::Result<(), ()> {
        let mut req = [0u8; 16 + 256];
        if payload.len() > 256 {
            return Err(());
        }
        req[0] = MAGIC0;
        req[1] = MAGIC1;
        req[2] = VERSION;
        req[3] = OP_UDP_SEND_TO;
        req[4..8].copy_from_slice(&udp_id.to_le_bytes());
        req[8..12].copy_from_slice(&dst_ip);
        req[12..14].copy_from_slice(&dst_port.to_le_bytes());
        req[14..16].copy_from_slice(&(payload.len() as u16).to_le_bytes());
        req[16..16 + payload.len()].copy_from_slice(payload);
        let rsp = rpc(net, &req[..16 + payload.len()])?;
        if rsp[0] == MAGIC0
            && rsp[1] == MAGIC1
            && rsp[2] == VERSION
            && rsp[3] == (OP_UDP_SEND_TO | 0x80)
            && rsp[4] == STATUS_OK
        {
            Ok(())
        } else {
            Err(())
        }
    }

    fn udp_recv_frame(
        net: &KernelClient,
        udp_id: u32,
        out: &mut [u8],
    ) -> core::result::Result<Option<([u8; 4], u16, usize)>, ()> {
        let mut req = [0u8; 10];
        req[0] = MAGIC0;
        req[1] = MAGIC1;
        req[2] = VERSION;
        req[3] = OP_UDP_RECV_FROM;
        req[4..8].copy_from_slice(&udp_id.to_le_bytes());
        req[8..10].copy_from_slice(&((out.len().min(460)) as u16).to_le_bytes());
        let rsp = rpc(net, &req)?;
        if rsp[0] != MAGIC0
            || rsp[1] != MAGIC1
            || rsp[2] != VERSION
            || rsp[3] != (OP_UDP_RECV_FROM | 0x80)
        {
            return Err(());
        }
        if rsp[4] == STATUS_WOULD_BLOCK {
            return Ok(None);
        }
        if rsp[4] != STATUS_OK {
            return Err(());
        }
        let n = u16::from_le_bytes([rsp[5], rsp[6]]) as usize;
        let from_ip = [rsp[7], rsp[8], rsp[9], rsp[10]];
        let from_port = u16::from_le_bytes([rsp[11], rsp[12]]);
        let base = 13usize;
        if n > out.len() || base + n > rsp.len() {
            return Err(());
        }
        out[..n].copy_from_slice(&rsp[base..base + n]);
        Ok(Some((from_ip, from_port, n)))
    }

    // Step 1: Write msg1 (initiator ephemeral public key, 32 bytes)
    let mut msg1 = [0u8; MSG1_LEN];
    initiator.write_msg1(&mut msg1);
    let mut frame = [0u8; 256];
    let msg1_len =
        encode_quic_frame(QUIC_OP_MSG1, SESSION_NONCE, &msg1, &mut frame).ok_or(())?;
    udp_send_frame(&net, udp_id, server_ip, port, &frame[..msg1_len])?;

    // Step 2: Read msg2 (responder ephemeral + encrypted static + tag, 96 bytes)
    let mut msg2 = [0u8; MSG2_LEN];
    let mut inbound = [0u8; 256];
    let mut got_msg2 = false;
    for _ in 0..100_000 {
        match udp_recv_frame(&net, udp_id, &mut inbound)? {
            Some((from_ip, from_port, n)) => {
                if from_ip != server_ip || from_port != port {
                    continue;
                }
                let Some((op, nonce, payload)) = decode_quic_frame(&inbound, n) else {
                    continue;
                };
                if op == QUIC_OP_MSG2 && nonce == SESSION_NONCE && payload.len() == MSG2_LEN {
                    msg2.copy_from_slice(payload);
                    got_msg2 = true;
                    break;
                }
            }
            None => {
                let _ = yield_();
            }
        }
    }
    if !got_msg2 {
        return Err(());
    }

    // Step 3: Write msg3 and get transport keys (encrypted initiator static + tag, 64 bytes)
    let mut msg3 = [0u8; MSG3_LEN];
    let transport_keys = initiator.read_msg2_write_msg3(&msg2, &mut msg3).map_err(|_| ())?;
    let msg3_len =
        encode_quic_frame(QUIC_OP_MSG3, SESSION_NONCE, &msg3, &mut frame).ok_or(())?;
    udp_send_frame(&net, udp_id, server_ip, port, &frame[..msg3_len])?;

    // Create transport for encrypted communication
    let mut _transport = Transport::new(transport_keys);

    // Handshake complete - server will emit "dsoftbusd: auth ok" after processing msg3

    // WRITE "PING" datagram frame.
    let ping_len =
        encode_quic_frame(QUIC_OP_PING, SESSION_NONCE, b"PING", &mut frame).ok_or(())?;
    udp_send_frame(&net, udp_id, server_ip, port, &frame[..ping_len])?;

    // READ "PONG" datagram frame.
    for _ in 0..100_000 {
        match udp_recv_frame(&net, udp_id, &mut inbound)? {
            Some((from_ip, from_port, n)) => {
                if from_ip != server_ip || from_port != port {
                    continue;
                }
                let Some((op, nonce, payload)) = decode_quic_frame(&inbound, n) else {
                    continue;
                };
                if op == QUIC_OP_PONG && nonce == SESSION_NONCE && payload == b"PONG" {
                    return Ok(());
                }
            }
            None => {}
        }
        let _ = yield_();
    }
    Err(())
}
