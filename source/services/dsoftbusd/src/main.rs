#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std, no_main)]
// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: DSoftBus daemon entrypoint (os-lite)
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Proven via QEMU markers (TASK-0003..0005 / scripts/qemu-test.sh + tools/os2vm.sh)
//!
//! SECURITY INVARIANTS:
//! - No network capability transfer: remote proxy forwards bounded request/response bytes only.
//! - Remote proxy is deny-by-default (explicit allowlist).
//! - No secrets (keys/session material) are logged to UART.
//!
//! ADR: docs/adr/0005-dsoftbus-architecture.md

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
extern crate alloc;

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn os_entry() -> core::result::Result<(), ()> {
    use alloc::string::String;
    use alloc::vec::Vec;
    use nexus_abi::yield_;
    use nexus_discovery_packet::{decode_announce_v1, encode_announce_v1, AnnounceV1};
    use nexus_ipc::{IpcError as IpcErrorLite, KernelClient, Wait};
    use nexus_peer_lru::{PeerEntry, PeerLru};

    // dsoftbusd must NOT own MMIO; it uses netstackd's IPC facade.
    let net = loop {
        match KernelClient::new_for("netstackd") {
            Ok(c) => break c,
            Err(_) => {
                let _ = yield_();
            }
        }
    };

    const MAGIC0: u8 = b'N';
    const MAGIC1: u8 = b'S';
    const VERSION: u8 = 1;
    const OP_LISTEN: u8 = 1;
    const OP_ACCEPT: u8 = 2;
    const OP_READ: u8 = 4;
    const OP_WRITE: u8 = 5;
    const OP_UDP_BIND: u8 = 6;
    const OP_UDP_RECV_FROM: u8 = 8;
    const OP_LOCAL_ADDR: u8 = 10;
    const STATUS_OK: u8 = 0;
    const STATUS_NOT_FOUND: u8 = 1;
    const STATUS_MALFORMED: u8 = 2;
    const STATUS_WOULD_BLOCK: u8 = 3;
    const STATUS_IO: u8 = 4;

    fn rpc(net: &KernelClient, req: &[u8]) -> core::result::Result<[u8; 512], ()> {
        let reply = KernelClient::new_for("@reply").map_err(|_| {
            let _ = nexus_abi::debug_println("dsoftbusd: write reply route fail");
            ()
        })?;
        let (reply_send_slot, reply_recv_slot) = reply.slots();
        let reply_send_clone = nexus_abi::cap_clone(reply_send_slot).map_err(|_| {
            let _ = nexus_abi::debug_println("dsoftbusd: write cap clone fail");
            ()
        })?;
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

    fn get_local_ip(net: &KernelClient) -> Option<[u8; 4]> {
        let req = [MAGIC0, MAGIC1, VERSION, OP_LOCAL_ADDR];
        let rsp = rpc(net, &req).ok()?;
        if rsp[0] != MAGIC0
            || rsp[1] != MAGIC1
            || rsp[2] != VERSION
            || rsp[3] != (OP_LOCAL_ADDR | 0x80)
            || rsp[4] != STATUS_OK
        {
            return None;
        }
        Some([rsp[5], rsp[6], rsp[7], rsp[8]])
    }

    // Wait for netstackd to finish IPv4 configuration (DHCP or deterministic static fallback).
    // This avoids a race where early callers observe "no local addr" and then fail to bind sockets.
    let mut local_ip = [10, 0, 2, 15];
    for _ in 0..50_000 {
        if let Some(ip) = get_local_ip(&net) {
            local_ip = ip;
            break;
        }
        let _ = yield_();
    }
    let is_cross_vm = local_ip[0] == 10 && local_ip[1] == 42;
    if is_cross_vm {
        // Cross-VM mode (TASK-0005 / RFC-0010): real UDP datagrams + TCP sessions across two QEMU instances.
        // This path is opt-in via the 2-VM harness (socket/mcast backend) and MUST remain deterministic.
        cross_vm_main(&net, local_ip)?;
        return Ok(());
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
                noise_static: nexus_noise_xk::StaticKeypair::from_secret(derive_test_secret(
                    0xD1,
                    node_b_port,
                ))
                .public,
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

                // Single-VM bring-up note: some backends may not deliver multicast/broadcast reliably.
                // For deterministic local bring-up, we unicast a single announce to LOCAL_IP and then poll recv.
                send[8..12].copy_from_slice(&LOCAL_IP);
                let rsp = rpc(net, &send[..hdr_len + bytes.len()])?;
                Ok(rsp[0] == MAGIC0
                    && rsp[1] == MAGIC1
                    && rsp[2] == VERSION
                    && rsp[3] == (OP_UDP_SEND_TO | 0x80)
                    && rsp[4] == STATUS_OK)
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
        if rsp[0] == MAGIC0
            && rsp[1] == MAGIC1
            && rsp[2] == VERSION
            && rsp[3] == (OP_UDP_RECV_FROM | 0x80)
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

    use nexus_noise_xk::{
        StaticKeypair, Transport, XkInitiator, XkResponder, MSG1_LEN, MSG2_LEN, MSG3_LEN,
    };

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
    fn dual_stream_read(
        net: &KernelClient,
        sid: u32,
        buf: &mut [u8],
    ) -> core::result::Result<(), ()> {
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
    fn dual_stream_write(
        net: &KernelClient,
        sid: u32,
        data: &[u8],
    ) -> core::result::Result<(), ()> {
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
        let wait = Wait::Timeout(core::time::Duration::from_millis(100));
        let mut sent = false;
        for _ in 0..64 {
            match net.send_with_cap_move_wait(&w[..10 + data.len()], reply_send_clone, wait) {
                Ok(()) => {
                    sent = true;
                    break;
                }
                Err(IpcErrorLite::WouldBlock)
                | Err(IpcErrorLite::Timeout)
                | Err(IpcErrorLite::NoSpace) => {
                    let _ = nexus_abi::yield_();
                }
                Err(_) => {
                    let _ = nexus_abi::debug_println("dsoftbusd: write send fail");
                    return Err(());
                }
            }
        }
        if !sent {
            let _ = nexus_abi::debug_println("dsoftbusd: write send timeout");
            return Err(());
        }
        let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut rsp = [0u8; 64];
        let mut retry_logged = false;
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
                    {
                        if rsp[4] == STATUS_OK {
                            return Ok(());
                        }
                        if !retry_logged {
                            retry_logged = true;
                            let _ = match rsp[4] {
                                STATUS_NOT_FOUND => nexus_abi::debug_println("dsoftbusd: write status not-found"),
                                STATUS_MALFORMED => nexus_abi::debug_println("dsoftbusd: write status malformed"),
                                STATUS_WOULD_BLOCK => {
                                    nexus_abi::debug_println("dsoftbusd: write status would-block")
                                }
                                STATUS_IO => nexus_abi::debug_println("dsoftbusd: write status io"),
                                _ => nexus_abi::debug_println("dsoftbusd: write status unknown"),
                            };
                        }
                        let _ = nexus_abi::yield_();
                        continue;
                    }
                    // Ignore unrelated replies on the shared netstackd reply inbox.
                    let _ = nexus_abi::yield_();
                    continue;
                }
                Err(nexus_abi::IpcError::QueueEmpty) => {
                    let _ = nexus_abi::yield_();
                }
                Err(_) => return Err(()),
            }
        }
        let _ = nexus_abi::debug_println("dsoftbusd: write timeout");
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
    nexus_log::info("dsoftbusd", |line| {
        line.text("dsoftbusd: ready");
    });

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
        let wait = Wait::Timeout(core::time::Duration::from_millis(100));
        let mut sent = false;
        for _ in 0..64 {
            match net.send_with_cap_move_wait(&w[..10 + data.len()], reply_send_clone, wait) {
                Ok(()) => {
                    sent = true;
                    break;
                }
                Err(IpcErrorLite::WouldBlock)
                | Err(IpcErrorLite::Timeout)
                | Err(IpcErrorLite::NoSpace) => {
                    let _ = nexus_abi::yield_();
                }
                Err(_) => return Err(()),
            }
        }
        if !sent {
            return Err(());
        }
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
                    {
                        if rsp[4] == STATUS_OK {
                            return Ok(());
                        }
                        let _ = nexus_abi::yield_();
                        continue;
                    }
                    // Ignore unrelated replies on the shared netstackd reply inbox.
                    let _ = nexus_abi::yield_();
                    continue;
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

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
fn cross_vm_main(net: &nexus_ipc::KernelClient, local_ip: [u8; 4]) -> core::result::Result<(), ()> {
    use alloc::string::String;
    use alloc::vec::Vec;
    use nexus_abi::yield_;
    use nexus_discovery_packet::{decode_announce_v1, encode_announce_v1, AnnounceV1};
    use nexus_ipc::{Client as _, KernelClient, KernelServer, Server as _, Wait};
    use nexus_peer_lru::{PeerEntry, PeerLru};

    const MAGIC0: u8 = b'N';
    const MAGIC1: u8 = b'S';
    const VERSION: u8 = 1;
    const OP_LISTEN: u8 = 1;
    const OP_ACCEPT: u8 = 2;
    const OP_CONNECT: u8 = 3;
    const OP_READ: u8 = 4;
    const OP_WRITE: u8 = 5;
    const OP_UDP_BIND: u8 = 6;
    const OP_UDP_SEND_TO: u8 = 7;
    const OP_UDP_RECV_FROM: u8 = 8;
    const STATUS_OK: u8 = 0;
    const STATUS_WOULD_BLOCK: u8 = 3;
    const STATUS_IO: u8 = 4;

    const DISC_PORT: u16 = 37_020;
    const MCAST_IP: [u8; 4] = [239, 42, 0, 1];

    // Local IPC protocol (selftest-client -> dsoftbusd) for cross-VM proofs.
    const L0: u8 = b'D';
    const L1: u8 = b'S';
    const LVER: u8 = 1;
    const LOP_REMOTE_RESOLVE: u8 = 1;
    const LOP_REMOTE_BUNDLE_LIST: u8 = 2;
    const LOP_LOG_PROBE: u8 = 0x7f;
    const LSTATUS_OK: u8 = 0;
    const LSTATUS_FAIL: u8 = 1;

    // Remote gateway record sizes (fixed-size encrypted records; no plaintext framing on the wire).
    const TAGLEN: usize = 16;
    const MAX_REQ: usize = 256;
    const MAX_RSP: usize = 512;
    const REQ_PLAIN: usize = 1 + 2 + MAX_REQ;
    const RSP_PLAIN: usize = 1 + 2 + MAX_RSP;
    const REQ_CIPH: usize = REQ_PLAIN + TAGLEN;
    const RSP_CIPH: usize = RSP_PLAIN + TAGLEN;

    const SVC_SAMGR_RESOLVE_STATUS: u8 = 1;
    const SVC_BUNDLE_LIST: u8 = 2;

    // SECURITY: bring-up test keys, NOT production custody.
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

    fn rpc(net: &KernelClient, req: &[u8]) -> core::result::Result<[u8; 512], ()> {
        let reply = KernelClient::new_for("@reply").map_err(|_| ())?;
        let (reply_send_slot, reply_recv_slot) = reply.slots();
        let reply_send_clone = nexus_abi::cap_clone(reply_send_slot).map_err(|_| ())?;
        net.send_with_cap_move(req, reply_send_clone).map_err(|_| ())?;
        let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 512];
        for _ in 0..10_000 {
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

    fn stream_write_all(net: &KernelClient, sid: u32, data: &[u8]) -> core::result::Result<(), ()> {
        let mut off = 0usize;
        while off < data.len() {
            let chunk = core::cmp::min(480, data.len() - off);
            let mut w = [0u8; 512];
            w[0] = MAGIC0;
            w[1] = MAGIC1;
            w[2] = VERSION;
            w[3] = OP_WRITE;
            w[4..8].copy_from_slice(&sid.to_le_bytes());
            w[8..10].copy_from_slice(&(chunk as u16).to_le_bytes());
            w[10..10 + chunk].copy_from_slice(&data[off..off + chunk]);
            let rsp = rpc(net, &w[..10 + chunk])?;
            if rsp[0] != MAGIC0
                || rsp[1] != MAGIC1
                || rsp[2] != VERSION
                || rsp[3] != (OP_WRITE | 0x80)
            {
                return Err(());
            }
            if rsp[4] == STATUS_OK {
                let wrote = u16::from_le_bytes([rsp[5], rsp[6]]) as usize;
                if wrote == 0 {
                    return Err(());
                }
                off = off.saturating_add(wrote);
                continue;
            }
            if rsp[4] == STATUS_WOULD_BLOCK {
                let _ = yield_();
                continue;
            }
            return Err(());
        }
        Ok(())
    }

    fn stream_read_exact(
        net: &KernelClient,
        sid: u32,
        out: &mut [u8],
    ) -> core::result::Result<(), ()> {
        let mut off = 0usize;
        while off < out.len() {
            let want = core::cmp::min(460, out.len() - off); // must match netstackd recv cap
            let mut r = [0u8; 10];
            r[0] = MAGIC0;
            r[1] = MAGIC1;
            r[2] = VERSION;
            r[3] = OP_READ;
            r[4..8].copy_from_slice(&sid.to_le_bytes());
            r[8..10].copy_from_slice(&(want as u16).to_le_bytes());
            let rsp = rpc(net, &r)?;
            if rsp[0] != MAGIC0
                || rsp[1] != MAGIC1
                || rsp[2] != VERSION
                || rsp[3] != (OP_READ | 0x80)
            {
                return Err(());
            }
            if rsp[4] == STATUS_OK {
                let n = u16::from_le_bytes([rsp[5], rsp[6]]) as usize;
                if n == 0 || 7 + n > rsp.len() {
                    return Err(());
                }
                out[off..off + n].copy_from_slice(&rsp[7..7 + n]);
                off += n;
                continue;
            }
            if rsp[4] == STATUS_WOULD_BLOCK {
                let _ = yield_();
                continue;
            }
            return Err(());
        }
        Ok(())
    }

    fn udp_bind(net: &KernelClient, ip: [u8; 4], port: u16) -> core::result::Result<u32, ()> {
        // OP_UDP_BIND v2: [magic,ver,op, ip[4], port:u16le]
        let req = [
            MAGIC0,
            MAGIC1,
            VERSION,
            OP_UDP_BIND,
            ip[0],
            ip[1],
            ip[2],
            ip[3],
            (port & 0xff) as u8,
            (port >> 8) as u8,
        ];
        let rsp = rpc(net, &req).map_err(|_| ())?;
        if rsp[0] != MAGIC0
            || rsp[1] != MAGIC1
            || rsp[2] != VERSION
            || rsp[3] != (OP_UDP_BIND | 0x80)
        {
            return Err(());
        }
        if rsp[4] != STATUS_OK {
            return Err(());
        }
        Ok(u32::from_le_bytes([rsp[5], rsp[6], rsp[7], rsp[8]]))
    }

    fn udp_send_to(
        net: &KernelClient,
        udp_id: u32,
        ip: [u8; 4],
        port: u16,
        payload: &[u8],
    ) -> core::result::Result<(), ()> {
        if payload.len() > 256 {
            return Err(());
        }
        let mut send = [0u8; 16 + 256];
        send[0] = MAGIC0;
        send[1] = MAGIC1;
        send[2] = VERSION;
        send[3] = OP_UDP_SEND_TO;
        send[4..8].copy_from_slice(&udp_id.to_le_bytes());
        send[8..12].copy_from_slice(&ip);
        send[12..14].copy_from_slice(&port.to_le_bytes());
        send[14..16].copy_from_slice(&(payload.len() as u16).to_le_bytes());
        send[16..16 + payload.len()].copy_from_slice(payload);
        let rsp = rpc(net, &send[..16 + payload.len()])?;
        if rsp[0] == MAGIC0
            && rsp[1] == MAGIC1
            && rsp[2] == VERSION
            && rsp[3] == (OP_UDP_SEND_TO | 0x80)
        {
            if rsp[4] == STATUS_OK || rsp[4] == STATUS_WOULD_BLOCK {
                return Ok(());
            }
        }
        Err(())
    }

    fn tcp_listen(net: &KernelClient, port: u16) -> core::result::Result<u32, ()> {
        let req = [MAGIC0, MAGIC1, VERSION, OP_LISTEN, (port & 0xff) as u8, (port >> 8) as u8];
        let rsp = rpc(net, &req)?;
        if rsp[0] != MAGIC0 || rsp[1] != MAGIC1 || rsp[2] != VERSION || rsp[3] != (OP_LISTEN | 0x80)
        {
            return Err(());
        }
        if rsp[4] != STATUS_OK {
            return Err(());
        }
        Ok(u32::from_le_bytes([rsp[5], rsp[6], rsp[7], rsp[8]]))
    }

    fn tcp_connect(net: &KernelClient, ip: [u8; 4], port: u16) -> core::result::Result<u32, ()> {
        let mut c = [0u8; 10];
        c[0] = MAGIC0;
        c[1] = MAGIC1;
        c[2] = VERSION;
        c[3] = OP_CONNECT;
        c[4..8].copy_from_slice(&ip);
        c[8..10].copy_from_slice(&port.to_le_bytes());
        let rsp = rpc(net, &c)?;
        if rsp[0] == MAGIC0
            && rsp[1] == MAGIC1
            && rsp[2] == VERSION
            && rsp[3] == (OP_CONNECT | 0x80)
        {
            if rsp[4] == STATUS_OK {
                return Ok(u32::from_le_bytes([rsp[5], rsp[6], rsp[7], rsp[8]]));
            }
            // WOULD_BLOCK is expected during connect establishment; caller retries.
            if rsp[4] == STATUS_WOULD_BLOCK {
                return Err(());
            }
        }
        Err(())
    }

    fn tcp_accept(net: &KernelClient, lid: u32) -> core::result::Result<u32, ()> {
        let mut a = [0u8; 8];
        a[0] = MAGIC0;
        a[1] = MAGIC1;
        a[2] = VERSION;
        a[3] = OP_ACCEPT;
        a[4..8].copy_from_slice(&lid.to_le_bytes());
        let rsp = rpc(net, &a)?;
        if rsp[0] == MAGIC0 && rsp[1] == MAGIC1 && rsp[2] == VERSION && rsp[3] == (OP_ACCEPT | 0x80)
        {
            if rsp[4] == STATUS_OK {
                return Ok(u32::from_le_bytes([rsp[5], rsp[6], rsp[7], rsp[8]]));
            }
            // WOULD_BLOCK is expected until a peer connects; caller retries.
            if rsp[4] == STATUS_WOULD_BLOCK {
                return Err(());
            }
        }
        Err(())
    }

    // Determine deterministic node identity from local IP (10.42.0.<mac_lsb>), matching tools/os2vm.sh MACs.
    let (device_id, listen_port, peer_ip, peer_port, peer_device_id, key_tag_self, key_tag_peer) =
        if local_ip == [10, 42, 0, 10] {
            ("node-a", 34_567u16, [10, 42, 0, 11], 34_568u16, "node-b", 0xD0u8, 0xD1u8)
        } else {
            ("node-b", 34_568u16, [10, 42, 0, 10], 34_567u16, "node-a", 0xD1u8, 0xD0u8)
        };

    // Bind discovery UDP and start RX loop.
    let udp_id = {
        let mut out: Option<u32> = None;
        for _ in 0..50_000 {
            if let Ok(id) = udp_bind(net, local_ip, DISC_PORT) {
                out = Some(id);
                break;
            }
            let _ = yield_();
        }
        out.ok_or(())?
    };
    let _ = nexus_abi::debug_println("dsoftbusd: discovery cross-vm up");

    // Peer cache (bounded).
    let mut peers = PeerLru::with_default_capacity();
    let mut peer_ips: Vec<(String, [u8; 4])> = Vec::new();

    fn set_peer_ip(ips: &mut Vec<(String, [u8; 4])>, id: &str, ip: [u8; 4]) {
        if let Some(pos) = ips.iter().position(|(x, _)| x == id) {
            ips[pos].1 = ip;
        } else {
            ips.push((String::from(id), ip));
        }
    }
    fn get_peer_ip(ips: &[(String, [u8; 4])], id: &str) -> Option<[u8; 4]> {
        ips.iter().find(|(x, _)| x == id).map(|(_, ip)| *ip)
    }

    // Periodically announce ourselves until we observe the peer.
    // Keep this bounded-per-iteration and cooperative; do not exit on transient network bring-up races.
    // Listen for the session port early so the acceptor is ready even if discovery arrives later.
    let lid = tcp_listen(net, listen_port)?;

    // Establish a single deterministic session: node-a initiates, node-b accepts.
    let is_initiator = device_id == "node-a";
    let mut sid: Option<u32> = None;
    let mut announced_once = false;
    let mut announce_send_failed = false;
    let mut udp_recv_failed = false;
    let mut dial_logged = false;
    let mut accept_logged = false;

    // Single cooperative loop:
    // - continuously announces + receives discovery packets
    // - initiator dials once peer mapping is known
    // - acceptor accepts as soon as the peer dials (no need to pre-learn peer ip)
    // - both wait until the peer entry exists before starting Noise (identity binding)
    loop {
        // Ensure we always send at least one announce before trying to establish sessions.
        // Then rate-limit to once per 64 ticks for determinism.
        let now = (nexus_abi::nsec().unwrap_or(0) / 1_000_000) as u64;
        if !announced_once || (now & 0x3f) == 0 {
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
                let ok1 = udp_send_to(net, udp_id, MCAST_IP, DISC_PORT, &bytes).is_ok();
                let ok2 = udp_send_to(net, udp_id, peer_ip, DISC_PORT, &bytes).is_ok();
                if !(ok1 && ok2) && !announce_send_failed {
                    announce_send_failed = true;
                }
                if !announced_once {
                    let _ = nexus_abi::debug_println("dsoftbusd: discovery announce sent");
                    announced_once = true;
                }
            }
        }

        // Try receive one announce.
        let mut r = [0u8; 10];
        r[0] = MAGIC0;
        r[1] = MAGIC1;
        r[2] = VERSION;
        r[3] = OP_UDP_RECV_FROM;
        r[4..8].copy_from_slice(&udp_id.to_le_bytes());
        r[8..10].copy_from_slice(&(256u16).to_le_bytes());
        let rsp = rpc(net, &r)?;
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
                                let _ =
                                    nexus_abi::debug_println("dsoftbusd: discovery peer learned");
                            }
                        }
                        Err(_) => {
                            let _ =
                                nexus_abi::debug_println("dsoftbusd: announce ignored (malformed)");
                        }
                    }
                }
            } else if rsp[4] == STATUS_IO && !udp_recv_failed {
                let _ = nexus_abi::debug_println("dsoftbusd: discovery recv FAIL");
                udp_recv_failed = true;
            }
        }

        // Session establishment.
        if sid.is_none() {
            if is_initiator {
                let Some(peer) = peers.peek(peer_device_id) else {
                    let _ = yield_();
                    continue;
                };
                let Some(ip) = get_peer_ip(&peer_ips, peer_device_id) else {
                    let _ = yield_();
                    continue;
                };
                if !dial_logged {
                    let _ = nexus_abi::debug_println("dsoftbusd: cross-vm dial start");
                    dial_logged = true;
                }
                if let Ok(s) = tcp_connect(net, ip, peer.port) {
                    sid = Some(s);
                }
            } else {
                if !accept_logged {
                    let _ = nexus_abi::debug_println("dsoftbusd: cross-vm accept wait");
                    accept_logged = true;
                }
                if let Ok(s) = tcp_accept(net, lid) {
                    sid = Some(s);
                }
            }
        }

        // Before starting Noise, ensure we have the peer's discovery entry for identity binding.
        if sid.is_some() && peers.peek(peer_device_id).is_some() {
            break;
        }

        let _ = yield_();
    }

    let sid = sid.ok_or(())?;

    // Noise handshake (XK): initiator <-> responder
    use nexus_noise_xk::{
        StaticKeypair, Transport, XkInitiator, XkResponder, MSG1_LEN, MSG2_LEN, MSG3_LEN,
    };

    // SECURITY: bring-up test keys, NOT production custody
    let self_static = StaticKeypair::from_secret(derive_test_secret(key_tag_self, listen_port));
    // SECURITY: bring-up test keys, NOT production custody
    let self_eph_seed = derive_test_secret(0xE0, listen_port);
    let peer_expected_pub =
        StaticKeypair::from_secret(derive_test_secret(key_tag_peer, peer_port)).public;

    // Enforce identity binding: expected pub key MUST match discovery mapping.
    let Some(peer_entry) = peers.peek(peer_device_id) else {
        return Err(());
    };
    if peer_entry.noise_static != peer_expected_pub {
        let _ = nexus_abi::debug_println("dsoftbusd: identity mismatch peer=crossvm");
        return Err(());
    }

    let mut transport = if is_initiator {
        let mut initiator = XkInitiator::new(self_static, peer_expected_pub, self_eph_seed);
        let mut msg1 = [0u8; MSG1_LEN];
        initiator.write_msg1(&mut msg1);
        stream_write_all(net, sid, &msg1)?;

        let mut msg2 = [0u8; MSG2_LEN];
        stream_read_exact(net, sid, &mut msg2)?;

        let mut msg3 = [0u8; MSG3_LEN];
        let keys = initiator.read_msg2_write_msg3(&msg2, &mut msg3).map_err(|_| ())?;
        stream_write_all(net, sid, &msg3)?;
        Transport::new(keys)
    } else {
        let mut responder = XkResponder::new(self_static, peer_expected_pub, self_eph_seed);
        let mut msg1 = [0u8; MSG1_LEN];
        stream_read_exact(net, sid, &mut msg1)?;
        let mut msg2 = [0u8; MSG2_LEN];
        responder.read_msg1_write_msg2(&msg1, &mut msg2).map_err(|_| ())?;
        stream_write_all(net, sid, &msg2)?;
        let mut msg3 = [0u8; MSG3_LEN];
        stream_read_exact(net, sid, &mut msg3)?;
        let keys = responder.read_msg3_finish(&msg3).map_err(|_| ())?;
        Transport::new(keys)
    };

    // Session established.
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

    // Node B: remote gateway server loop (serve requests from node-a).
    if !is_initiator {
        let samgrd = loop {
            match KernelClient::new_for("samgrd") {
                Ok(c) => break c,
                Err(_) => {
                    let _ = yield_();
                }
            }
        };
        let bundlemgrd = loop {
            match KernelClient::new_for("bundlemgrd") {
                Ok(c) => break c,
                Err(_) => {
                    let _ = yield_();
                }
            }
        };
        // Reply inbox for CAP_MOVE request/reply to local services.
        let reply = loop {
            match KernelClient::new_for("@reply") {
                Ok(c) => break c,
                Err(_) => {
                    let _ = yield_();
                }
            }
        };
        let (reply_send_slot, _reply_recv_slot) = reply.slots();
        let _ = nexus_abi::debug_println("dsoftbusd: remote proxy up");
        let mut rx_logged = false;
        loop {
            let mut ciph = [0u8; REQ_CIPH];
            stream_read_exact(net, sid, &mut ciph)?;
            if !rx_logged {
                let _ = nexus_abi::debug_println("dsoftbusd: remote proxy rx");
                rx_logged = true;
            }
            let mut plain = [0u8; REQ_PLAIN];
            let n = transport.decrypt(&ciph, &mut plain).map_err(|_| ())?;
            if n != REQ_PLAIN {
                let _ = nexus_abi::debug_println("dsoftbusd: remote proxy denied (malformed)");
                continue;
            }
            let svc = plain[0];
            let used = u16::from_le_bytes([plain[1], plain[2]]) as usize;
            if used > MAX_REQ {
                let _ = nexus_abi::debug_println("dsoftbusd: remote proxy denied (oversized)");
                continue;
            }
            let req = &plain[3..3 + used];

            let mut status = 0u8;
            let mut rsp_payload: Vec<u8> = Vec::new();
            match svc {
                SVC_SAMGR_RESOLVE_STATUS => {
                    if req.len() < 5 || req[0] != b'S' || req[1] != b'M' || req[2] != 1 {
                        status = 1;
                    } else {
                        // CAP_MOVE reply: move a cloned reply SEND cap so samgrd can respond on it.
                        let cap = nexus_abi::cap_clone(reply_send_slot).map_err(|_| ())?;
                        samgrd
                            .send_with_cap_move_wait(
                                req,
                                cap,
                                Wait::Timeout(core::time::Duration::from_millis(300)),
                            )
                            .map_err(|_| ())?;
                        let rsp = reply
                            .recv(Wait::Timeout(core::time::Duration::from_millis(300)))
                            .map_err(|_| ())?;
                        rsp_payload.extend_from_slice(&rsp);
                        let _ = nexus_abi::debug_println(
                            "dsoftbusd: remote proxy ok (peer=node-a service=samgrd)",
                        );
                    }
                }
                SVC_BUNDLE_LIST => {
                    let cap = nexus_abi::cap_clone(reply_send_slot).map_err(|_| ())?;
                    bundlemgrd
                        .send_with_cap_move_wait(
                            req,
                            cap,
                            Wait::Timeout(core::time::Duration::from_millis(300)),
                        )
                        .map_err(|_| ())?;
                    let rsp = reply
                        .recv(Wait::Timeout(core::time::Duration::from_millis(300)))
                        .map_err(|_| ())?;
                    rsp_payload.extend_from_slice(&rsp);
                    let _ = nexus_abi::debug_println(
                        "dsoftbusd: remote proxy ok (peer=node-a service=bundlemgrd)",
                    );
                }
                _ => {
                    status = 1;
                    let _ = nexus_abi::debug_println(
                        "dsoftbusd: remote proxy denied (service=unknown)",
                    );
                }
            }

            // Build fixed-size response record.
            let mut rsp_plain = [0u8; RSP_PLAIN];
            rsp_plain[0] = status;
            let len = core::cmp::min(rsp_payload.len(), MAX_RSP);
            rsp_plain[1..3].copy_from_slice(&(len as u16).to_le_bytes());
            rsp_plain[3..3 + len].copy_from_slice(&rsp_payload[..len]);

            let mut rsp_ciph = [0u8; RSP_CIPH];
            let n = transport.encrypt(&rsp_plain, &mut rsp_ciph).map_err(|_| ())?;
            if n != RSP_CIPH {
                return Err(());
            }
            stream_write_all(net, sid, &rsp_ciph)?;
        }
    }

    // Node A: local IPC server loop (selftest-client drives remote resolve/query).
    let server = loop {
        match KernelServer::new_for("dsoftbusd") {
            Ok(s) => break s,
            Err(_) => {
                let _ = yield_();
            }
        }
    };
    let mut ipc_logged = false;
    loop {
        // Use the plain request/response channel semantics (`Client::send`/`Client::recv`),
        // not the cap-move reply-token style.
        let frame = match server.recv(Wait::Blocking) {
            Ok(x) => x,
            Err(_) => {
                let _ = yield_();
                continue;
            }
        };
        if !ipc_logged {
            ipc_logged = true;
        }

        let mut out: Vec<u8> = Vec::new();
        if frame.len() < 4 || frame[0] != L0 || frame[1] != L1 || frame[2] != LVER {
            out.extend_from_slice(&[L0, L1, LVER, 0x80, LSTATUS_FAIL]);
        } else {
            match frame[3] {
                LOP_LOG_PROBE => {
                    let ok =
                        append_probe_to_logd(b"dsoftbusd", b"core service log probe: dsoftbusd");
                    out.extend_from_slice(&[
                        L0,
                        L1,
                        LVER,
                        LOP_LOG_PROBE | 0x80,
                        if ok { LSTATUS_OK } else { LSTATUS_FAIL },
                    ]);
                }
                LOP_REMOTE_RESOLVE => {
                    if frame.len() < 5 {
                        out.extend_from_slice(&[
                            L0,
                            L1,
                            LVER,
                            LOP_REMOTE_RESOLVE | 0x80,
                            LSTATUS_FAIL,
                        ]);
                    } else {
                        let n = frame[4] as usize;
                        if n == 0 || frame.len() != 5 + n {
                            out.extend_from_slice(&[
                                L0,
                                L1,
                                LVER,
                                LOP_REMOTE_RESOLVE | 0x80,
                                LSTATUS_FAIL,
                            ]);
                        } else {
                            // Build samgrd resolve-status request frame.
                            let mut req = Vec::with_capacity(5 + n);
                            req.push(b'S');
                            req.push(b'M');
                            req.push(1);
                            req.push(6); // OP_RESOLVE_STATUS
                            req.push(n as u8);
                            req.extend_from_slice(&frame[5..]);

                            // Send remote gateway request.
                            let mut plain = [0u8; REQ_PLAIN];
                            plain[0] = SVC_SAMGR_RESOLVE_STATUS;
                            let used = core::cmp::min(req.len(), MAX_REQ);
                            plain[1..3].copy_from_slice(&(used as u16).to_le_bytes());
                            plain[3..3 + used].copy_from_slice(&req[..used]);
                            let mut ciph = [0u8; REQ_CIPH];
                            let n = transport.encrypt(&plain, &mut ciph).map_err(|_| ())?;
                            if n != REQ_CIPH {
                                return Err(());
                            }
                            stream_write_all(net, sid, &ciph)?;

                            let mut rsp_ciph = [0u8; RSP_CIPH];
                            stream_read_exact(net, sid, &mut rsp_ciph)?;
                            let mut rsp_plain = [0u8; RSP_PLAIN];
                            let n = transport.decrypt(&rsp_ciph, &mut rsp_plain).map_err(|_| ())?;
                            if n != RSP_PLAIN {
                                return Err(());
                            }
                            let st = rsp_plain[0];
                            let len = u16::from_le_bytes([rsp_plain[1], rsp_plain[2]]) as usize;
                            let mut ok = false;
                            if st == 0 && len >= 13 {
                                let p = &rsp_plain[3..3 + len];
                                ok = p[0] == b'S'
                                    && p[1] == b'M'
                                    && p[2] == 1
                                    && p[3] == (6 | 0x80)
                                    && p[4] == 0;
                            }

                            out.extend_from_slice(&[
                                L0,
                                L1,
                                LVER,
                                LOP_REMOTE_RESOLVE | 0x80,
                                if ok { LSTATUS_OK } else { LSTATUS_FAIL },
                            ]);
                        }
                    }
                }
                LOP_REMOTE_BUNDLE_LIST => {
                    // bundlemgrd list request: [B,N,1,OP_LIST]
                    let req = [b'B', b'N', 1, nexus_abi::bundlemgrd::OP_LIST];
                    let mut plain = [0u8; REQ_PLAIN];
                    plain[0] = SVC_BUNDLE_LIST;
                    plain[1..3].copy_from_slice(&(req.len() as u16).to_le_bytes());
                    plain[3..3 + req.len()].copy_from_slice(&req);
                    let mut ciph = [0u8; REQ_CIPH];
                    let n = transport.encrypt(&plain, &mut ciph).map_err(|_| ())?;
                    if n != REQ_CIPH {
                        return Err(());
                    }
                    stream_write_all(net, sid, &ciph)?;

                    let mut rsp_ciph = [0u8; RSP_CIPH];
                    stream_read_exact(net, sid, &mut rsp_ciph)?;
                    let mut rsp_plain = [0u8; RSP_PLAIN];
                    let n = transport.decrypt(&rsp_ciph, &mut rsp_plain).map_err(|_| ())?;
                    if n != RSP_PLAIN {
                        return Err(());
                    }
                    let st = rsp_plain[0];
                    let len = u16::from_le_bytes([rsp_plain[1], rsp_plain[2]]) as usize;
                    let mut ok = false;
                    let mut count: u16 = 0;
                    if st == 0 && len >= 8 {
                        let p = &rsp_plain[3..3 + len];
                        if p[0] == b'B'
                            && p[1] == b'N'
                            && p[2] == 1
                            && p[3] == (nexus_abi::bundlemgrd::OP_LIST | 0x80)
                            && p[4] == 0
                        {
                            count = u16::from_le_bytes([p[5], p[6]]);
                            ok = true;
                        }
                    }
                    out.extend_from_slice(&[
                        L0,
                        L1,
                        LVER,
                        LOP_REMOTE_BUNDLE_LIST | 0x80,
                        if ok { LSTATUS_OK } else { LSTATUS_FAIL },
                    ]);
                    out.extend_from_slice(&count.to_le_bytes());
                }
                _ => {
                    out.extend_from_slice(&[L0, L1, LVER, (frame[3] | 0x80), LSTATUS_FAIL]);
                }
            }
        }

        let _ = server.send(&out, Wait::Blocking);
    }
}

