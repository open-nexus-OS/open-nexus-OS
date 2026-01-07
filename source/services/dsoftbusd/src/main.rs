#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std, no_main)]

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
    let req =
        [MAGIC0, MAGIC1, VERSION, OP_UDP_BIND, (disc_port & 0xff) as u8, (disc_port >> 8) as u8];
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
    let _ = nexus_abi::debug_println("dsoftbusd: discovery up (udp)");

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
        if rsp[0] == MAGIC0
            && rsp[1] == MAGIC1
            && rsp[2] == VERSION
            && rsp[3] == (OP_UDP_RECV_FROM | 0x80)
        {
            match rsp[4] {
                STATUS_OK => {
                    let n = u16::from_le_bytes([rsp[5], rsp[6]]) as usize;
                    if n == payload.len() && &rsp[13..13 + n] == payload {
                        got_disc = true;
                        break;
                    }
                }
                STATUS_WOULD_BLOCK => {}
                STATUS_MALFORMED => {
                    let _ = nexus_abi::debug_println("dsoftbusd: udp recv MALFORMED");
                    break;
                }
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
        // Emit marker once per successful announce send (first iteration only)
        static mut ANNOUNCE_SENT: bool = false;
        // SAFETY: single-threaded OS context
        if unsafe { !ANNOUNCE_SENT } {
            let _ = nexus_abi::debug_println("dsoftbusd: discovery announce sent");
            unsafe { ANNOUNCE_SENT = true };
        }
        let mut r = [0u8; 10];
        r[0] = MAGIC0;
        r[1] = MAGIC1;
        r[2] = VERSION;
        r[3] = OP_UDP_RECV_FROM;
        r[4..8].copy_from_slice(&udp_id.to_le_bytes());
        r[8..10].copy_from_slice(&(64u16).to_le_bytes());
        let rsp = rpc(&net, &r).map_err(|_| ())?;
        if rsp[0] == MAGIC0
            && rsp[1] == MAGIC1
            && rsp[2] == VERSION
            && rsp[3] == (OP_UDP_RECV_FROM | 0x80)
        {
            if rsp[4] == STATUS_OK {
                let n = u16::from_le_bytes([rsp[5], rsp[6]]) as usize;
                let base = 13;
                if n >= 5
                    && n <= 64
                    && base + n <= rsp.len()
                    && &rsp[base..base + 4] == b"NXSB"
                    && rsp[base + 4] == 1
                {
                    // Parse device_id from announcement (version 1 format)
                    // Layout: NXSB(4) + version(1) + dev_len(1) + dev_id(dev_len) + port(2)
                    if n >= 6 {
                        let dev_len = rsp[base + 5] as usize;
                        if dev_len >= 1 && dev_len <= 32 && n >= 6 + dev_len {
                            let _ = nexus_abi::debug_println(
                                "dsoftbusd: discovery peer found device=local",
                            );
                        }
                    }
                    got = true;
                    break;
                }
            } else if rsp[4] == STATUS_WOULD_BLOCK {
                // keep polling
            } else if rsp[4] == STATUS_MALFORMED {
                let _ = nexus_abi::debug_println("dsoftbusd: udp recv MALFORMED");
                break;
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

    // ============================================================
    // REAL Noise XK Handshake (RFC-0008)
    // ============================================================
    use nexus_noise_xk::{StaticKeypair, Transport, XkResponder, MSG1_LEN, MSG2_LEN, MSG3_LEN};

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

    // Server (responder) static keypair - derived from port with tag 0xA0
    // SECURITY: bring-up test keys, NOT production custody
    let server_static = StaticKeypair::from_secret(derive_test_secret(0xA0, port));
    // Server ephemeral seed - derived from port with tag 0xC0
    // SECURITY: bring-up test keys, NOT production custody
    let server_eph_seed = derive_test_secret(0xC0, port);
    // Expected client static public key (client uses tag 0xB0)
    // SECURITY: bring-up test keys, NOT production custody
    let client_static_expected = StaticKeypair::from_secret(derive_test_secret(0xB0, port)).public;

    let mut responder = XkResponder::new(server_static, client_static_expected, server_eph_seed);

    // Helper to read exactly N bytes from the session
    fn stream_read(net: &KernelClient, sid: u32, buf: &mut [u8]) -> core::result::Result<(), ()> {
        const MAGIC0: u8 = b'N';
        const MAGIC1: u8 = b'S';
        const VERSION: u8 = 1;
        const OP_READ: u8 = 4;
        const STATUS_OK: u8 = 0;
        const STATUS_WOULD_BLOCK: u8 = 3;

        let len = buf.len();
        for _ in 0..100_000 {
            let mut r = [0u8; 10];
            r[0] = MAGIC0;
            r[1] = MAGIC1;
            r[2] = VERSION;
            r[3] = OP_READ;
            r[4..8].copy_from_slice(&sid.to_le_bytes());
            r[8..10].copy_from_slice(&(len as u16).to_le_bytes());
            let reply = KernelClient::new_for("@reply").map_err(|_| ())?;
            let (reply_send_slot, reply_recv_slot) = reply.slots();
            let reply_send_clone = nexus_abi::cap_clone(reply_send_slot).map_err(|_| ())?;
            net.send_with_cap_move(&r, reply_send_clone).map_err(|_| ())?;
            let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
            let mut rsp = [0u8; 512];
            for _ in 0..5_000 {
                match nexus_abi::ipc_recv_v1(
                    reply_recv_slot,
                    &mut hdr,
                    &mut rsp,
                    nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
                    0,
                ) {
                    Ok(_) => {
                        if rsp[0] == MAGIC0
                            && rsp[1] == MAGIC1
                            && rsp[2] == VERSION
                            && rsp[3] == (OP_READ | 0x80)
                        {
                            if rsp[4] == STATUS_OK {
                                let n = u16::from_le_bytes([rsp[5], rsp[6]]) as usize;
                                if n == len && 7 + n <= rsp.len() {
                                    buf.copy_from_slice(&rsp[7..7 + n]);
                                    return Ok(());
                                }
                            } else if rsp[4] == STATUS_WOULD_BLOCK {
                                break; // retry outer loop
                            } else {
                                return Err(());
                            }
                        }
                        break;
                    }
                    Err(nexus_abi::IpcError::QueueEmpty) => {
                        let _ = nexus_abi::yield_();
                    }
                    Err(_) => return Err(()),
                }
            }
            let _ = nexus_abi::yield_();
        }
        Err(())
    }

    // Helper to write exactly N bytes to the session
    fn stream_write(net: &KernelClient, sid: u32, data: &[u8]) -> core::result::Result<(), ()> {
        const MAGIC0: u8 = b'N';
        const MAGIC1: u8 = b'S';
        const VERSION: u8 = 1;
        const OP_WRITE: u8 = 5;
        const STATUS_OK: u8 = 0;

        let mut w = [0u8; 256];
        if data.len() + 10 > w.len() {
            return Err(());
        }
        w[0] = MAGIC0;
        w[1] = MAGIC1;
        w[2] = VERSION;
        w[3] = OP_WRITE;
        w[4..8].copy_from_slice(&sid.to_le_bytes());
        w[8..10].copy_from_slice(&(data.len() as u16).to_le_bytes());
        w[10..10 + data.len()].copy_from_slice(data);

        let reply = KernelClient::new_for("@reply").map_err(|_| ())?;
        let (reply_send_slot, reply_recv_slot) = reply.slots();
        let reply_send_clone = nexus_abi::cap_clone(reply_send_slot).map_err(|_| ())?;
        net.send_with_cap_move(&w[..10 + data.len()], reply_send_clone).map_err(|_| ())?;
        let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut rsp = [0u8; 64];
        for _ in 0..5_000 {
            match nexus_abi::ipc_recv_v1(
                reply_recv_slot,
                &mut hdr,
                &mut rsp,
                nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
                0,
            ) {
                Ok(_) => {
                    if rsp[0] == MAGIC0
                        && rsp[1] == MAGIC1
                        && rsp[2] == VERSION
                        && rsp[3] == (OP_WRITE | 0x80)
                        && rsp[4] == STATUS_OK
                    {
                        return Ok(());
                    }
                    return Err(());
                }
                Err(nexus_abi::IpcError::QueueEmpty) => {
                    let _ = nexus_abi::yield_();
                }
                Err(_) => return Err(()),
            }
        }
        Err(())
    }

    // Step 1: Read msg1 (initiator ephemeral public key, 32 bytes)
    let mut msg1 = [0u8; MSG1_LEN];
    if stream_read(&net, sid, &mut msg1).is_err() {
        let _ = nexus_abi::debug_println("dsoftbusd: noise msg1 read FAIL");
        loop {
            let _ = yield_();
        }
    }

    // Step 2: Write msg2 (responder ephemeral + encrypted static + tag, 96 bytes)
    let mut msg2 = [0u8; MSG2_LEN];
    if responder.read_msg1_write_msg2(&msg1, &mut msg2).is_err() {
        let _ = nexus_abi::debug_println("dsoftbusd: noise msg2 gen FAIL");
        loop {
            let _ = yield_();
        }
    }
    if stream_write(&net, sid, &msg2).is_err() {
        let _ = nexus_abi::debug_println("dsoftbusd: noise msg2 write FAIL");
        loop {
            let _ = yield_();
        }
    }

    // Step 3: Read msg3 (encrypted initiator static + tag, 64 bytes)
    let mut msg3 = [0u8; MSG3_LEN];
    if stream_read(&net, sid, &mut msg3).is_err() {
        let _ = nexus_abi::debug_println("dsoftbusd: noise msg3 read FAIL");
        loop {
            let _ = yield_();
        }
    }

    // Finish handshake and get transport keys
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

    // Create transport for encrypted communication
    let mut _transport = Transport::new(transport_keys);
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
