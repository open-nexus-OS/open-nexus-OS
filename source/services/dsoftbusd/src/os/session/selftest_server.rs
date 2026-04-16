//! Selftest server loop runner for single-VM os_entry path.

use nexus_abi::yield_;
use nexus_ipc::reqrep::ReplyBuffer;
use nexus_ipc::KernelClient;
use nexus_noise_xk::{StaticKeypair, Transport, XkResponder, MSG1_LEN, MSG2_LEN, MSG3_LEN};
use super::quic_frame::{
    decode_quic_frame, encode_quic_frame, QUIC_OP_MSG1, QUIC_OP_MSG2, QUIC_OP_MSG3, QUIC_OP_PING,
    QUIC_OP_PONG,
};

fn run_mux_contract_selftest() -> bool {
    use crate::os::mux_v2::{
        MuxHostEndpoint, MuxSessionState, PriorityClass, SendBudgetOutcome, StreamId, StreamName,
        WindowCredit, DEFAULT_INITIAL_STREAM_CREDIT, MAX_FRAME_PAYLOAD_BYTES,
    };

    let control_id = match StreamId::new(1) {
        Some(v) => v,
        None => return false,
    };
    let bulk_id = match StreamId::new(2) {
        Some(v) => v,
        None => return false,
    };
    let control_pri = match PriorityClass::new(PriorityClass::HIGHEST) {
        Some(v) => v,
        None => return false,
    };
    let bulk_pri = match PriorityClass::new(4) {
        Some(v) => v,
        None => return false,
    };
    let control_name = match StreamName::new("selftest/control") {
        Ok(v) => v,
        Err(_) => return false,
    };
    let bulk_name = match StreamName::new("selftest/bulk") {
        Ok(v) => v,
        Err(_) => return false,
    };
    let credit = WindowCredit::new(DEFAULT_INITIAL_STREAM_CREDIT);

    // Endpoint-level proof: OPEN/OPEN_ACK + DATA propagation with typed names.
    let mut client = MuxHostEndpoint::new_authenticated(0);
    let mut server = MuxHostEndpoint::new_authenticated(0);
    if client.open_stream(control_id, control_pri, control_name.clone(), credit).is_err() {
        return false;
    }
    if client.open_stream(bulk_id, bulk_pri, bulk_name.clone(), credit).is_err() {
        return false;
    }
    let client_open_events = client.drain_outbound();
    for event in client_open_events {
        if server.ingest(event).is_err() {
            return false;
        }
    }
    let mut saw_control_accept = false;
    let mut saw_bulk_accept = false;
    while let Some(accepted) = server.accept_stream() {
        if accepted.stream_id == control_id && accepted.name.as_str() == "selftest/control" {
            saw_control_accept = true;
        }
        if accepted.stream_id == bulk_id && accepted.name.as_str() == "selftest/bulk" {
            saw_bulk_accept = true;
        }
    }
    if !(saw_control_accept && saw_bulk_accept) {
        return false;
    }
    let server_open_ack_events = server.drain_outbound();
    for event in server_open_ack_events {
        if client.ingest(event).is_err() {
            return false;
        }
    }

    let control_sent =
        matches!(client.send_data(control_id, control_pri, 16), Ok(SendBudgetOutcome::Sent { .. }));
    let bulk_sent =
        matches!(client.send_data(bulk_id, bulk_pri, 128), Ok(SendBudgetOutcome::Sent { .. }));
    if !(control_sent && bulk_sent) {
        return false;
    }
    let client_data_events = client.drain_outbound();
    for event in client_data_events {
        if server.ingest(event).is_err() {
            return false;
        }
    }
    let control_buffered = server.buffered_bytes(control_id).unwrap_or(0) >= 16;
    let bulk_buffered = server.buffered_bytes(bulk_id).unwrap_or(0) >= 128;
    if !(control_buffered && bulk_buffered) {
        return false;
    }

    // Session-level proof: scheduler prioritizes control, and credit exhaustion backpressures bulk.
    let mut priority_session = MuxSessionState::new_authenticated(0);
    if priority_session
        .open_stream(control_id, control_pri, WindowCredit::new(DEFAULT_INITIAL_STREAM_CREDIT))
        .is_err()
    {
        return false;
    }
    if priority_session
        .open_stream(bulk_id, bulk_pri, WindowCredit::new(DEFAULT_INITIAL_STREAM_CREDIT))
        .is_err()
    {
        return false;
    }
    if priority_session.send_data(bulk_id, 8).is_err() {
        return false;
    }
    if priority_session.send_data(control_id, 8).is_err() {
        return false;
    }
    let control_priority_wins =
        matches!(priority_session.dequeue_next_stream(), Some(id) if id == control_id);
    if !control_priority_wins {
        return false;
    }

    let mut backpressure_session = MuxSessionState::new_authenticated(0);
    if backpressure_session
        .open_stream(bulk_id, bulk_pri, WindowCredit::new(DEFAULT_INITIAL_STREAM_CREDIT))
        .is_err()
    {
        return false;
    }
    let first = backpressure_session.send_data(bulk_id, MAX_FRAME_PAYLOAD_BYTES);
    let second = backpressure_session.send_data(bulk_id, MAX_FRAME_PAYLOAD_BYTES);
    let third = backpressure_session.send_data(bulk_id, 1);
    let backpressure_ok = matches!(first, Ok(SendBudgetOutcome::Sent { .. }))
        && matches!(second, Ok(SendBudgetOutcome::Sent { .. }))
        && matches!(third, Ok(SendBudgetOutcome::WouldBlock { .. }));

    backpressure_ok
}

