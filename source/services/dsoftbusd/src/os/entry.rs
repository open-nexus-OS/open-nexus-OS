// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Small entry/wiring helpers shared by os_entry flow.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered transitively by dsoftbusd QEMU proofs.
//!
//! ADR: docs/adr/0005-dsoftbus-architecture.md

pub(crate) const DEFAULT_LOCAL_IP: [u8; 4] = crate::os::entry_pure::QEMU_USERNET_FALLBACK_IP;
/// dsoftbusd's CAP_MOVE reply inbox, as declared by the topology (TASK-0324 P4f-4).
pub(crate) const DSOFT_REPLY_RECV_SLOT: u32 = nexus_service_topology::slots::dsoftbusd::REPLY.recv;
/// The inbox's SEND half, cloned into every request that expects a reply.
pub(crate) const DSOFT_REPLY_SEND_SLOT: u32 = nexus_service_topology::slots::dsoftbusd::REPLY.send;

const MAGIC0: u8 = b'N';
const MAGIC1: u8 = b'S';
const VERSION: u8 = 1;
const OP_LISTEN: u8 = 1;
const OP_CONNECT: u8 = 3;
const OP_READ: u8 = 4;
const OP_WRITE: u8 = 5;
const OP_UDP_BIND: u8 = 6;
const OP_UDP_SEND_TO: u8 = 7;
const OP_UDP_RECV_FROM: u8 = 8;
const STATUS_OK: u8 = 0;
const STATUS_WOULD_BLOCK: u8 = 3;
const STATUS_IO: u8 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OsTransportSelection {
    QuicUdp,
}

pub(crate) fn select_os_transport_for_session() -> OsTransportSelection {
    // OS QUIC v2 datapath runs over UDP datagrams with explicit Noise XK
    // handshake framing (no fallback marker on this path).
    let _ = nexus_abi::trace_line("dsoftbusd: transport selected quic");
    OsTransportSelection::QuicUdp
}

#[inline]
pub(crate) fn is_cross_vm_ip(local_ip: [u8; 4]) -> bool {
    crate::os::entry_pure::is_cross_vm_ip(local_ip)
}

#[inline]
pub(crate) fn next_nonce(n: &mut u64) -> u64 {
    crate::os::entry_pure::next_nonce(n)
}

/// dsoftbusd's own legs into the ONE netstack RPC (`os::netstack::rpc::rpc_nonce`). This file
/// used to carry a second, divergent copy of that function — same protocol, different budget,
/// different failure mode; TASK-0054C P2-c deleted the copy, not the caller.
pub(crate) fn rpc_nonce(
    net: &nexus_ipc::KernelClient,
    req: &[u8],
    expect_rsp_op: u8,
    nonce: u64,
) -> core::result::Result<[u8; 512], ()> {
    crate::os::netstack::rpc::rpc_nonce(
        net,
        req,
        expect_rsp_op,
        nonce,
        DSOFT_REPLY_RECV_SLOT,
        DSOFT_REPLY_SEND_SLOT,
    )
}

pub(crate) fn get_local_ip(
    net: &nexus_ipc::KernelClient,
    nonce_ctr: &mut u64,
    iter: u32,
) -> Option<[u8; 4]> {
    const OP_LOCAL_ADDR: u8 = 10;
    let nonce = next_nonce(nonce_ctr);
    let mut req = [0u8; 12];
    req[0] = MAGIC0;
    req[1] = MAGIC1;
    req[2] = VERSION;
    req[3] = OP_LOCAL_ADDR;
    req[4..12].copy_from_slice(&nonce.to_le_bytes());
    let rsp = match rpc_nonce(net, &req, OP_LOCAL_ADDR | 0x80, nonce) {
        Ok(r) => r,
        Err(_) => {
            if iter == 30 {
                let _ = nexus_abi::debug_println("dsoftbusd: local ip rpc fail");
            }
            return None;
        }
    };
    if rsp[0] != MAGIC0 || rsp[1] != MAGIC1 || rsp[2] != VERSION || rsp[3] != (OP_LOCAL_ADDR | 0x80)
    {
        return None;
    }
    if rsp[4] != STATUS_OK {
        return None;
    }
    Some([rsp[5], rsp[6], rsp[7], rsp[8]])
}