#[cfg(all(target_os = "none", target_arch = "riscv64"))]
fn append_probe_to_logd(scope: &[u8], msg: &[u8]) -> bool {
    use nexus_ipc::{Client as _, KernelClient, Wait};

    const MAGIC0: u8 = b'L';
    const MAGIC1: u8 = b'O';
    const VERSION: u8 = 1;
    const OP_APPEND: u8 = 1;
    const LEVEL_INFO: u8 = 2;

    if scope.is_empty() || scope.len() > 64 || msg.is_empty() || msg.len() > 256 {
        return false;
    }

    let logd = match KernelClient::new_for("logd") {
        Ok(c) => c,
        Err(_) => return false,
    };
    let reply = match KernelClient::new_for("@reply") {
        Ok(c) => c,
        Err(_) => return false,
    };
    let (reply_send_slot, reply_recv_slot) = reply.slots();
    let moved = match nexus_abi::cap_clone(reply_send_slot) {
        Ok(slot) => slot,
        Err(_) => return false,
    };
    let mut frame = alloc::vec::Vec::with_capacity(4 + 1 + 1 + 2 + 2 + scope.len() + msg.len());
    frame.extend_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_APPEND]);
    frame.push(LEVEL_INFO);
    frame.push(scope.len() as u8);
    frame.extend_from_slice(&(msg.len() as u16).to_le_bytes());
    frame.extend_from_slice(&0u16.to_le_bytes()); // fields_len
    frame.extend_from_slice(scope);
    frame.extend_from_slice(msg);

    // Drain a few stale replies to keep the inbox bounded.
    {
        let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 32];
        for _ in 0..4 {
            match nexus_abi::ipc_recv_v1(
                reply_recv_slot,
                &mut hdr,
                &mut buf,
                nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
                0,
            ) {
                Ok(_) => {}
                Err(nexus_abi::IpcError::QueueEmpty) => break,
                Err(_) => break,
            }
        }
    }

    // Use CAP_MOVE so the logd response does not pollute selftest-client's logd recv queue.
    logd.send_with_cap_move_wait(&frame, moved, Wait::NonBlocking).is_ok()
}

#[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
fn main() -> ! {
    dsoftbus::run();
    loop {
        core::hint::spin_loop();
    }
}
