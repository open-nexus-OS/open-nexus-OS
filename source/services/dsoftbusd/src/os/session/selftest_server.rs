//! Selftest server loop runner for single-VM os_entry path.

use nexus_abi::yield_;
use nexus_ipc::reqrep::ReplyBuffer;
use nexus_ipc::KernelClient;
use nexus_noise_xk::{StaticKeypair, Transport, XkResponder, MSG1_LEN, MSG2_LEN, MSG3_LEN};

const MAGIC0: u8 = b'N';
const MAGIC1: u8 = b'S';
const VERSION: u8 = 1;
const OP_ACCEPT: u8 = 2;
const OP_READ: u8 = 4;
const OP_WRITE: u8 = 5;
const STATUS_OK: u8 = 0;
const STATUS_WOULD_BLOCK: u8 = 3;

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
    if client
        .open_stream(control_id, control_pri, control_name.clone(), credit)
        .is_err()
    {
        return false;
    }
    if client
        .open_stream(bulk_id, bulk_pri, bulk_name.clone(), credit)
        .is_err()
    {
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

    let control_sent = matches!(
        client.send_data(control_id, control_pri, 16),
        Ok(SendBudgetOutcome::Sent { .. })
    );
    let bulk_sent = matches!(
        client.send_data(bulk_id, bulk_pri, 128),
        Ok(SendBudgetOutcome::Sent { .. })
    );
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
        .open_stream(
            control_id,
            control_pri,
            WindowCredit::new(DEFAULT_INITIAL_STREAM_CREDIT),
        )
        .is_err()
    {
        return false;
    }
    if priority_session
        .open_stream(
            bulk_id,
            bulk_pri,
            WindowCredit::new(DEFAULT_INITIAL_STREAM_CREDIT),
        )
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
        .open_stream(
            bulk_id,
            bulk_pri,
            WindowCredit::new(DEFAULT_INITIAL_STREAM_CREDIT),
        )
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

pub(crate) fn run_selftest_server_loop(
    pending_replies: &mut ReplyBuffer<16, 512>,
    net: &KernelClient,
    nonce_ctr: &mut u64,
    lid: u32,
    port: u16,
) -> ! {
    // Wait for a client connection, perform auth, then do ping/pong over stream IO.
    let mut sid: Option<u32> = None;
    for _ in 0..50_000 {
        let nonce = crate::os::entry::next_nonce(nonce_ctr);
        let mut a = [0u8; 16];
        a[0] = MAGIC0;
        a[1] = MAGIC1;
        a[2] = VERSION;
        a[3] = OP_ACCEPT;
        a[4..8].copy_from_slice(&lid.to_le_bytes());
        a[8..16].copy_from_slice(&nonce.to_le_bytes());
        let rsp =
            match crate::os::entry::rpc_nonce(pending_replies, net, &a, OP_ACCEPT | 0x80, nonce) {
                Ok(v) => v,
                Err(_) => continue,
            };
        if rsp[0] == MAGIC0 && rsp[1] == MAGIC1 && rsp[2] == VERSION && rsp[3] == (OP_ACCEPT | 0x80)
        {
            if rsp[4] == STATUS_OK {
                sid = Some(u32::from_le_bytes([rsp[5], rsp[6], rsp[7], rsp[8]]));
                break;
            }
            if rsp[4] != STATUS_WOULD_BLOCK {
                break;
            }
        }
        let _ = yield_();
    }
    let Some(sid) = sid else {
        loop {
            let _ = yield_();
        }
    };

    // REAL Noise XK Handshake with selftest-client (RFC-0008).
    // SECURITY: bring-up test keys, NOT production custody.
    let server_static =
        StaticKeypair::from_secret(crate::os::entry::derive_test_secret(0xA0, port));
    // SECURITY: bring-up test keys, NOT production custody.
    let server_eph_seed = crate::os::entry::derive_test_secret(0xC0, port);
    // SECURITY: bring-up test keys, NOT production custody.
    let client_static_expected =
        StaticKeypair::from_secret(crate::os::entry::derive_test_secret(0xB0, port)).public;

    let mut responder = XkResponder::new(server_static, client_static_expected, server_eph_seed);

    let mut msg1 = [0u8; MSG1_LEN];
    if crate::os::entry::stream_read(pending_replies, net, nonce_ctr, sid, &mut msg1).is_err() {
        let _ = nexus_abi::debug_println("dsoftbusd: noise msg1 read FAIL");
        loop {
            let _ = yield_();
        }
    }

    let mut msg2 = [0u8; MSG2_LEN];
    if responder.read_msg1_write_msg2(&msg1, &mut msg2).is_err() {
        let _ = nexus_abi::debug_println("dsoftbusd: noise msg2 gen FAIL");
        loop {
            let _ = yield_();
        }
    }
    if crate::os::entry::stream_write(pending_replies, net, nonce_ctr, sid, &msg2).is_err() {
        let _ = nexus_abi::debug_println("dsoftbusd: noise msg2 write FAIL");
        loop {
            let _ = yield_();
        }
    }

    let mut msg3 = [0u8; MSG3_LEN];
    if crate::os::entry::stream_read(pending_replies, net, nonce_ctr, sid, &mut msg3).is_err() {
        let _ = nexus_abi::debug_println("dsoftbusd: noise msg3 read FAIL");
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
            let _ = nexus_abi::debug_println("dsoftbusd: noise msg3 FAIL");
            loop {
                let _ = yield_();
            }
        }
    };

    let mut _transport = Transport::new(transport_keys);
    let _ = nexus_abi::debug_println("dsoftbusd: auth ok");

    let mut got_ping = false;
    for _ in 0..50_000 {
        let nonce = crate::os::entry::next_nonce(nonce_ctr);
        let mut r = [0u8; 18];
        r[0] = MAGIC0;
        r[1] = MAGIC1;
        r[2] = VERSION;
        r[3] = OP_READ;
        r[4..8].copy_from_slice(&sid.to_le_bytes());
        r[8..10].copy_from_slice(&(4u16).to_le_bytes());
        r[10..18].copy_from_slice(&nonce.to_le_bytes());
        let rsp = match crate::os::entry::rpc_nonce(pending_replies, net, &r, OP_READ | 0x80, nonce)
        {
            Ok(v) => v,
            Err(_) => continue,
        };
        if rsp[0] == MAGIC0 && rsp[1] == MAGIC1 && rsp[2] == VERSION && rsp[3] == (OP_READ | 0x80) {
            if rsp[4] == STATUS_OK {
                let n = u16::from_le_bytes([rsp[5], rsp[6]]) as usize;
                if n == 4 && &rsp[7..11] == b"PING" {
                    got_ping = true;
                    break;
                }
            }
        }
        let _ = yield_();
    }
    if !got_ping {
        loop {
            let _ = yield_();
        }
    }

    let nonce = crate::os::entry::next_nonce(nonce_ctr);
    let mut w = [0u8; 22];
    w[0] = MAGIC0;
    w[1] = MAGIC1;
    w[2] = VERSION;
    w[3] = OP_WRITE;
    w[4..8].copy_from_slice(&sid.to_le_bytes());
    w[8..10].copy_from_slice(&(4u16).to_le_bytes());
    w[10..14].copy_from_slice(b"PONG");
    w[14..22].copy_from_slice(&nonce.to_le_bytes());
    let _ = crate::os::entry::rpc_nonce(pending_replies, net, &w, OP_WRITE | 0x80, nonce);
    let _ = nexus_abi::debug_println("dsoftbusd: os session ok");

    // Prove mux-v2 contract behavior in the OS execution path without synthetic
    // success markers. Markers below are emitted only after real state-machine checks.
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