/// The declared HOLD of the single-VM session path (network family HOLD until the joint
/// replanning, tasks/IMPLEMENTATION-ORDER.md "Netz-Gruppe"): the daemon stays alive and
/// blocks in the kernel on a fence nobody signals — zero CPU, no error exit for init's
/// supervision to count, no `yield_()` spin. Until TASK-0324 P7 this daemon never got here at
/// all: a 10 000-yield slot probe at Idle QoS wedged it for every proof boot, and the
/// bring-up failures below each spun a core forever.
pub(crate) fn hold_forever(marker: &str) -> ! {
    let _ = nexus_abi::debug_println(marker);
    if let Ok(fence) = nexus_abi::fence_create() {
        while nexus_abi::fence_wait(fence, 1, 0).is_ok() {}
    }
    // No fence (ABI refused): block on our own server endpoint instead; a request that
    // lands there is dropped (the single-VM server is not up — the caller's probe times out
    // exactly as it did while the daemon was wedged).
    let server = nexus_service_topology::slots::dsoftbusd::SERVER;
    loop {
        let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 16];
        let _ =
            nexus_abi::ipc_recv_v1(server.recv, &mut hdr, &mut buf, nexus_abi::IPC_SYS_TRUNCATE, 0);
    }
}

pub(crate) fn init_netstack_client() -> core::result::Result<nexus_ipc::KernelClient, ()> {
    let _ = nexus_abi::trace_line("dsoftbusd: entry");
    let route = nexus_service_topology::slots::dsoftbusd::NETSTACKD;
    match nexus_ipc::KernelClient::new_with_slots(route.send, route.recv) {
        Ok(c) => Ok(c),
        Err(_) => {
            let _ = nexus_abi::debug_println("dsoftbusd: netstackd slots fail");
            Err(())
        }
    }
}

pub(crate) fn resolve_local_ip_with_wait(
    net: &nexus_ipc::KernelClient,
    nonce_ctr: &mut u64,
) -> [u8; 4] {
    let _ = nexus_abi::trace_line("dsoftbusd: waiting for local ip");
    let mut local_ip = DEFAULT_LOCAL_IP;
    let mut local_ip_resolved = false;
    for i in 0..300u32 {
        if let Some(ip) = get_local_ip(net, nonce_ctr, i) {
            local_ip = ip;
            local_ip_resolved = true;
            break;
        }
        if i % 50 == 0 && i > 0 {
            let _ = nexus_abi::trace_line("dsoftbusd: local ip wait");
        }
        for _ in 0..500 {
            let _ = nexus_abi::yield_();
        }
    }
    if !local_ip_resolved {
        let _ = nexus_abi::trace_line("dsoftbusd: local ip fallback");
    } else {
        let _ = nexus_abi::trace_line("dsoftbusd: local ip ok");
    }
    let _ = nexus_abi::trace_line("dsoftbusd: ip phase done");
    local_ip
}

pub(crate) fn bind_discovery_udp_with_wait(
    net: &nexus_ipc::KernelClient,
    nonce_ctr: &mut u64,
    disc_port: u16,
) -> u32 {
    let _ = nexus_abi::trace_line("dsoftbusd: udp bind begin");
    let udp_id = match udp_bind(net, nonce_ctr, [0, 0, 0, 0], disc_port) {
        Ok(id) => id,
        Err(()) => {
            let _ = nexus_abi::debug_println("dsoftbusd: udp bind rpc timeout");
            loop {
                let _ = nexus_abi::yield_();
            }
        }
    };
    let _ = nexus_abi::trace_line("dsoftbusd: udp bind ok");
    let _ = nexus_abi::trace_line("dsoftbusd: discovery up (udp loopback)");
    udp_id
}

pub(crate) fn udp_bind(
    net: &nexus_ipc::KernelClient,
    nonce_ctr: &mut u64,
    bind_ip: [u8; 4],
    bind_port: u16,
) -> core::result::Result<u32, ()> {
    let mut req = [0u8; 18];
    req[0] = MAGIC0;
    req[1] = MAGIC1;
    req[2] = VERSION;
    req[3] = OP_UDP_BIND;
    req[4..8].copy_from_slice(&bind_ip);
    req[8..10].copy_from_slice(&bind_port.to_le_bytes());
    for _ in 0..500 {
        let nonce = next_nonce(nonce_ctr);
        req[10..18].copy_from_slice(&nonce.to_le_bytes());
        let rsp = rpc_nonce(net, &req, OP_UDP_BIND | 0x80, nonce)?;
        if rsp[0] == MAGIC0
            && rsp[1] == MAGIC1
            && rsp[2] == VERSION
            && rsp[3] == (OP_UDP_BIND | 0x80)
            && rsp[4] == STATUS_OK
        {
            return Ok(u32::from_le_bytes([rsp[5], rsp[6], rsp[7], rsp[8]]));
        }
        let _ = nexus_abi::yield_();
    }
    Err(())
}

