#![cfg_attr(
    all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"),
    no_std,
    no_main
)]

//! CONTEXT: netstackd (v0) — networking owner service for OS bring-up
//! OWNERS: @runtime
//! STATUS: Bring-up
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Proven via QEMU markers (TASK-0003 / scripts/qemu-test.sh)
//!
//! Responsibilities (v0, Step 1):
//! - Own virtio-net + smoltcp via `userspace/nexus-net-os`.
//! - Prove the facade can do real on-wire traffic (gateway ping + UDP DNS).
//! - Export a minimal sockets facade via IPC for other services (TASK-0003).

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
extern crate alloc;

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
nexus_service_entry::declare_entry!(os_entry);

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
fn os_entry() -> core::result::Result<(), ()> {
    use nexus_abi::yield_;
    use nexus_ipc::KernelServer;
    use nexus_net::{
        NetSocketAddrV4, NetStack as _, TcpListener as _, TcpStream as _, UdpSocket as _,
    };
    use nexus_net_os::{DhcpConfig, OsTcpListener, OsTcpStream, SmoltcpVirtioNetStack};

    use alloc::vec::Vec;

    // Helper to emit DHCP bound marker: "net: dhcp bound <ip>/<prefix> gw=<gw>"
    fn emit_dhcp_bound_marker(config: &DhcpConfig) {
        let mut buf = [0u8; 64];
        let mut pos = 0;
        // "net: dhcp bound "
        let prefix = b"net: dhcp bound ";
        buf[pos..pos + prefix.len()].copy_from_slice(prefix);
        pos += prefix.len();
        // IP address
        pos += write_ip(&config.ip, &mut buf[pos..]);
        // "/"
        buf[pos] = b'/';
        pos += 1;
        // prefix length
        pos += write_u8(config.prefix_len, &mut buf[pos..]);
        // " gw="
        let gw_prefix = b" gw=";
        buf[pos..pos + gw_prefix.len()].copy_from_slice(gw_prefix);
        pos += gw_prefix.len();
        // gateway (or "none")
        if let Some(gw) = config.gateway {
            pos += write_ip(&gw, &mut buf[pos..]);
        } else {
            let none = b"none";
            buf[pos..pos + none.len()].copy_from_slice(none);
            pos += none.len();
        }
        // Null-terminate and emit
        if let Ok(s) = core::str::from_utf8(&buf[..pos]) {
            let _ = nexus_abi::debug_println(s);
        }
    }

    // Helper to emit smoltcp iface marker with actual IP
    fn emit_smoltcp_iface_marker(config: &DhcpConfig) {
        let mut buf = [0u8; 48];
        let mut pos = 0;
        let prefix = b"net: smoltcp iface up ";
        buf[pos..pos + prefix.len()].copy_from_slice(prefix);
        pos += prefix.len();
        pos += write_ip(&config.ip, &mut buf[pos..]);
        if let Ok(s) = core::str::from_utf8(&buf[..pos]) {
            let _ = nexus_abi::debug_println(s);
        }
    }

    // Write IP address as decimal dotted string, return bytes written
    fn write_ip(ip: &[u8; 4], out: &mut [u8]) -> usize {
        let mut pos = 0;
        for (i, octet) in ip.iter().enumerate() {
            if i > 0 {
                out[pos] = b'.';
                pos += 1;
            }
            pos += write_u8(*octet, &mut out[pos..]);
        }
        pos
    }

    // Write u8 as decimal string, return bytes written
    fn write_u8(val: u8, out: &mut [u8]) -> usize {
        if val >= 100 {
            out[0] = b'0' + (val / 100);
            out[1] = b'0' + ((val / 10) % 10);
            out[2] = b'0' + (val % 10);
            3
        } else if val >= 10 {
            out[0] = b'0' + (val / 10);
            out[1] = b'0' + (val % 10);
            2
        } else {
            out[0] = b'0' + val;
            1
        }
    }

    let _ = nexus_abi::debug_println("netstackd: ready");

    // Bring up networking owner stack.
    let mut net = match SmoltcpVirtioNetStack::new_default() {
        Ok(n) => n,
        Err(_) => {
            let _ = nexus_abi::debug_println("netstackd: net FAIL");
            loop {
                let _ = yield_();
            }
        }
    };

    let _ = nexus_abi::debug_println("net: virtio-net up");
    let _ = nexus_abi::debug_println("SELFTEST: net iface ok");

    // DHCP lease acquisition loop (bounded, TASK-0004 Step 1)
    let start_ms = (nexus_abi::nsec().unwrap_or(0) / 1_000_000) as u64;
    let mut dhcp_bound = false;
    for i in 0..8000u64 {
        let now = start_ms + i;
        net.poll(now);

        if let Some(config) = net.dhcp_poll() {
            // Emit DHCP bound marker: net: dhcp bound <ip>/<mask> gw=<gw>
            emit_dhcp_bound_marker(&config);
            dhcp_bound = true;
            break;
        }

        if (i & 0x3f) == 0 {
            let _ = yield_();
        }
    }
    if !dhcp_bound {
        let _ = nexus_abi::debug_println("netstackd: dhcp FAIL");
        loop {
            let _ = yield_();
        }
    }

    // Emit iface up marker with DHCP-assigned IP
    if let Some(config) = net.get_dhcp_config() {
        emit_smoltcp_iface_marker(&config);
    }

    // Prove real L3 (gateway ping) — bounded.
    let ping_start = (nexus_abi::nsec().unwrap_or(0) / 1_000_000) as u64;
    if net.probe_ping_gateway(ping_start, 4000).is_err() {
        let _ = nexus_abi::debug_println("netstackd: net ping FAIL");
        loop {
            let _ = yield_();
        }
    }
    let _ = nexus_abi::debug_println("SELFTEST: net ping ok");

    // Prove UDP send+recv (DNS on QEMU usernet). This is the same proof shape as selftest-client,
    // but executed by the owner service (ensures MMIO capability distribution is correct).
    let dns = NetSocketAddrV4::new([10, 0, 2, 3], 53);
    let mut sock = match net.udp_bind(NetSocketAddrV4::new([10, 0, 2, 15], 40_000)) {
        Ok(s) => s,
        Err(_) => {
            let _ = nexus_abi::debug_println("netstackd: udp bind FAIL");
            loop {
                let _ = yield_();
            }
        }
    };

    // Minimal DNS query for A example.com (RFC1035).
    let mut q = [0u8; 32];
    q[0] = 0x12;
    q[1] = 0x34; // id
    q[2] = 0x01;
    q[3] = 0x00; // flags: recursion desired
    q[4] = 0x00;
    q[5] = 0x01; // qdcount
                 // qname: 7 'example' 3 'com' 0
    let mut p = 12usize;
    q[p] = 7;
    p += 1;
    q[p..p + 7].copy_from_slice(b"example");
    p += 7;
    q[p] = 3;
    p += 1;
    q[p..p + 3].copy_from_slice(b"com");
    p += 3;
    q[p] = 0;
    p += 1;
    // qtype A, qclass IN
    q[p] = 0;
    q[p + 1] = 1;
    q[p + 2] = 0;
    q[p + 3] = 1;
    p += 4;

    let mut ok = false;
    for i in 0..8000u64 {
        let now = start_ms + i;
        net.poll(now);
        let _ = sock.send_to(&q[..p], dns);
        let mut buf = [0u8; 512];
        if let Ok((n, _from)) = sock.recv_from(&mut buf) {
            if n >= 12 && buf[0] == 0x12 && buf[1] == 0x34 {
                ok = true;
                break;
            }
        }
        if (i & 0x3f) == 0 {
            let _ = yield_();
        }
    }
    if !ok {
        let _ = nexus_abi::debug_println("netstackd: udp dns FAIL");
        loop {
            let _ = yield_();
        }
    }
    let _ = nexus_abi::debug_println("SELFTEST: net udp dns ok");

    // TCP facade smoke: listen must succeed.
    if net.tcp_listen(NetSocketAddrV4::new([10, 0, 2, 15], 41_000), 1).is_ok() {
        let _ = nexus_abi::debug_println("SELFTEST: net tcp listen ok");
    } else {
        let _ = nexus_abi::debug_println("netstackd: tcp listen FAIL");
        loop {
            let _ = yield_();
        }
    }

    let _ = nexus_abi::debug_println("netstackd: facade up");

    // IPC v0: minimal sockets proxy for OS bring-up.
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
    const OP_ICMP_PING: u8 = 9;

    const STATUS_OK: u8 = 0;
    const STATUS_NOT_FOUND: u8 = 1;
    const STATUS_MALFORMED: u8 = 2;
    const STATUS_WOULD_BLOCK: u8 = 3;
    const STATUS_IO: u8 = 4;
    const STATUS_TIMED_OUT: u8 = 5;

    // Local-only loopback for DSoftBus bring-up (QEMU usernet has no TCP self-loopback).
    const LOOPBACK_PORT: u16 = 34_567;
    const LOOPBACK_PORT_B: u16 = 34_568; // Dual-node mode: second node port
    // Local-only UDP loopback port for discovery bring-up.
    const LOOPBACK_UDP_PORT: u16 = 37_020;

    #[derive(Clone, Copy)]
    struct LoopBuf {
        buf: [u8; 128],
        r: usize,
        w: usize,
        len: usize,
    }

    impl LoopBuf {
        const fn new() -> Self {
            Self { buf: [0u8; 128], r: 0, w: 0, len: 0 }
        }

        fn push(&mut self, data: &[u8]) -> usize {
            let mut n = 0;
            for &b in data {
                if self.len == self.buf.len() {
                    break;
                }
                self.buf[self.w] = b;
                self.w = (self.w + 1) % self.buf.len();
                self.len += 1;
                n += 1;
            }
            n
        }

        fn pop(&mut self, out: &mut [u8]) -> usize {
            let mut n = 0;
            for slot in out.iter_mut() {
                if self.len == 0 {
                    break;
                }
                *slot = self.buf[self.r];
                self.r = (self.r + 1) % self.buf.len();
                self.len -= 1;
                n += 1;
            }
            n
        }
    }

    enum Listener {
        Tcp(OsTcpListener),
        Loop { port: u16, pending: Option<u32> },
    }

    enum Stream {
        Tcp(OsTcpStream),
        Loop { peer: u32, rx: LoopBuf },
    }

    struct LoopUdp {
        rx: LoopBuf,
        port: u16,
    }

    enum UdpSock {
        Udp(nexus_net_os::OsUdpSocket),
        Loop(LoopUdp),
    }

    // Bind to the routable service endpoints provided by init-lite routing.
    // Retry (bring-up) to avoid killing the service if init-lite isn't ready yet.
    // Resolve our service endpoints via init-lite routing, then use raw syscalls to avoid heap
    // allocations in the hot-path (nexus-service-entry uses a bump allocator).
    let (svc_recv_slot, _svc_send_slot) = loop {
        match KernelServer::new_for("netstackd") {
            Ok(s) => break s.slots(),
            Err(_) => {
                let _ = yield_();
            }
        }
    };
    let mut listeners: Vec<Option<Listener>> = Vec::new();
    let mut streams: Vec<Option<Stream>> = Vec::new();
    let mut udps: Vec<Option<UdpSock>> = Vec::new();

    loop {
        let now_ms = (nexus_abi::nsec().unwrap_or(0) / 1_000_000) as u64;
        net.poll(now_ms);

        let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut sid: u64 = 0;
        let mut buf = [0u8; 512];
        match nexus_abi::ipc_recv_v2(
            svc_recv_slot,
            &mut hdr,
            &mut buf,
            &mut sid,
            nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => {
                let n = n as usize;
                let req = &buf[..n];
                let reply_slot = if (hdr.flags & nexus_abi::ipc_hdr::CAP_MOVE) != 0 {
                    Some(hdr.src as u32)
                } else {
                    None
                };

                let reply = |frame: &[u8]| {
                    if let Some(slot) = reply_slot {
                        let rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, frame.len() as u32);
                        // Blocking reply: avoid silently dropping replies under queue pressure.
                        let _ = nexus_abi::ipc_send_v1(slot, &rh, frame, 0, 0);
                        let _ = nexus_abi::cap_close(slot);
                    }
                };

                if req.len() < 4 || req[0] != MAGIC0 || req[1] != MAGIC1 || req[2] != VERSION {
                    reply(&[MAGIC0, MAGIC1, VERSION, 0x80, STATUS_MALFORMED]);
                    let _ = yield_();
                    continue;
                }
                let op = req[3];
                match op {
                    OP_LISTEN => {
                        if req.len() != 6 {
                            reply(&[MAGIC0, MAGIC1, VERSION, OP_LISTEN | 0x80, STATUS_MALFORMED]);
                            let _ = yield_();
                            continue;
                        }
                        let _ = nexus_abi::debug_println("netstackd: rpc listen");
                        let port = u16::from_le_bytes([req[4], req[5]]);
                        if port == LOOPBACK_PORT || port == LOOPBACK_PORT_B {
                            listeners.push(Some(Listener::Loop { port, pending: None }));
                            let id = listeners.len() as u32;
                            let mut rsp = [0u8; 9];
                            rsp[0] = MAGIC0;
                            rsp[1] = MAGIC1;
                            rsp[2] = VERSION;
                            rsp[3] = OP_LISTEN | 0x80;
                            rsp[4] = STATUS_OK;
                            rsp[5..9].copy_from_slice(&id.to_le_bytes());
                            reply(&rsp);
                            let _ = nexus_abi::debug_println("netstackd: rpc listen ok");
                        } else {
                            let addr = NetSocketAddrV4::new([10, 0, 2, 15], port);
                            match net.tcp_listen(addr, 1) {
                                Ok(l) => {
                                    listeners.push(Some(Listener::Tcp(l)));
                                    let id = listeners.len() as u32;
                                    let mut rsp = [0u8; 9];
                                    rsp[0] = MAGIC0;
                                    rsp[1] = MAGIC1;
                                    rsp[2] = VERSION;
                                    rsp[3] = OP_LISTEN | 0x80;
                                    rsp[4] = STATUS_OK;
                                    rsp[5..9].copy_from_slice(&id.to_le_bytes());
                                    reply(&rsp);
                                    let _ = nexus_abi::debug_println("netstackd: rpc listen ok");
                                }
                                Err(_) => {
                                    reply(&[MAGIC0, MAGIC1, VERSION, OP_LISTEN | 0x80, STATUS_IO]);
                                    let _ = nexus_abi::debug_println("netstackd: rpc listen FAIL");
                                }
                            }
                        }
                    }
                    OP_ACCEPT => {
                        if req.len() != 8 {
                            reply(&[MAGIC0, MAGIC1, VERSION, OP_ACCEPT | 0x80, STATUS_MALFORMED]);
                            let _ = yield_();
                            continue;
                        }
                        let lid = u32::from_le_bytes([req[4], req[5], req[6], req[7]]) as usize;
                        let Some(Some(l)) = listeners.get_mut(lid.wrapping_sub(1)) else {
                            reply(&[MAGIC0, MAGIC1, VERSION, OP_ACCEPT | 0x80, STATUS_NOT_FOUND]);
                            let _ = yield_();
                            continue;
                        };
                        match l {
                            Listener::Tcp(l) => {
                                let deadline = now_ms + 1;
                                match l.accept(Some(deadline)) {
                                    Ok(s) => {
                                        streams.push(Some(Stream::Tcp(s)));
                                        let sid = streams.len() as u32;
                                        let mut rsp = [0u8; 9];
                                        rsp[0] = MAGIC0;
                                        rsp[1] = MAGIC1;
                                        rsp[2] = VERSION;
                                        rsp[3] = OP_ACCEPT | 0x80;
                                        rsp[4] = STATUS_OK;
                                        rsp[5..9].copy_from_slice(&sid.to_le_bytes());
                                        reply(&rsp);
                                        let _ =
                                            nexus_abi::debug_println("netstackd: rpc accept ok");
                                    }
                                    Err(nexus_net::NetError::WouldBlock) => {
                                        reply(&[
                                            MAGIC0,
                                            MAGIC1,
                                            VERSION,
                                            OP_ACCEPT | 0x80,
                                            STATUS_WOULD_BLOCK,
                                        ]);
                                    }
                                    Err(_) => reply(&[
                                        MAGIC0,
                                        MAGIC1,
                                        VERSION,
                                        OP_ACCEPT | 0x80,
                                        STATUS_IO,
                                    ]),
                                }
                            }
                            Listener::Loop { pending, .. } => {
                                if let Some(sid) = pending.take() {
                                    let mut rsp = [0u8; 9];
                                    rsp[0] = MAGIC0;
                                    rsp[1] = MAGIC1;
                                    rsp[2] = VERSION;
                                    rsp[3] = OP_ACCEPT | 0x80;
                                    rsp[4] = STATUS_OK;
                                    rsp[5..9].copy_from_slice(&sid.to_le_bytes());
                                    reply(&rsp);
                                    let _ = nexus_abi::debug_println("netstackd: rpc accept ok");
                                } else {
                                    reply(&[
                                        MAGIC0,
                                        MAGIC1,
                                        VERSION,
                                        OP_ACCEPT | 0x80,
                                        STATUS_WOULD_BLOCK,
                                    ]);
                                }
                            }
                        }
                    }
                    OP_CONNECT => {
                        if req.len() != 10 {
                            reply(&[MAGIC0, MAGIC1, VERSION, OP_CONNECT | 0x80, STATUS_MALFORMED]);
                            let _ = yield_();
                            continue;
                        }
                        let ip = [req[4], req[5], req[6], req[7]];
                        let port = u16::from_le_bytes([req[8], req[9]]);
                        if ip == [10, 0, 2, 15]
                            && (port == LOOPBACK_PORT || port == LOOPBACK_PORT_B)
                        {
                            // Create paired in-memory streams.
                            let a = (streams.len() + 1) as u32;
                            streams.push(Some(Stream::Loop { peer: a + 1, rx: LoopBuf::new() }));
                            streams.push(Some(Stream::Loop { peer: a, rx: LoopBuf::new() }));
                            // Queue server side on the loop listener.
                            for l in listeners.iter_mut() {
                                if let Some(Listener::Loop { port: listen_port, pending }) = l {
                                    if *listen_port == port && pending.is_none() {
                                        *pending = Some(a + 1);
                                        break;
                                    }
                                }
                            }
                            let mut rsp = [0u8; 9];
                            rsp[0] = MAGIC0;
                            rsp[1] = MAGIC1;
                            rsp[2] = VERSION;
                            rsp[3] = OP_CONNECT | 0x80;
                            rsp[4] = STATUS_OK;
                            rsp[5..9].copy_from_slice(&a.to_le_bytes());
                            reply(&rsp);
                            let _ = nexus_abi::debug_println("netstackd: rpc connect ok");
                        } else {
                            let remote = NetSocketAddrV4::new(ip, port);
                            let deadline = now_ms + 1;
                            match net.tcp_connect(remote, Some(deadline)) {
                                Ok(s) => {
                                    streams.push(Some(Stream::Tcp(s)));
                                    let sid = streams.len() as u32;
                                    let mut rsp = [0u8; 9];
                                    rsp[0] = MAGIC0;
                                    rsp[1] = MAGIC1;
                                    rsp[2] = VERSION;
                                    rsp[3] = OP_CONNECT | 0x80;
                                    rsp[4] = STATUS_OK;
                                    rsp[5..9].copy_from_slice(&sid.to_le_bytes());
                                    reply(&rsp);
                                    let _ = nexus_abi::debug_println("netstackd: rpc connect ok");
                                }
                                Err(nexus_net::NetError::WouldBlock) => {
                                    reply(&[
                                        MAGIC0,
                                        MAGIC1,
                                        VERSION,
                                        OP_CONNECT | 0x80,
                                        STATUS_WOULD_BLOCK,
                                    ]);
                                }
                                Err(_) => {
                                    reply(&[MAGIC0, MAGIC1, VERSION, OP_CONNECT | 0x80, STATUS_IO])
                                }
                            }
                        }
                    }
                    OP_UDP_BIND => {
                        // v1: [magic,ver,op, port:u16le]
                        // v2 (backward compatible): [magic,ver,op, ip[4], port:u16le]
                        if req.len() != 6 && req.len() != 10 {
                            reply(&[MAGIC0, MAGIC1, VERSION, OP_UDP_BIND | 0x80, STATUS_MALFORMED]);
                            let _ = yield_();
                            continue;
                        }
                        let (bind_ip, port) = if req.len() == 10 {
                            ([req[4], req[5], req[6], req[7]], u16::from_le_bytes([req[8], req[9]]))
                        } else {
                            ([10, 0, 2, 15], u16::from_le_bytes([req[4], req[5]]))
                        };
                        if port == LOOPBACK_UDP_PORT {
                            udps.push(Some(UdpSock::Loop(LoopUdp { rx: LoopBuf::new(), port })));
                            let id = udps.len() as u32;
                            let mut rsp = [0u8; 9];
                            rsp[0] = MAGIC0;
                            rsp[1] = MAGIC1;
                            rsp[2] = VERSION;
                            rsp[3] = OP_UDP_BIND | 0x80;
                            rsp[4] = STATUS_OK;
                            rsp[5..9].copy_from_slice(&id.to_le_bytes());
                            reply(&rsp);
                            let _ = yield_();
                            continue;
                        }
                        let addr = NetSocketAddrV4::new(bind_ip, port);
                        match net.udp_bind(addr) {
                            Ok(s) => {
                                udps.push(Some(UdpSock::Udp(s)));
                                let id = udps.len() as u32;
                                let mut rsp = [0u8; 9];
                                rsp[0] = MAGIC0;
                                rsp[1] = MAGIC1;
                                rsp[2] = VERSION;
                                rsp[3] = OP_UDP_BIND | 0x80;
                                rsp[4] = STATUS_OK;
                                rsp[5..9].copy_from_slice(&id.to_le_bytes());
                                reply(&rsp);
                            }
                            Err(nexus_net::NetError::AddrInUse) => {
                                reply(&[MAGIC0, MAGIC1, VERSION, OP_UDP_BIND | 0x80, STATUS_IO]);
                            }
                            Err(_) => {
                                reply(&[MAGIC0, MAGIC1, VERSION, OP_UDP_BIND | 0x80, STATUS_IO])
                            }
                        }
                    }
                    OP_UDP_SEND_TO => {
                        // [magic,ver,op, udp_id:u32le, ip[4], port:u16le, len:u16le, payload...]
                        if req.len() < 4 + 4 + 4 + 2 + 2 {
                            reply(&[
                                MAGIC0,
                                MAGIC1,
                                VERSION,
                                OP_UDP_SEND_TO | 0x80,
                                STATUS_MALFORMED,
                            ]);
                            let _ = yield_();
                            continue;
                        }
                        let udp_id = u32::from_le_bytes([req[4], req[5], req[6], req[7]]) as usize;
                        let ip = [req[8], req[9], req[10], req[11]];
                        let port = u16::from_le_bytes([req[12], req[13]]);
                        let len = u16::from_le_bytes([req[14], req[15]]) as usize;
                        if req.len() != 16 + len {
                            reply(&[
                                MAGIC0,
                                MAGIC1,
                                VERSION,
                                OP_UDP_SEND_TO | 0x80,
                                STATUS_MALFORMED,
                            ]);
                            let _ = yield_();
                            continue;
                        }
                        let idx = udp_id.wrapping_sub(1);
                        let Some(Some(sock)) = udps.get(idx) else {
                            reply(&[
                                MAGIC0,
                                MAGIC1,
                                VERSION,
                                OP_UDP_SEND_TO | 0x80,
                                STATUS_NOT_FOUND,
                            ]);
                            let _ = yield_();
                            continue;
                        };
                        match sock {
                            UdpSock::Udp(_) => {
                                let Some(Some(UdpSock::Udp(s))) = udps.get_mut(idx) else {
                                    reply(&[
                                        MAGIC0,
                                        MAGIC1,
                                        VERSION,
                                        OP_UDP_SEND_TO | 0x80,
                                        STATUS_IO,
                                    ]);
                                    let _ = yield_();
                                    continue;
                                };
                                let dst = NetSocketAddrV4::new(ip, port);
                                match s.send_to(&req[16..], dst) {
                                    Ok(n) => {
                                        let mut rsp = [0u8; 7];
                                        rsp[0] = MAGIC0;
                                        rsp[1] = MAGIC1;
                                        rsp[2] = VERSION;
                                        rsp[3] = OP_UDP_SEND_TO | 0x80;
                                        rsp[4] = STATUS_OK;
                                        rsp[5..7].copy_from_slice(&(n as u16).to_le_bytes());
                                        reply(&rsp);
                                    }
                                    Err(nexus_net::NetError::WouldBlock) => reply(&[
                                        MAGIC0,
                                        MAGIC1,
                                        VERSION,
                                        OP_UDP_SEND_TO | 0x80,
                                        STATUS_WOULD_BLOCK,
                                    ]),
                                    Err(_) => reply(&[
                                        MAGIC0,
                                        MAGIC1,
                                        VERSION,
                                        OP_UDP_SEND_TO | 0x80,
                                        STATUS_IO,
                                    ]),
                                }
                            }
                            UdpSock::Loop(LoopUdp { rx: _, port: local }) => {
                                // Only supports loopback to self on the same port.
                                if ip != [10, 0, 2, 15] || port != *local {
                                    reply(&[
                                        MAGIC0,
                                        MAGIC1,
                                        VERSION,
                                        OP_UDP_SEND_TO | 0x80,
                                        STATUS_IO,
                                    ]);
                                    let _ = yield_();
                                    continue;
                                }
                                // Push into our own RX buffer (loopback).
                                let Some(Some(UdpSock::Loop(LoopUdp { rx, .. }))) =
                                    udps.get_mut(idx)
                                else {
                                    reply(&[
                                        MAGIC0,
                                        MAGIC1,
                                        VERSION,
                                        OP_UDP_SEND_TO | 0x80,
                                        STATUS_IO,
                                    ]);
                                    let _ = yield_();
                                    continue;
                                };
                                let wrote = rx.push(&req[16..]);
                                if wrote == 0 {
                                    reply(&[
                                        MAGIC0,
                                        MAGIC1,
                                        VERSION,
                                        OP_UDP_SEND_TO | 0x80,
                                        STATUS_WOULD_BLOCK,
                                    ]);
                                } else {
                                    let mut rsp = [0u8; 7];
                                    rsp[0] = MAGIC0;
                                    rsp[1] = MAGIC1;
                                    rsp[2] = VERSION;
                                    rsp[3] = OP_UDP_SEND_TO | 0x80;
                                    rsp[4] = STATUS_OK;
                                    rsp[5..7].copy_from_slice(&(wrote as u16).to_le_bytes());
                                    reply(&rsp);
                                }
                            }
                        }
                    }
                    OP_UDP_RECV_FROM => {
                        // [magic,ver,op, udp_id:u32le, max:u16le]
                        if req.len() != 4 + 4 + 2 {
                            reply(&[
                                MAGIC0,
                                MAGIC1,
                                VERSION,
                                OP_UDP_RECV_FROM | 0x80,
                                STATUS_MALFORMED,
                            ]);
                            let _ = yield_();
                            continue;
                        }
                        let udp_id = u32::from_le_bytes([req[4], req[5], req[6], req[7]]) as usize;
                        let max = u16::from_le_bytes([req[8], req[9]]) as usize;
                        let max = core::cmp::min(max, 460); // keep reply bounded
                        let idx = udp_id.wrapping_sub(1);
                        let Some(Some(sock)) = udps.get(idx) else {
                            reply(&[
                                MAGIC0,
                                MAGIC1,
                                VERSION,
                                OP_UDP_RECV_FROM | 0x80,
                                STATUS_NOT_FOUND,
                            ]);
                            let _ = yield_();
                            continue;
                        };
                        match sock {
                            UdpSock::Udp(_) => {
                                let Some(Some(UdpSock::Udp(s))) = udps.get_mut(idx) else {
                                    reply(&[
                                        MAGIC0,
                                        MAGIC1,
                                        VERSION,
                                        OP_UDP_RECV_FROM | 0x80,
                                        STATUS_IO,
                                    ]);
                                    let _ = yield_();
                                    continue;
                                };
                                let mut tmp = [0u8; 460];
                                match s.recv_from(&mut tmp[..max]) {
                                    Ok((n, from)) => {
                                        let mut rsp = [0u8; 512];
                                        rsp[0] = MAGIC0;
                                        rsp[1] = MAGIC1;
                                        rsp[2] = VERSION;
                                        rsp[3] = OP_UDP_RECV_FROM | 0x80;
                                        rsp[4] = STATUS_OK;
                                        rsp[5..7].copy_from_slice(&(n as u16).to_le_bytes());
                                        rsp[7..11].copy_from_slice(&from.ip.0);
                                        rsp[11..13].copy_from_slice(&from.port.to_le_bytes());
                                        rsp[13..13 + n].copy_from_slice(&tmp[..n]);
                                        reply(&rsp[..13 + n]);
                                    }
                                    Err(nexus_net::NetError::WouldBlock) => reply(&[
                                        MAGIC0,
                                        MAGIC1,
                                        VERSION,
                                        OP_UDP_RECV_FROM | 0x80,
                                        STATUS_WOULD_BLOCK,
                                    ]),
                                    Err(_) => reply(&[
                                        MAGIC0,
                                        MAGIC1,
                                        VERSION,
                                        OP_UDP_RECV_FROM | 0x80,
                                        STATUS_IO,
                                    ]),
                                }
                            }
                            UdpSock::Loop(LoopUdp { rx: _, port: _ }) => {
                                let mut tmp = [0u8; 460];
                                let Some(Some(UdpSock::Loop(LoopUdp { rx, port }))) =
                                    udps.get_mut(idx)
                                else {
                                    reply(&[
                                        MAGIC0,
                                        MAGIC1,
                                        VERSION,
                                        OP_UDP_RECV_FROM | 0x80,
                                        STATUS_IO,
                                    ]);
                                    let _ = yield_();
                                    continue;
                                };
                                let n = rx.pop(&mut tmp[..max]);
                                if n == 0 {
                                    reply(&[
                                        MAGIC0,
                                        MAGIC1,
                                        VERSION,
                                        OP_UDP_RECV_FROM | 0x80,
                                        STATUS_WOULD_BLOCK,
                                    ]);
                                } else {
                                    let mut rsp = [0u8; 512];
                                    rsp[0] = MAGIC0;
                                    rsp[1] = MAGIC1;
                                    rsp[2] = VERSION;
                                    rsp[3] = OP_UDP_RECV_FROM | 0x80;
                                    rsp[4] = STATUS_OK;
                                    rsp[5..7].copy_from_slice(&(n as u16).to_le_bytes());
                                    rsp[7..11].copy_from_slice(&[10, 0, 2, 15]);
                                    rsp[11..13].copy_from_slice(&port.to_le_bytes());
                                    rsp[13..13 + n].copy_from_slice(&tmp[..n]);
                                    reply(&rsp[..13 + n]);
                                }
                            }
                        }
                    }
                    OP_WRITE => {
                        if req.len() < 10 {
                            reply(&[MAGIC0, MAGIC1, VERSION, OP_WRITE | 0x80, STATUS_MALFORMED]);
                            let _ = yield_();
                            continue;
                        }
                        let sid = u32::from_le_bytes([req[4], req[5], req[6], req[7]]) as usize;
                        let len = u16::from_le_bytes([req[8], req[9]]) as usize;
                        if req.len() != 10 + len {
                            reply(&[MAGIC0, MAGIC1, VERSION, OP_WRITE | 0x80, STATUS_MALFORMED]);
                            let _ = yield_();
                            continue;
                        }
                        let sid0 = sid.wrapping_sub(1);
                        let Some(Some(kind)) = streams.get(sid0) else {
                            reply(&[MAGIC0, MAGIC1, VERSION, OP_WRITE | 0x80, STATUS_NOT_FOUND]);
                            let _ = yield_();
                            continue;
                        };
                        match kind {
                            Stream::Tcp(_) => {
                                let Some(Some(Stream::Tcp(s))) = streams.get_mut(sid0) else {
                                    reply(&[MAGIC0, MAGIC1, VERSION, OP_WRITE | 0x80, STATUS_IO]);
                                    let _ = yield_();
                                    continue;
                                };
                                let deadline = now_ms + 1;
                                match s.write(Some(deadline), &req[10..]) {
                                    Ok(n) => {
                                        let mut rsp = [0u8; 7];
                                        rsp[0] = MAGIC0;
                                        rsp[1] = MAGIC1;
                                        rsp[2] = VERSION;
                                        rsp[3] = OP_WRITE | 0x80;
                                        rsp[4] = STATUS_OK;
                                        rsp[5..7].copy_from_slice(&(n as u16).to_le_bytes());
                                        reply(&rsp);
                                    }
                                    Err(nexus_net::NetError::WouldBlock) => {
                                        reply(&[
                                            MAGIC0,
                                            MAGIC1,
                                            VERSION,
                                            OP_WRITE | 0x80,
                                            STATUS_WOULD_BLOCK,
                                        ]);
                                    }
                                    Err(_) => reply(&[
                                        MAGIC0,
                                        MAGIC1,
                                        VERSION,
                                        OP_WRITE | 0x80,
                                        STATUS_IO,
                                    ]),
                                }
                            }
                            Stream::Loop { peer, .. } => {
                                let peer0 = (*peer as usize).wrapping_sub(1);
                                let Some(Some(Stream::Loop { rx, .. })) = streams.get_mut(peer0)
                                else {
                                    reply(&[MAGIC0, MAGIC1, VERSION, OP_WRITE | 0x80, STATUS_IO]);
                                    let _ = yield_();
                                    continue;
                                };
                                let wrote = rx.push(&req[10..]);
                                if wrote == 0 {
                                    reply(&[
                                        MAGIC0,
                                        MAGIC1,
                                        VERSION,
                                        OP_WRITE | 0x80,
                                        STATUS_WOULD_BLOCK,
                                    ]);
                                } else {
                                    let mut rsp = [0u8; 7];
                                    rsp[0] = MAGIC0;
                                    rsp[1] = MAGIC1;
                                    rsp[2] = VERSION;
                                    rsp[3] = OP_WRITE | 0x80;
                                    rsp[4] = STATUS_OK;
                                    rsp[5..7].copy_from_slice(&(wrote as u16).to_le_bytes());
                                    reply(&rsp);
                                }
                            }
                        }
                    }
                    OP_READ => {
                        if req.len() != 10 {
                            reply(&[MAGIC0, MAGIC1, VERSION, OP_READ | 0x80, STATUS_MALFORMED]);
                            let _ = yield_();
                            continue;
                        }
                        let sid = u32::from_le_bytes([req[4], req[5], req[6], req[7]]) as usize;
                        let max = u16::from_le_bytes([req[8], req[9]]) as usize;
                        let max = core::cmp::min(max, 480); // keep reply bounded under 512
                        let Some(Some(s)) = streams.get_mut(sid.wrapping_sub(1)) else {
                            reply(&[MAGIC0, MAGIC1, VERSION, OP_READ | 0x80, STATUS_NOT_FOUND]);
                            let _ = yield_();
                            continue;
                        };
                        match s {
                            Stream::Tcp(s) => {
                                let deadline = now_ms + 1;
                                let mut buf = [0u8; 480];
                                match s.read(Some(deadline), &mut buf[..max]) {
                                    Ok(n) => {
                                        let mut rsp = [0u8; 512];
                                        rsp[0] = MAGIC0;
                                        rsp[1] = MAGIC1;
                                        rsp[2] = VERSION;
                                        rsp[3] = OP_READ | 0x80;
                                        rsp[4] = STATUS_OK;
                                        rsp[5..7].copy_from_slice(&(n as u16).to_le_bytes());
                                        rsp[7..7 + n].copy_from_slice(&buf[..n]);
                                        reply(&rsp[..7 + n]);
                                    }
                                    Err(nexus_net::NetError::WouldBlock) => {
                                        reply(&[
                                            MAGIC0,
                                            MAGIC1,
                                            VERSION,
                                            OP_READ | 0x80,
                                            STATUS_WOULD_BLOCK,
                                        ]);
                                    }
                                    Err(_) => {
                                        reply(&[MAGIC0, MAGIC1, VERSION, OP_READ | 0x80, STATUS_IO])
                                    }
                                }
                            }
                            Stream::Loop { rx, .. } => {
                                let mut out = [0u8; 480];
                                let n = rx.pop(&mut out[..max]);
                                if n == 0 {
                                    reply(&[
                                        MAGIC0,
                                        MAGIC1,
                                        VERSION,
                                        OP_READ | 0x80,
                                        STATUS_WOULD_BLOCK,
                                    ]);
                                } else {
                                    let mut rsp = [0u8; 512];
                                    rsp[0] = MAGIC0;
                                    rsp[1] = MAGIC1;
                                    rsp[2] = VERSION;
                                    rsp[3] = OP_READ | 0x80;
                                    rsp[4] = STATUS_OK;
                                    rsp[5..7].copy_from_slice(&(n as u16).to_le_bytes());
                                    rsp[7..7 + n].copy_from_slice(&out[..n]);
                                    reply(&rsp[..7 + n]);
                                }
                            }
                        }
                    }
                    OP_ICMP_PING => {
                        // OP_ICMP_PING: [magic,ver,op, ip[4], timeout_ms:u16le]
                        if req.len() != 10 {
                            reply(&[
                                MAGIC0,
                                MAGIC1,
                                VERSION,
                                OP_ICMP_PING | 0x80,
                                STATUS_MALFORMED,
                            ]);
                            let _ = yield_();
                            continue;
                        }
                        let target_ip = [req[4], req[5], req[6], req[7]];
                        let timeout_ms = u16::from_le_bytes([req[8], req[9]]) as u64;
                        let ping_start = (nexus_abi::nsec().unwrap_or(0) / 1_000_000) as u64;

                        match net.icmp_ping(target_ip, ping_start, timeout_ms) {
                            Ok(rtt_ms) => {
                                let mut rsp = [0u8; 11];
                                rsp[0] = MAGIC0;
                                rsp[1] = MAGIC1;
                                rsp[2] = VERSION;
                                rsp[3] = OP_ICMP_PING | 0x80;
                                rsp[4] = STATUS_OK;
                                // Include RTT in response (u16le, capped at 65535)
                                let rtt_capped = core::cmp::min(rtt_ms, 65535) as u16;
                                rsp[5..7].copy_from_slice(&rtt_capped.to_le_bytes());
                                reply(&rsp[..7]);
                            }
                            Err(_) => {
                                reply(&[
                                    MAGIC0,
                                    MAGIC1,
                                    VERSION,
                                    OP_ICMP_PING | 0x80,
                                    STATUS_TIMED_OUT,
                                ]);
                            }
                        }
                    }
                    _ => reply(&[MAGIC0, MAGIC1, VERSION, op | 0x80, STATUS_MALFORMED]),
                }
            }
            Err(_) => {}
        }

        let _ = yield_();
    }
}

#[cfg(not(all(
    nexus_env = "os",
    target_arch = "riscv64",
    target_os = "none",
    feature = "os-lite"
)))]
fn main() -> ! {
    // Host builds intentionally do nothing for now.
    loop {
        core::hint::spin_loop();
    }
}