fn run_quic_udp_selftest_server_loop(
    pending_replies: &mut ReplyBuffer<16, 512>,
    net: &KernelClient,
    nonce_ctr: &mut u64,
    port: u16,
) -> ! {
    let udp_id = match crate::os::entry::udp_bind(pending_replies, net, nonce_ctr, [0, 0, 0, 0], port) {
        Ok(id) => id,
        Err(()) => {
            let _ = nexus_abi::debug_println("dsoftbusd: quic udp bind FAIL");
            loop {
                let _ = yield_();
            }
        }
    };

    let server_static = StaticKeypair::from_secret(crate::os::entry::derive_test_secret(0xA0, port));
    let server_eph_seed = crate::os::entry::derive_test_secret(0xC0, port);
    let client_static_expected =
        StaticKeypair::from_secret(crate::os::entry::derive_test_secret(0xB0, port)).public;
    let mut responder = XkResponder::new(server_static, client_static_expected, server_eph_seed);

    let mut in_frame = [0u8; 256];
    let mut out_frame = [0u8; 256];
    let mut peer: Option<([u8; 4], u16)> = None;
    let mut session_nonce: u32 = 0;

    let mut msg1 = [0u8; MSG1_LEN];
    let mut got_msg1 = false;
    for _ in 0..50_000 {
        match crate::os::entry::udp_recv_from(pending_replies, net, nonce_ctr, udp_id, &mut in_frame) {
            Ok(Some((from_ip, from_port, n))) => {
                let Some((op, nonce, payload)) = decode_quic_frame(&in_frame, n) else {
                    continue;
                };
                if op != QUIC_OP_MSG1 || payload.len() != MSG1_LEN {
                    continue;
                }
                msg1.copy_from_slice(payload);
                peer = Some((from_ip, from_port));
                session_nonce = nonce;
                got_msg1 = true;
                break;
            }
            Ok(None) => {}
            Err(()) => {}
        }
        let _ = yield_();
    }
    if !got_msg1 {
        let _ = nexus_abi::debug_println("dsoftbusd: quic msg1 timeout");
        loop {
            let _ = yield_();
        }
    }

    let Some((peer_ip, peer_port)) = peer else {
        loop {
            let _ = yield_();
        }
    };
    let mut msg2 = [0u8; MSG2_LEN];
    if responder.read_msg1_write_msg2(&msg1, &mut msg2).is_err() {
        let _ = nexus_abi::debug_println("dsoftbusd: quic msg2 gen FAIL");
        loop {
            let _ = yield_();
        }
    }
    let Some(msg2_len) = encode_quic_frame(QUIC_OP_MSG2, session_nonce, &msg2, &mut out_frame) else {
        let _ = nexus_abi::debug_println("dsoftbusd: quic msg2 frame FAIL");
        loop {
            let _ = yield_();
        }
    };
    if crate::os::entry::udp_send_to(
        pending_replies,
        net,
        nonce_ctr,
        udp_id,
        peer_ip,
        peer_port,
        &out_frame[..msg2_len],
    )
    .is_err()
    {
        let _ = nexus_abi::debug_println("dsoftbusd: quic msg2 send FAIL");
        loop {
            let _ = yield_();
        }
    }

    let mut msg3 = [0u8; MSG3_LEN];
    let mut got_msg3 = false;
    for _ in 0..50_000 {
        match crate::os::entry::udp_recv_from(pending_replies, net, nonce_ctr, udp_id, &mut in_frame) {
            Ok(Some((from_ip, from_port, n))) => {
                if from_ip != peer_ip || from_port != peer_port {
                    continue;
                }
                let Some((op, nonce, payload)) = decode_quic_frame(&in_frame, n) else {
                    continue;
                };
                if nonce != session_nonce || op != QUIC_OP_MSG3 || payload.len() != MSG3_LEN {
                    continue;
                }
                msg3.copy_from_slice(payload);
                got_msg3 = true;
                break;
            }
            Ok(None) => {}
            Err(()) => {}
        }
        let _ = yield_();
    }
    if !got_msg3 {
        let _ = nexus_abi::debug_println("dsoftbusd: quic msg3 timeout");
        loop {
            let _ = yield_();
        }
    }

    let transport_keys = match responder.read_msg3_finish(&msg3) {
        Ok(keys) => keys,
        Err(nexus_noise_xk::NoiseError::StaticKeyMismatch) => {
            let _ = nexus_abi::debug_println("dsoftbusd: noise static key mismatch");
            loop {
                let _ = yield_();
            }
        }
        Err(_) => {
            let _ = nexus_abi::debug_println("dsoftbusd: quic msg3 FAIL");
            loop {
                let _ = yield_();
            }
        }
    };
    let mut _transport = Transport::new(transport_keys);
    let _ = nexus_abi::debug_println("dsoftbusd: auth ok");

    let mut got_ping = false;
    for _ in 0..50_000 {
        match crate::os::entry::udp_recv_from(pending_replies, net, nonce_ctr, udp_id, &mut in_frame) {
            Ok(Some((from_ip, from_port, n))) => {
                if from_ip != peer_ip || from_port != peer_port {
                    continue;
                }
                let Some((op, nonce, payload)) = decode_quic_frame(&in_frame, n) else {
                    continue;
                };
                if nonce != session_nonce || op != QUIC_OP_PING || payload != b"PING" {
                    continue;
                }
                got_ping = true;
                break;
            }
            Ok(None) => {}
            Err(()) => {}
        }
        let _ = yield_();
    }
    if !got_ping {
        let _ = nexus_abi::debug_println("dsoftbusd: quic ping timeout");
        loop {
            let _ = yield_();
        }
    }

    let Some(pong_len) = encode_quic_frame(QUIC_OP_PONG, session_nonce, b"PONG", &mut out_frame) else {
        let _ = nexus_abi::debug_println("dsoftbusd: quic pong frame FAIL");
        loop {
            let _ = yield_();
        }
    };
    if crate::os::entry::udp_send_to(
        pending_replies,
        net,
        nonce_ctr,
        udp_id,
        peer_ip,
        peer_port,
        &out_frame[..pong_len],
    )
    .is_err()
    {
        let _ = nexus_abi::debug_println("dsoftbusd: quic pong send FAIL");
        loop {
            let _ = yield_();
        }
    }

    let _ = nexus_abi::debug_println("dsoftbusd: os session ok");
    let _ = nexus_abi::debug_println("SELFTEST: quic session ok");

    if run_mux_contract_selftest() {
        let _ = nexus_abi::debug_println("dsoftbus:mux session up");
        let _ = nexus_abi::debug_println("dsoftbus:mux data ok");
        let _ = nexus_abi::debug_println("SELFTEST: mux pri control ok");
        let _ = nexus_abi::debug_println("SELFTEST: mux bulk ok");
        let _ = nexus_abi::debug_println("SELFTEST: mux backpressure ok");
    } else {
        let _ = nexus_abi::debug_println("dsoftbus:mux selftest fail");
    }

    loop {
        let _ = yield_();
    }
}

pub(crate) fn run_selftest_server_loop(
    pending_replies: &mut ReplyBuffer<16, 512>,
    net: &KernelClient,
    nonce_ctr: &mut u64,
    lid: u32,
    port: u16,
    transport_selection: crate::os::entry::OsTransportSelection,
) -> ! {
    match transport_selection {
        crate::os::entry::OsTransportSelection::QuicUdp => {
            let _ = lid;
            run_quic_udp_selftest_server_loop(pending_replies, net, nonce_ctr, port)
        }
    }
}