pub(crate) fn udp_send_to(
    net: &nexus_ipc::KernelClient,
    nonce_ctr: &mut u64,
    udp_id: u32,
    dst_ip: [u8; 4],
    dst_port: u16,
    payload: &[u8],
) -> core::result::Result<(), ()> {
    if payload.len() > 256 {
        return Err(());
    }
    let mut req = [0u8; 16 + 256 + 8];
    req[0] = MAGIC0;
    req[1] = MAGIC1;
    req[2] = VERSION;
    req[3] = OP_UDP_SEND_TO;
    req[4..8].copy_from_slice(&udp_id.to_le_bytes());
    req[8..12].copy_from_slice(&dst_ip);
    req[12..14].copy_from_slice(&dst_port.to_le_bytes());
    req[14..16].copy_from_slice(&(payload.len() as u16).to_le_bytes());
    req[16..16 + payload.len()].copy_from_slice(payload);
    let nonce = next_nonce(nonce_ctr);
    req[16 + payload.len()..16 + payload.len() + 8].copy_from_slice(&nonce.to_le_bytes());
    let rsp = rpc_nonce(net, &req[..16 + payload.len() + 8], OP_UDP_SEND_TO | 0x80, nonce)?;
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

pub(crate) fn udp_recv_from(
    net: &nexus_ipc::KernelClient,
    nonce_ctr: &mut u64,
    udp_id: u32,
    out: &mut [u8],
) -> core::result::Result<Option<([u8; 4], u16, usize)>, ()> {
    let mut req = [0u8; 18];
    req[0] = MAGIC0;
    req[1] = MAGIC1;
    req[2] = VERSION;
    req[3] = OP_UDP_RECV_FROM;
    req[4..8].copy_from_slice(&udp_id.to_le_bytes());
    req[8..10].copy_from_slice(&((out.len().min(460)) as u16).to_le_bytes());
    let nonce = next_nonce(nonce_ctr);
    req[10..18].copy_from_slice(&nonce.to_le_bytes());
    let rsp = rpc_nonce(net, &req, OP_UDP_RECV_FROM | 0x80, nonce)?;
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

pub(crate) fn listen_with_retry(
    net: &nexus_ipc::KernelClient,
    nonce_ctr: &mut u64,
    port: u16,
) -> core::result::Result<u32, ()> {
    let mut req = [0u8; 14];
    req[0] = MAGIC0;
    req[1] = MAGIC1;
    req[2] = VERSION;
    req[3] = OP_LISTEN;
    req[4] = (port & 0xff) as u8;
    req[5] = (port >> 8) as u8;
    let mut out: Option<u32> = None;
    for _ in 0..50_000 {
        let nonce = next_nonce(nonce_ctr);
        req[6..14].copy_from_slice(&nonce.to_le_bytes());
        let rsp = rpc_nonce(net, &req, OP_LISTEN | 0x80, nonce)?;
        if rsp[0] == MAGIC0
            && rsp[1] == MAGIC1
            && rsp[2] == VERSION
            && rsp[3] == (OP_LISTEN | 0x80)
            && rsp[4] == STATUS_OK
        {
            out = Some(u32::from_le_bytes([rsp[5], rsp[6], rsp[7], rsp[8]]));
            break;
        }
        let _ = nexus_abi::yield_();
    }
    match out {
        Some(id) => Ok(id),
        None => {
            hold_forever("dsoftbusd: listen FAIL");
        }
    }
}

pub(crate) fn rebuild_peer_ips(
    peers: &nexus_peer_lru::PeerLru,
    ips: &mut alloc::vec::Vec<(alloc::string::String, [u8; 4])>,
) {
    crate::os::entry_pure::rebuild_peer_ips(peers, ips);
}

pub(crate) fn set_peer_ip(
    ips: &mut alloc::vec::Vec<(alloc::string::String, [u8; 4])>,
    device_id: &str,
    ip: [u8; 4],
) {
    crate::os::entry_pure::set_peer_ip(ips, device_id, ip);
}

pub(crate) fn get_peer_ip(
    ips: &[(alloc::string::String, [u8; 4])],
    device_id: &str,
) -> Option<[u8; 4]> {
    crate::os::entry_pure::get_peer_ip(ips, device_id)
}

/// SECURITY: bring-up test keys, NOT production custody.
pub(crate) fn derive_test_secret(tag: u8, port: u16) -> [u8; 32] {
    crate::os::entry_pure::derive_test_secret(tag, port)
}

pub(crate) fn tcp_connect(
    net: &nexus_ipc::KernelClient,
    nonce_ctr: &mut u64,
    ip: [u8; 4],
    port: u16,
) -> core::result::Result<u32, ()> {
    for _ in 0..100_000 {
        let nonce = next_nonce(nonce_ctr);
        let mut c = [0u8; 18];
        c[0] = MAGIC0;
        c[1] = MAGIC1;
        c[2] = VERSION;
        c[3] = OP_CONNECT;
        c[4..8].copy_from_slice(&ip);
        c[8..10].copy_from_slice(&port.to_le_bytes());
        c[10..18].copy_from_slice(&nonce.to_le_bytes());
        let rsp = rpc_nonce(net, &c, OP_CONNECT | 0x80, nonce)?;
        if rsp[0] == MAGIC0
            && rsp[1] == MAGIC1
            && rsp[2] == VERSION
            && rsp[3] == (OP_CONNECT | 0x80)
        {
            if rsp[4] == STATUS_OK {
                return Ok(u32::from_le_bytes([rsp[5], rsp[6], rsp[7], rsp[8]]));
            }
            if rsp[4] == STATUS_WOULD_BLOCK || rsp[4] == STATUS_IO {
                let _ = nexus_abi::yield_();
                continue;
            }
        }
        return Err(());
    }
    Err(())
}

pub(crate) fn tcp_accept(
    net: &nexus_ipc::KernelClient,
    nonce_ctr: &mut u64,
    lid: u32,
) -> core::result::Result<u32, ()> {
    const OP_ACCEPT: u8 = 2;
    for _ in 0..100_000 {
        let nonce = next_nonce(nonce_ctr);
        let mut a = [0u8; 16];
        a[0] = MAGIC0;
        a[1] = MAGIC1;
        a[2] = VERSION;
        a[3] = OP_ACCEPT;
        a[4..8].copy_from_slice(&lid.to_le_bytes());
        a[8..16].copy_from_slice(&nonce.to_le_bytes());
        let rsp = rpc_nonce(net, &a, OP_ACCEPT | 0x80, nonce)?;
        if rsp[0] == MAGIC0 && rsp[1] == MAGIC1 && rsp[2] == VERSION && rsp[3] == (OP_ACCEPT | 0x80)
        {
            if rsp[4] == STATUS_OK {
                return Ok(u32::from_le_bytes([rsp[5], rsp[6], rsp[7], rsp[8]]));
            }
            if rsp[4] == STATUS_WOULD_BLOCK {
                let _ = nexus_abi::yield_();
                continue;
            }
        }
        return Err(());
    }
    Err(())
}

pub(crate) fn dual_stream_read(
    net: &nexus_ipc::KernelClient,
    nonce_ctr: &mut u64,
    sid: u32,
    buf: &mut [u8],
) -> core::result::Result<(), ()> {
    let len = buf.len();
    for _ in 0..100_000 {
        let nonce = next_nonce(nonce_ctr);
        let mut r = [0u8; 18];
        r[0] = MAGIC0;
        r[1] = MAGIC1;
        r[2] = VERSION;
        r[3] = OP_READ;
        r[4..8].copy_from_slice(&sid.to_le_bytes());
        r[8..10].copy_from_slice(&(len as u16).to_le_bytes());
        r[10..18].copy_from_slice(&nonce.to_le_bytes());
        let rsp = rpc_nonce(net, &r, OP_READ | 0x80, nonce)?;
        if rsp[4] == STATUS_OK {
            let n = u16::from_le_bytes([rsp[5], rsp[6]]) as usize;
            if n == len && 7 + n <= rsp.len() {
                buf.copy_from_slice(&rsp[7..7 + n]);
                return Ok(());
            }
            return Err(());
        }
        if rsp[4] == STATUS_WOULD_BLOCK {
            let _ = nexus_abi::yield_();
            continue;
        }
        return Err(());
    }
    Err(())
}

pub(crate) fn dual_stream_write(
    net: &nexus_ipc::KernelClient,
    nonce_ctr: &mut u64,
    sid: u32,
    data: &[u8],
) -> core::result::Result<(), ()> {
    let mut w = [0u8; 256];
    if data.len() + 18 > w.len() {
        return Err(());
    }
    let nonce = next_nonce(nonce_ctr);
    w[0] = MAGIC0;
    w[1] = MAGIC1;
    w[2] = VERSION;
    w[3] = OP_WRITE;
    w[4..8].copy_from_slice(&sid.to_le_bytes());
    w[8..10].copy_from_slice(&(data.len() as u16).to_le_bytes());
    w[10..10 + data.len()].copy_from_slice(data);
    w[10 + data.len()..10 + data.len() + 8].copy_from_slice(&nonce.to_le_bytes());
    let rsp = rpc_nonce(net, &w[..10 + data.len() + 8], OP_WRITE | 0x80, nonce)?;
    if rsp[4] == STATUS_OK {
        Ok(())
    } else {
        Err(())
    }
}
