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
        let rsp = match crate::os::entry::rpc_nonce(pending_replies, net, &a, OP_ACCEPT | 0x80, nonce) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if rsp[0] == MAGIC0 && rsp[1] == MAGIC1 && rsp[2] == VERSION && rsp[3] == (OP_ACCEPT | 0x80) {
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
    let server_static = StaticKeypair::from_secret(crate::os::entry::derive_test_secret(0xA0, port));
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
        let rsp = match crate::os::entry::rpc_nonce(pending_replies, net, &r, OP_READ | 0x80, nonce) {
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

    loop {
        let _ = yield_();
    }
}
