#![cfg_attr(
    all(nexus_env = "os", target_arch = "riscv64", target_os = "none"),
    no_std,
    no_main
)]

//! CONTEXT: DSoftBus daemon entrypoint
//! INTENT: Local-only transport milestone over userspace networking
//! DEPS (OS bring-up): nexus-net facade + smoltcp backend; no kernel networking
//! READINESS: prints "dsoftbusd: ready" in OS/QEMU

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn os_entry() -> core::result::Result<(), ()> {
    use nexus_abi::yield_;
    use nexus_ipc::KernelClient;

    let _ = nexus_abi::debug_println("dsoftbusd: ready");

    // dsoftbusd must NOT own MMIO; it uses netstackd's IPC facade.
    if KernelClient::new_for("keystored").is_ok() {
        let _ = nexus_abi::debug_println("dsoftbusd: routing keystored ok");
    } else {
        let _ = nexus_abi::debug_println("dsoftbusd: routing keystored FAIL");
    }
    let net = loop {
        match KernelClient::new_for("netstackd") {
            Ok(c) => break c,
            Err(_) => {
                let _ = yield_();
            }
        }
    };
    let _ = nexus_abi::debug_println("dsoftbusd: routing netstackd ok");

    const MAGIC0: u8 = b'N';
    const MAGIC1: u8 = b'S';
    const VERSION: u8 = 1;
    const OP_LISTEN: u8 = 1;
    const OP_ACCEPT: u8 = 2;
    const OP_READ: u8 = 4;
    const OP_WRITE: u8 = 5;
    const OP_UDP_BIND: u8 = 6;
    const OP_UDP_SEND_TO: u8 = 7;
    const OP_UDP_RECV_FROM: u8 = 8;
    const STATUS_OK: u8 = 0;
    const STATUS_WOULD_BLOCK: u8 = 3;
    const STATUS_MALFORMED: u8 = 2;

    fn rpc(net: &KernelClient, req: &[u8]) -> core::result::Result<[u8; 512], ()> {
        let reply = KernelClient::new_for("@reply").map_err(|_| ())?;
        let (reply_send_slot, reply_recv_slot) = reply.slots();
        let reply_send_clone = nexus_abi::cap_clone(reply_send_slot).map_err(|_| ())?;
        net.send_with_cap_move(req, reply_send_clone).map_err(|_| ())?;
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

    // UDP discovery socket proof (local-only): bind + loopback send/recv via netstackd.
    // This is *not* claiming on-wire multicast yet; it only proves the UDP facade path works.
    let disc_port: u16 = 37_020;
    let req = [
        MAGIC0,
        MAGIC1,
        VERSION,
        OP_UDP_BIND,
        (disc_port & 0xff) as u8,
        (disc_port >> 8) as u8,
    ];
    let rsp = rpc(&net, &req).map_err(|_| ())?;
    if rsp[0] != MAGIC0
        || rsp[1] != MAGIC1
        || rsp[2] != VERSION
        || rsp[3] != (OP_UDP_BIND | 0x80)
        || rsp[4] != STATUS_OK
    {
        let _ = nexus_abi::debug_println("dsoftbusd: udp bind FAIL");
        loop {
            let _ = yield_();
        }
    }
    let udp_id = u32::from_le_bytes([rsp[5], rsp[6], rsp[7], rsp[8]]);

    // Minimal discovery announce/receive loop (local-only, loopback):
    let mut got_disc = false;
    for _ in 0..10_000 {
        // SEND discovery
        let payload = b"NXDISC1";
        let mut send = [0u8; 16 + 32];
        send[0] = MAGIC0;
        send[1] = MAGIC1;
        send[2] = VERSION;
        send[3] = OP_UDP_SEND_TO;
        send[4..8].copy_from_slice(&udp_id.to_le_bytes());
        send[8..12].copy_from_slice(&[10, 0, 2, 15]);
        send[12..14].copy_from_slice(&disc_port.to_le_bytes());
        send[14..16].copy_from_slice(&(payload.len() as u16).to_le_bytes());
        send[16..16 + payload.len()].copy_from_slice(payload);
        let _ = rpc(&net, &send[..16 + payload.len()]).map_err(|_| ())?;

        // RECV
        let mut r = [0u8; 10];
        r[0] = MAGIC0;
        r[1] = MAGIC1;
        r[2] = VERSION;
        r[3] = OP_UDP_RECV_FROM;
        r[4..8].copy_from_slice(&udp_id.to_le_bytes());
        r[8..10].copy_from_slice(&(32u16).to_le_bytes());
        let rsp = rpc(&net, &r).map_err(|_| ())?;
        if rsp[0] == MAGIC0 && rsp[1] == MAGIC1 && rsp[2] == VERSION && rsp[3] == (OP_UDP_RECV_FROM | 0x80) {
            match rsp[4] {
                STATUS_OK => {
                    let n = u16::from_le_bytes([rsp[5], rsp[6]]) as usize;
                    if n == payload.len() && &rsp[13..13 + n] == payload {
                        got_disc = true;
                        break;
                    }
                }
                STATUS_WOULD_BLOCK => {}
                _ => {
                    let _ = nexus_abi::debug_println("dsoftbusd: udp recv FAIL");
                    break;
                }
            }
        }
        let _ = yield_();
    }
    if !got_disc {
        let _ = nexus_abi::debug_println("dsoftbusd: udp discovery FAIL");
        loop {
            let _ = yield_();
        }
    }

    // Ask netstackd to listen on our DSoftBus session port.
    let port: u16 = 34_567;
    let req = [MAGIC0, MAGIC1, VERSION, OP_LISTEN, (port & 0xff) as u8, (port >> 8) as u8];
    let rsp = rpc(&net, &req).map_err(|_| ())?;
    if rsp[0] != MAGIC0
        || rsp[1] != MAGIC1
        || rsp[2] != VERSION
        || rsp[3] != (OP_LISTEN | 0x80)
        || rsp[4] != STATUS_OK
    {
        let _ = nexus_abi::debug_println("dsoftbusd: listen FAIL");
        loop {
            let _ = yield_();
        }
    }
    let lid = u32::from_le_bytes([rsp[5], rsp[6], rsp[7], rsp[8]]);

    // UDP announce/receive proof (structured, bounded, loopback via netstackd).
    let dev_id = b"dev.local";
    let dev_len = core::cmp::min(dev_id.len(), 32) as u8;
    let payload_len = 4 + 1 + (dev_len as usize) + 2;
    let mut payload = [0u8; 64];
    payload[0..4].copy_from_slice(b"NXSB");
    payload[4] = 1;
    payload[5] = dev_len;
    payload[6..6 + dev_len as usize].copy_from_slice(&dev_id[..dev_len as usize]);
    let port_off = 6 + dev_len as usize;
    payload[port_off..port_off + 2].copy_from_slice(&disc_port.to_le_bytes());
    let payload = &payload[..payload_len];
    // RECV_FROM: [.., udp_id:u32, max:u16]
    let mut got = false;
    for _ in 0..10_000 {
        let mut send = [0u8; 16 + 64];
        send[0] = MAGIC0;
        send[1] = MAGIC1;
        send[2] = VERSION;
        send[3] = OP_UDP_SEND_TO;
        send[4..8].copy_from_slice(&udp_id.to_le_bytes());
        send[8..12].copy_from_slice(&[10, 0, 2, 15]);
        send[12..14].copy_from_slice(&disc_port.to_le_bytes());
        send[14..16].copy_from_slice(&(payload.len() as u16).to_le_bytes());
        send[16..16 + payload.len()].copy_from_slice(payload);
        let send_rsp = rpc(&net, &send[..16 + payload.len()]).map_err(|_| ())?;
        if send_rsp[0] != MAGIC0
            || send_rsp[1] != MAGIC1
            || send_rsp[2] != VERSION
            || send_rsp[3] != (OP_UDP_SEND_TO | 0x80)
            || send_rsp[4] != STATUS_OK
        {
            let _ = yield_();
            continue;
        }
        let mut r = [0u8; 10];
        r[0] = MAGIC0;
        r[1] = MAGIC1;
        r[2] = VERSION;
        r[3] = OP_UDP_RECV_FROM;
        r[4..8].copy_from_slice(&udp_id.to_le_bytes());
        r[8..10].copy_from_slice(&(64u16).to_le_bytes());
        let rsp = rpc(&net, &r).map_err(|_| ())?;
        if rsp[0] == MAGIC0 && rsp[1] == MAGIC1 && rsp[2] == VERSION && rsp[3] == (OP_UDP_RECV_FROM | 0x80) {
            if rsp[4] == STATUS_OK {
                let n = u16::from_le_bytes([rsp[5], rsp[6]]) as usize;
                let base = 13;
                if n >= 5 && n <= 64 && base + n <= rsp.len() && &rsp[base..base + 4] == b"NXSB" && rsp[base + 4] == 1 {
                    got = true;
                    break;
                }
            } else if rsp[4] == STATUS_WOULD_BLOCK {
                // keep polling
            } else {
                let _ = nexus_abi::debug_println("dsoftbusd: udp recv FAIL");
                break;
            }
        }
        let _ = yield_();
    }
    if !got {
        let _ = nexus_abi::debug_println("dsoftbusd: udp loopback FAIL");
        loop {
            let _ = yield_();
        }
    }

    let _ = nexus_abi::debug_println("dsoftbusd: os transport up (udp+tcp)");

    // Wait for a client connection, perform a minimal auth check, then do ping/pong over proxied stream IO.
    let mut sid: Option<u32> = None;
    for _ in 0..50_000 {
        let mut a = [0u8; 8];
        a[0] = MAGIC0;
        a[1] = MAGIC1;
        a[2] = VERSION;
        a[3] = OP_ACCEPT;
        a[4..8].copy_from_slice(&lid.to_le_bytes());
        let rsp = rpc(&net, &a).map_err(|_| ())?;
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

    // Auth handshake (bounded, deterministic challenge/response).
    const AUTH_MAGIC: &[u8; 4] = b"NOI1";
    // Expected client static pubkey.
    const CLIENT_PUB: [u8; 32] = [
        0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88,
        0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x10,
        0x21, 0x32, 0x43, 0x54, 0x65, 0x76, 0x87, 0x98,
        0xa9, 0xba, 0xcb, 0xdc, 0xed, 0xfe, 0x0f, 0x1e,
    ];
    // Server static pubkey (for binding).
    const SERVER_PUB: [u8; 32] = [
        0x7a, 0x6b, 0x5c, 0x4d, 0x3e, 0x2f, 0x1a, 0x0b,
        0x9c, 0x8d, 0x7e, 0x6f, 0x5a, 0x4b, 0x3c, 0x2d,
        0x1e, 0x0f, 0xfa, 0xeb, 0xdc, 0xcd, 0xbe, 0xaf,
        0x90, 0x81, 0x72, 0x63, 0x54, 0x45, 0x36, 0x27,
    ];
    const AUTH_SECRET: [u8; 64] = [
        0xde, 0xad, 0xbe, 0xef, 0xaa, 0xbb, 0xcc, 0xdd,
        0x01, 0x02, 0x03, 0x04, 0x10, 0x20, 0x30, 0x40,
        0x50, 0x60, 0x70, 0x80, 0x90, 0xa0, 0xb0, 0xc0,
        0xd0, 0xe0, 0xf0, 0x0f, 0x1a, 0x2b, 0x3c, 0x4d,
        0x5e, 0x6f, 0x7a, 0x8b, 0x9c, 0xad, 0xbe, 0xcf,
        0xda, 0xeb, 0xfc, 0x0d, 0x1e, 0x2f, 0x3a, 0x4b,
        0x5c, 0x6d, 0x7e, 0x8f, 0x9a, 0xab, 0xbc, 0xcd,
        0xde, 0xef, 0xfa, 0xfb, 0xfc, 0xfd, 0xfe, 0xff,
    ];
    // Step 1: read client hello (magic + pubkey).
    let mut authed = false;
    for _ in 0..50_000 {
        let mut r = [0u8; 10];
        r[0] = MAGIC0;
        r[1] = MAGIC1;
        r[2] = VERSION;
        r[3] = OP_READ;
        r[4..8].copy_from_slice(&sid.to_le_bytes());
        r[8..10].copy_from_slice(&(40u16).to_le_bytes());
        let rsp = rpc(&net, &r).map_err(|_| ())?;
        if rsp[0] == MAGIC0 && rsp[1] == MAGIC1 && rsp[2] == VERSION && rsp[3] == (OP_READ | 0x80) {
            match rsp[4] {
                STATUS_OK => {
                    let n = u16::from_le_bytes([rsp[5], rsp[6]]) as usize;
                    if n == 36 && &rsp[7..11] == AUTH_MAGIC && rsp[11..43] == CLIENT_PUB {
                        authed = true;
                        break;
                    } else {
                        let _ = nexus_abi::debug_println("dsoftbusd: auth hello FAIL");
                        loop {
                            let _ = yield_();
                        }
                    }
                }
                STATUS_WOULD_BLOCK => {}
                _ => {
                    let _ = nexus_abi::debug_println("dsoftbusd: auth hello FAIL");
                    loop {
                        let _ = yield_();
                    }
                }
            }
        }
        let _ = yield_();
    }
    if !authed {
        let _ = nexus_abi::debug_println("dsoftbusd: auth hello FAIL");
        loop {
            let _ = yield_();
        }
    }

    // Step 2: send challenge (deterministic nonce + server pub to bind identity).
    let nonce: [u8; 8] = [0x10, 0x21, 0x32, 0x43, 0x54, 0x65, 0x76, 0x87];
    let mut chal = [0u8; 54];
    chal[0] = MAGIC0;
    chal[1] = MAGIC1;
    chal[2] = VERSION;
    chal[3] = OP_WRITE;
    chal[4..8].copy_from_slice(&sid.to_le_bytes());
    chal[8..10].copy_from_slice(&(44u16).to_le_bytes());
    chal[10..14].copy_from_slice(b"CHAL");
    chal[14..46].copy_from_slice(&SERVER_PUB);
    chal[46..54].copy_from_slice(&nonce);
    let _ = rpc(&net, &chal);

    // Step 3: read response (magic + tag).
    let mut authed = false;
    for _ in 0..50_000 {
        let mut r = [0u8; 10];
        r[0] = MAGIC0;
        r[1] = MAGIC1;
        r[2] = VERSION;
        r[3] = OP_READ;
        r[4..8].copy_from_slice(&sid.to_le_bytes());
        r[8..10].copy_from_slice(&(96u16).to_le_bytes());
        let rsp = rpc(&net, &r).map_err(|_| ())?;
        if rsp[0] == MAGIC0 && rsp[1] == MAGIC1 && rsp[2] == VERSION && rsp[3] == (OP_READ | 0x80) {
            match rsp[4] {
                STATUS_OK => {
                    let n = u16::from_le_bytes([rsp[5], rsp[6]]) as usize;
                    if n == 68 && &rsp[7..11] == AUTH_MAGIC {
                        let mut expected = [0u8; 64];
                        for (i, b) in expected.iter_mut().enumerate() {
                            *b = AUTH_SECRET[i]
                                ^ CLIENT_PUB[i % CLIENT_PUB.len()]
                                ^ SERVER_PUB[i % SERVER_PUB.len()]
                                ^ nonce[i % nonce.len()];
                        }
                        if rsp.len() >= 75 && rsp[11..75] == expected {
                            authed = true;
                            break;
                        } else {
                            let _ = nexus_abi::debug_println("dsoftbusd: auth tag mismatch");
                            loop {
                                let _ = yield_();
                            }
                        }
                    } else {
                        let _ = nexus_abi::debug_println("dsoftbusd: auth rsp FAIL len");
                        loop {
                            let _ = yield_();
                        }
                    }
                }
                STATUS_WOULD_BLOCK => {}
                _ => {
                    let _ = nexus_abi::debug_println("dsoftbusd: auth rsp FAIL");
                    loop {
                        let _ = yield_();
                    }
                }
            }
        }
        let _ = yield_();
    }
    if !authed {
        let _ = nexus_abi::debug_println("dsoftbusd: auth rsp FAIL");
        loop {
            let _ = yield_();
        }
    }

    // Send auth ack.
    let mut ack = [0u8; 16];
    ack[0] = MAGIC0;
    ack[1] = MAGIC1;
    ack[2] = VERSION;
    ack[3] = OP_WRITE;
    ack[4..8].copy_from_slice(&sid.to_le_bytes());
    ack[8..10].copy_from_slice(&(6u16).to_le_bytes());
    ack[10..16].copy_from_slice(b"AUTHOK");
    let _ = rpc(&net, &ack);
    let _ = nexus_abi::debug_println("dsoftbusd: auth ok");

    // Read "PING", reply "PONG".
    let mut got_ping = false;
    for _ in 0..50_000 {
        // READ sid max=4
        let mut r = [0u8; 10];
        r[0] = MAGIC0;
        r[1] = MAGIC1;
        r[2] = VERSION;
        r[3] = OP_READ;
        r[4..8].copy_from_slice(&sid.to_le_bytes());
        r[8..10].copy_from_slice(&(4u16).to_le_bytes());
        let rsp = rpc(&net, &r).map_err(|_| ())?;
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

    // WRITE sid "PONG"
    let mut w = [0u8; 14];
    w[0] = MAGIC0;
    w[1] = MAGIC1;
    w[2] = VERSION;
    w[3] = OP_WRITE;
    w[4..8].copy_from_slice(&sid.to_le_bytes());
    w[8..10].copy_from_slice(&(4u16).to_le_bytes());
    w[10..14].copy_from_slice(b"PONG");
    let _ = rpc(&net, &w);
    let _ = nexus_abi::debug_println("dsoftbusd: os session ok");

    // Stay alive cooperatively.
    loop {
        let _ = yield_();
    }
}

#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
fn main() -> ! {
    dsoftbus::run();
    loop {
        core::hint::spin_loop();
    }
}
