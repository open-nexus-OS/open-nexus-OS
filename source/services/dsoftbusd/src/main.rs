#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std, no_main)]

//! CONTEXT: DSoftBus daemon entrypoint
//! INTENT: Local-only transport milestone over userspace networking
//! DEPS (OS bring-up): nexus-net facade + smoltcp backend; no kernel networking
//! READINESS: prints "dsoftbusd: ready" in OS/QEMU

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
extern crate alloc;

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn os_entry() -> core::result::Result<(), ()> {
    use nexus_abi::yield_;
    use nexus_ipc::KernelClient;
    use alloc::string::String;
    use alloc::vec::Vec;
    use nexus_discovery_packet::{decode_announce_v1, encode_announce_v1, AnnounceV1};
    use nexus_peer_lru::{PeerEntry, PeerLru};

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

    // UDP discovery socket bind (Phase 1): bind to 0.0.0.0:<port> so we can receive broadcast/multicast
    // traffic (as supported by the underlying QEMU network backend).
    let disc_port: u16 = 37_020;
    // OP_UDP_BIND v2: [magic, magic, ver, op, ip[4], port:u16le]
    let req = [
        MAGIC0,
        MAGIC1,
        VERSION,
        OP_UDP_BIND,
        0,
        0,
        0,
        0, // 0.0.0.0
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
    // Phase 1 contract: once bound + receive loop is active (we start it immediately below), we consider
    // discovery transport armed. Multicast may not be supported by all backends; broadcast fallback is
    // used when multicast is ineffective.
    // QEMU bring-up uses netstackd UDP loopback for discovery (deterministic, bounded).
    // Real subnet multicast/broadcast discovery is a follow-on phase (see TASK-0005 / TASK-0024).
    let _ = nexus_abi::debug_println("dsoftbusd: discovery up (udp loopback)");

    // Bounded peer cache (Phase 1): keep a small, deterministic LRU of recently seen peers.
    let mut peers = PeerLru::with_default_capacity();
    // Parallel table for last-seen IPv4 source address (bounded by peers.len()).
    // NOTE: `nexus-peer-lru` currently tracks peer identity/port/key; the source IP is derived from UDP recv metadata.
    let mut peer_ips: Vec<(String, [u8; 4])> = Vec::new();

    fn rebuild_peer_ips(peers: &PeerLru, ips: &mut Vec<(String, [u8; 4])>) {
        // Keep only entries that exist in the LRU and preserve LRU order deterministically.
        let mut out: Vec<(String, [u8; 4])> = Vec::new();
        for p in peers.peers() {
            if let Some((_id, ip)) = ips.iter().find(|(id, _)| id == &p.device_id) {
                out.push((p.device_id.clone(), *ip));
            }
        }
        *ips = out;
    }

    fn set_peer_ip(
        peers: &PeerLru,
        ips: &mut Vec<(String, [u8; 4])>,
        device_id: &str,
        ip: [u8; 4],
    ) {
        if let Some(pos) = ips.iter().position(|(id, _)| id == device_id) {
            ips[pos].1 = ip;
        } else {
            ips.push((String::from(device_id), ip));
        }
        rebuild_peer_ips(peers, ips);
    }

    fn get_peer_ip(ips: &[(String, [u8; 4])], device_id: &str) -> Option<[u8; 4]> {
        ips.iter().find(|(id, _)| id == device_id).map(|(_, ip)| *ip)
    }

    // Phase 1 discovery loop:
    // - Send AnnounceV1 periodically (deterministic schedule)
    // - Receive AnnounceV1 from peers, decode/bound, store in PeerLru
    //
    // NOTE: For QEMU bring-up we also fall back to sending to LOCAL_IP to ensure at least one
    // peer is observed under usernet backends that do not deliver multicast/broadcast.
    let mut announce_sent = false;
    // Dual-node bring-up: we require that node-b is learned via the discovery receive path
    // (not injected/seeded), so discovery-driven connect is actually observable.
    let node_b_device_id = "node-b";
    let node_b_port: u16 = 34_568;
    for i in 0..20_000u64 {
        // Send (bounded, deterministic)
        if !announce_sent && (i % 64 == 0) {
            let ann_b = AnnounceV1 {
                device_id: String::from(node_b_device_id),
                port: node_b_port,
                // SECURITY: bring-up test keys, NOT production custody
                noise_static: nexus_noise_xk::StaticKeypair::from_secret(derive_test_secret(0xD1, node_b_port)).public,
                services: alloc::vec!["dsoftbusd".into()],
            };

            fn send_announce(
                net: &KernelClient,
                udp_id: u32,
                disc_port: u16,
                bytes: &[u8],
            ) -> core::result::Result<bool, ()> {
                const MAGIC0: u8 = b'N';
                const MAGIC1: u8 = b'S';
                const VERSION: u8 = 1;
                const OP_UDP_SEND_TO: u8 = 7;
                const STATUS_OK: u8 = 0;

                const LOCAL_IP: [u8; 4] = [10, 0, 2, 15];

                let mut send = [0u8; 16 + 256];
                let hdr_len = 16;
                if hdr_len + bytes.len() > send.len() {
                    return Ok(false);
                }
                // Common header
                send[0] = MAGIC0;
                send[1] = MAGIC1;
                send[2] = VERSION;
                send[3] = OP_UDP_SEND_TO;
                send[4..8].copy_from_slice(&udp_id.to_le_bytes());
                send[12..14].copy_from_slice(&disc_port.to_le_bytes());
                send[14..16].copy_from_slice(&(bytes.len() as u16).to_le_bytes());
                send[16..16 + bytes.len()].copy_from_slice(bytes);

                // QEMU bring-up note: netstackd's loopback UDP is a tiny byte-ring (not datagram framed).
                // To keep parsing deterministic, we send a SINGLE announce to LOCAL_IP, then immediately poll recv.
                // Real subnet discovery is covered by follow-on tasks (non-loopback UDP socket backend).
                send[8..12].copy_from_slice(&LOCAL_IP);
                let rsp = rpc(net, &send[..hdr_len + bytes.len()])?;
                Ok(
                    rsp[0] == MAGIC0
                        && rsp[1] == MAGIC1
                        && rsp[2] == VERSION
                        && rsp[3] == (OP_UDP_SEND_TO | 0x80)
                        && rsp[4] == STATUS_OK,
                )
            }

            let ok_b = encode_announce_v1(&ann_b)
                .ok()
                .and_then(|b| send_announce(&net, udp_id, disc_port, &b).ok())
                .unwrap_or(false);

            if ok_b {
                let _ = nexus_abi::debug_println("dsoftbusd: discovery announce sent");
                announce_sent = true;
            }
        }

        // Receive (bounded)
        let mut r = [0u8; 10];
        r[0] = MAGIC0;
        r[1] = MAGIC1;
        r[2] = VERSION;
        r[3] = OP_UDP_RECV_FROM;
        r[4..8].copy_from_slice(&udp_id.to_le_bytes());
        r[8..10].copy_from_slice(&(256u16).to_le_bytes());
        let rsp = rpc(&net, &r).map_err(|_| ())?;
        if rsp[0] == MAGIC0 && rsp[1] == MAGIC1 && rsp[2] == VERSION && rsp[3] == (OP_UDP_RECV_FROM | 0x80)
        {
            match rsp[4] {
                STATUS_OK => {
                    let n = u16::from_le_bytes([rsp[5], rsp[6]]) as usize;
                    let from_ip = [rsp[7], rsp[8], rsp[9], rsp[10]];
                    // from_port is rsp[11..13], ignored for discovery
                    let base = 13;
                    if n <= 256 && base + n <= rsp.len() {
                        let payload = &rsp[base..base + n];
                        if let Ok(pkt) = decode_announce_v1(payload) {
                            let entry = PeerEntry::new(
                                pkt.device_id.clone(),
                                pkt.port,
                                pkt.noise_static,
                                pkt.services,
                            );
                            peers.insert(entry);
                            set_peer_ip(&peers, &mut peer_ips, &pkt.device_id, from_ip);
                            // In dual-node bring-up, require that we learned node-b before proceeding.
                            if peers.peek(node_b_device_id).is_some() {
                                // Marker for first peer observation (keep existing marker stable for CI bring-up).
                                let _ = nexus_abi::debug_println(
                                    "dsoftbusd: discovery peer found device=local",
                                );
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

    // NOTE: Legacy “structured NXSB loopback proof” was removed in favor of the canonical,
    // bounded AnnounceV1 encode/decode path above (`nexus-discovery-packet` + `nexus-peer-lru`).

    let _ = nexus_abi::debug_println("dsoftbusd: os transport up (udp+tcp)");

    // ============================================================
    // TASK-0004: Dual-node mode (RFC-0007 Phase 1)
    // ============================================================
    // Create two logical nodes (A and B) within this single process:
    // - Node A: existing listener on port 34567
    // - Node B: new listener on port 34568
    // Node A connects to Node B, completes handshake, proves dual-node session.

    use nexus_noise_xk::{StaticKeypair, Transport, XkInitiator, XkResponder, MSG1_LEN, MSG2_LEN, MSG3_LEN};

    // SECURITY: bring-up test keys, NOT production custody
    // These keys are deterministic and derived from port for reproducibility only.
    fn derive_test_secret(tag: u8, port: u16) -> [u8; 32] {
        let mut seed = [0u8; 32];
        seed[0] = tag;
        seed[1] = (port >> 8) as u8;
        seed[2] = (port & 0xff) as u8;
        for i in 3..32 {
            seed[i] = ((tag as u16).wrapping_mul(port).wrapping_add(i as u16) & 0xff) as u8;
        }
        seed
    }

    // Set up listener for node B (port 34568)
    let port_b: u16 = 34_568;
    let req_b = [MAGIC0, MAGIC1, VERSION, OP_LISTEN, (port_b & 0xff) as u8, (port_b >> 8) as u8];
    let rsp_b = rpc(&net, &req_b).map_err(|_| ())?;
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

    // Helper to connect to a TCP port via netstackd
    fn tcp_connect(net: &KernelClient, ip: [u8; 4], port: u16) -> core::result::Result<u32, ()> {
        const MAGIC0: u8 = b'N';
        const MAGIC1: u8 = b'S';
        const VERSION: u8 = 1;
        const OP_CONNECT: u8 = 3;
        const STATUS_OK: u8 = 0;
        const STATUS_WOULD_BLOCK: u8 = 3;

        fn rpc_inner(net: &KernelClient, req: &[u8]) -> core::result::Result<[u8; 512], ()> {
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
                    Ok(_) => return Ok(buf),
                    Err(nexus_abi::IpcError::QueueEmpty) => {
                        let _ = nexus_abi::yield_();
                    }
                    Err(_) => return Err(()),
                }
            }
            Err(())
        }

        for _ in 0..50_000 {
            let mut c = [0u8; 10];
            c[0] = MAGIC0;
            c[1] = MAGIC1;
            c[2] = VERSION;
            c[3] = OP_CONNECT;
            c[4..8].copy_from_slice(&ip);
            c[8..10].copy_from_slice(&port.to_le_bytes());
            let rsp = rpc_inner(net, &c)?;
            if rsp[0] == MAGIC0
                && rsp[1] == MAGIC1
                && rsp[2] == VERSION
                && rsp[3] == (OP_CONNECT | 0x80)
            {
                if rsp[4] == STATUS_OK {
                    return Ok(u32::from_le_bytes([rsp[5], rsp[6], rsp[7], rsp[8]]));
                }
                if rsp[4] == STATUS_WOULD_BLOCK {
                    let _ = nexus_abi::yield_();
                    continue;
                }
            }
            let _ = nexus_abi::yield_();
        }
        Err(())
    }

    // Helper: accept on listener
    fn tcp_accept(net: &KernelClient, lid: u32) -> core::result::Result<u32, ()> {
        const MAGIC0: u8 = b'N';
        const MAGIC1: u8 = b'S';
        const VERSION: u8 = 1;
        const OP_ACCEPT: u8 = 2;
        const STATUS_OK: u8 = 0;
        const STATUS_WOULD_BLOCK: u8 = 3;

        fn rpc_inner(net: &KernelClient, req: &[u8]) -> core::result::Result<[u8; 512], ()> {
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
                    Ok(_) => return Ok(buf),
                    Err(nexus_abi::IpcError::QueueEmpty) => {
                        let _ = nexus_abi::yield_();
                    }
                    Err(_) => return Err(()),
                }
            }
            Err(())
        }

        for _ in 0..50_000 {
            let mut a = [0u8; 8];
            a[0] = MAGIC0;
            a[1] = MAGIC1;
            a[2] = VERSION;
            a[3] = OP_ACCEPT;
            a[4..8].copy_from_slice(&lid.to_le_bytes());
            let rsp = rpc_inner(net, &a)?;
            if rsp[0] == MAGIC0
                && rsp[1] == MAGIC1
                && rsp[2] == VERSION
                && rsp[3] == (OP_ACCEPT | 0x80)
            {
                if rsp[4] == STATUS_OK {
                    return Ok(u32::from_le_bytes([rsp[5], rsp[6], rsp[7], rsp[8]]));
                }
                if rsp[4] == STATUS_WOULD_BLOCK {
                    let _ = nexus_abi::yield_();
                    continue;
                }
            }
            let _ = nexus_abi::yield_();
        }
        Err(())
    }

    // Dual-node session: Node A (initiator) connects to Node B (responder)
    // Both run in this process, proving in-process multi-node capability.
    //
    // Node A: port 34567 (existing), initiates connection
    // Node B: port 34568, accepts connection

    // Start connection from A to B (discovery-driven)
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

    // Discovery-driven connect marker (RFC-0007 GAP 2).
    let _ = nexus_abi::debug_println("dsoftbusd: session connect peer=node-b");

    let connect_result = tcp_connect(&net, peer_ip, peer_b.port);

    // Accept the connection on B side
    let accept_result = tcp_accept(&net, lid_b);

    let (sid_a, sid_b) = match (connect_result, accept_result) {
        (Ok(a), Ok(b)) => (a, b),
        _ => {
            let _ = nexus_abi::debug_println("dsoftbusd: dual-node connect FAIL");
            loop {
                let _ = yield_();
            }
        }
    };

    // Node A (initiator) static keypair
    // SECURITY: bring-up test keys, NOT production custody
    let node_a_static = StaticKeypair::from_secret(derive_test_secret(0xD0, port_b));
    let node_a_eph_seed = derive_test_secret(0xE0, port_b);

    // Node B (responder) static keypair
    // SECURITY: bring-up test keys, NOT production custody
    let node_b_static = StaticKeypair::from_secret(derive_test_secret(0xD1, port_b));
    let node_b_eph_seed = derive_test_secret(0xE1, port_b);

    // Node B expected public key (for A to verify) MUST come from discovery mapping (RFC-0008 Phase 1b).
    let Some(peer_b) = peers.peek(node_b_device_id) else {
        let _ = nexus_abi::debug_println("dsoftbusd: discovery missing peer=node-b");
        loop {
            let _ = yield_();
        }
    };
    if peer_b.noise_static != node_b_static.public {
        // Identity binding mismatch: discovery mapping doesn't match the key material we are about to authenticate.
        let _ = nexus_abi::debug_println("dsoftbusd: identity mismatch peer=node-b");
        loop {
            let _ = yield_();
        }
    }
    let node_b_pub_expected = peer_b.noise_static;
    // Node A expected public key (for B to verify)
    let node_a_pub_expected = node_a_static.public;

    // Create initiator (A) and responder (B)
    let mut initiator = XkInitiator::new(node_a_static, node_b_pub_expected, node_a_eph_seed);
    let mut responder = XkResponder::new(node_b_static, node_a_pub_expected, node_b_eph_seed);

    // Helper to read from a stream
    fn dual_stream_read(net: &KernelClient, sid: u32, buf: &mut [u8]) -> core::result::Result<(), ()> {
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
                                break;
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

    // Helper to write to a stream
    fn dual_stream_write(net: &KernelClient, sid: u32, data: &[u8]) -> core::result::Result<(), ()> {
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

    // Noise XK handshake between dual nodes
    // Step 1: A writes msg1
    let mut msg1 = [0u8; MSG1_LEN];
    initiator.write_msg1(&mut msg1);
    if dual_stream_write(&net, sid_a, &msg1).is_err() {
        let _ = nexus_abi::debug_println("dsoftbusd: dual-node msg1 write FAIL");
        loop {
            let _ = yield_();
        }
    }

    // B reads msg1, writes msg2
    let mut msg1_recv = [0u8; MSG1_LEN];
    if dual_stream_read(&net, sid_b, &mut msg1_recv).is_err() {
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
    if dual_stream_write(&net, sid_b, &msg2).is_err() {
        let _ = nexus_abi::debug_println("dsoftbusd: dual-node msg2 write FAIL");
        loop {
            let _ = yield_();
        }
    }

    // A reads msg2, writes msg3
    let mut msg2_recv = [0u8; MSG2_LEN];
    if dual_stream_read(&net, sid_a, &mut msg2_recv).is_err() {
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
    if dual_stream_write(&net, sid_a, &msg3).is_err() {
        let _ = nexus_abi::debug_println("dsoftbusd: dual-node msg3 write FAIL");
        loop {
            let _ = yield_();
        }
    }

    // B reads msg3, finishes handshake
    let mut msg3_recv = [0u8; MSG3_LEN];
    if dual_stream_read(&net, sid_b, &mut msg3_recv).is_err() {
        let _ = nexus_abi::debug_println("dsoftbusd: dual-node msg3 read FAIL");
        loop {
            let _ = yield_();
        }
    }
    let transport_b = match responder.read_msg3_finish(&msg3_recv) {
        Ok(keys) => Transport::new(keys),
        Err(nexus_noise_xk::NoiseError::StaticKeyMismatch) => {
            // RFC-0008 Phase 1b: Identity binding enforcement - mismatch case
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

    // Suppress unused variable warnings
    let _ = transport_a;
    let _ = transport_b;

    // ============================================================
    // RFC-0008 Phase 1b: Identity binding enforcement
    // ============================================================
    // After the Noise XK handshake completes successfully, the responder (Node B)
    // has verified that the initiator (Node A) possesses the expected static key.
    // This proves the device_id <-> noise_static_pub binding.
    //
    // In a full implementation:
    // 1. Discovery announcements carry (device_id, noise_static_pub)
    // 2. This mapping is cached
    // 3. After handshake, the binding is verified against the cache
    //
    // For bring-up, the test keys are deterministic, so we verify the binding
    // implicitly through the successful handshake (StaticKeyMismatch would have
    // been raised if keys didn't match).
    let _ = nexus_abi::debug_println("dsoftbusd: identity bound peer=node-b");

    // Dual-node handshake complete!
    let _ = nexus_abi::debug_println("dsoftbusd: dual-node session ok");

    // Service readiness: only report ready once the facade is usable and dual-node bring-up proofs
    // completed. This keeps QEMU marker ordering deterministic and avoids “ready” being emitted
    // before the service can actually accept sessions.
    let _ = nexus_abi::debug_println("dsoftbusd: ready");

    // ============================================================
    // End dual-node mode
    // ============================================================

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
    // REAL Noise XK Handshake with selftest-client (RFC-0008)
    // ============================================================
    // (nexus_noise_xk types already imported and derive_test_secret defined above)

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
