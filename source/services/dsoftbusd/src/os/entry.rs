//! CONTEXT: Small entry/wiring helpers shared by os_entry flow.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered transitively by dsoftbusd QEMU proofs.
//!
//! ADR: docs/adr/0005-dsoftbus-architecture.md

pub(crate) const DEFAULT_LOCAL_IP: [u8; 4] = [10, 0, 2, 15];
pub(crate) const DSOFT_REPLY_RECV_SLOT: u32 = 0x5;
pub(crate) const DSOFT_REPLY_SEND_SLOT: u32 = 0x6;

const MAGIC0: u8 = b'N';
const MAGIC1: u8 = b'S';
const VERSION: u8 = 1;
const OP_LISTEN: u8 = 1;
const OP_CONNECT: u8 = 3;
const OP_READ: u8 = 4;
const OP_WRITE: u8 = 5;
const OP_UDP_BIND: u8 = 6;
const STATUS_OK: u8 = 0;
const STATUS_WOULD_BLOCK: u8 = 3;
const STATUS_IO: u8 = 4;

#[inline]
pub(crate) fn is_cross_vm_ip(local_ip: [u8; 4]) -> bool {
    crate::os::entry_pure::is_cross_vm_ip(local_ip)
}

#[inline]
pub(crate) fn next_nonce(n: &mut u64) -> u64 {
    crate::os::entry_pure::next_nonce(n)
}

#[inline]
fn nonce_matches(buf: &[u8; 512], n: usize, nonce: u64) -> bool {
    if n < 13 {
        return false;
    }
    let mut b = [0u8; 8];
    b.copy_from_slice(&buf[n - 8..n]);
    u64::from_le_bytes(b) == nonce
}

pub(crate) fn rpc_nonce(
    pending: &mut nexus_ipc::reqrep::ReplyBuffer<16, 512>,
    net: &nexus_ipc::KernelClient,
    req: &[u8],
    expect_rsp_op: u8,
    nonce: u64,
) -> core::result::Result<[u8; 512], ()> {
    use nexus_ipc::Client as _;
    use nexus_ipc::IpcError as IpcErrorLite;
    use nexus_ipc::Wait;

    // Prefer CAP_MOVE replies (dedicated reply inbox) when available. In some bring-up harnesses
    // the fixed reply slots may not be present; fall back to normal send/recv on the netstackd
    // endpoint slots (still nonce-correlated, still deterministic).
    let reply_send_slot = DSOFT_REPLY_SEND_SLOT;
    let (net_send_slot, net_recv_slot) = net.slots();
    let mut use_cap_move = true;
    let mut reply_recv_slot = DSOFT_REPLY_RECV_SLOT;

    static CAP_CLONE_FAIL_LOGGED_NONCE: core::sync::atomic::AtomicBool =
        core::sync::atomic::AtomicBool::new(false);
    let reply_send_clone = match nexus_abi::cap_clone(reply_send_slot) {
        Ok(slot) => slot,
        Err(_) => {
            use_cap_move = false;
            reply_recv_slot = net_recv_slot;
            0
        }
    };
    if !use_cap_move
        && !CAP_CLONE_FAIL_LOGGED_NONCE.swap(true, core::sync::atomic::Ordering::Relaxed)
    {
        let _ = nexus_abi::debug_println("dsoftbusd: cap clone missing; fallback to direct recv");
    }

    let wait = Wait::Timeout(core::time::Duration::from_millis(20));
    let mut sent = false;
    for _ in 0..64 {
        let r = if use_cap_move {
            net.send_with_cap_move_wait(req, reply_send_clone, wait)
        } else {
            net.send(req, wait)
        };
        match r {
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
                if use_cap_move {
                    let _ = nexus_abi::cap_close(reply_send_clone);
                }
                return Err(());
            }
        }
    }
    if !sent {
        if use_cap_move {
            let _ = nexus_abi::cap_close(reply_send_clone);
        }
        return Err(());
    }
    if use_cap_move {
        let _ = nexus_abi::cap_close(reply_send_clone);
    } else {
        let _ = net_send_slot;
    }

    // If the reply already arrived out-of-order, return it from the pending buffer first.
    {
        let mut tmp = [0u8; 512];
        if let Some(n) = pending.take_into(nonce, &mut tmp) {
            if n >= 5
                && tmp[0] == MAGIC0
                && tmp[1] == MAGIC1
                && tmp[2] == VERSION
                && tmp[3] == expect_rsp_op
                && nonce_matches(&tmp, n, nonce)
            {
                return Ok(tmp);
            }
        }
    }

    let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
    let mut buf = [0u8; 512];
    let start = nexus_abi::nsec().ok().unwrap_or(0);
    let deadline = start.saturating_add(500_000_000); // 500ms
    loop {
        let now = nexus_abi::nsec().ok().unwrap_or(0);
        if now >= deadline {
            break;
        }
        match nexus_abi::ipc_recv_v1(
            reply_recv_slot,
            &mut hdr,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => {
                let n = n as usize;
                if n >= 5
                    && buf[0] == MAGIC0
                    && buf[1] == MAGIC1
                    && buf[2] == VERSION
                    && buf[3] == expect_rsp_op
                    && nonce_matches(&buf, n, nonce)
                {
                    return Ok(buf);
                }
                if n >= 13 && buf[0] == MAGIC0 && buf[1] == MAGIC1 && buf[2] == VERSION {
                    let mut b = [0u8; 8];
                    b.copy_from_slice(&buf[n - 8..n]);
                    let other = u64::from_le_bytes(b);
                    let _ = pending.push(other, &buf[..n]);
                }
            }
            Err(nexus_abi::IpcError::QueueEmpty) => {
                let _ = nexus_abi::yield_();
            }
            Err(_) => return Err(()),
        }
    }
    Err(())
}

pub(crate) fn get_local_ip(
    pending: &mut nexus_ipc::reqrep::ReplyBuffer<16, 512>,
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
    let rsp = match rpc_nonce(pending, net, &req, OP_LOCAL_ADDR | 0x80, nonce) {
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

pub(crate) fn wait_for_slots_ready() {
    let _ = nexus_abi::debug_println("dsoftbusd: waiting for slots");
    for _ in 0..10_000 {
        if let Ok(cloned) = nexus_abi::cap_clone(DSOFT_REPLY_SEND_SLOT) {
            let _ = nexus_abi::cap_close(cloned);
            break;
        }
        let _ = nexus_abi::yield_();
    }
}

pub(crate) fn init_netstack_client() -> core::result::Result<nexus_ipc::KernelClient, ()> {
    let _ = nexus_abi::debug_println("dsoftbusd: entry");
    match nexus_ipc::KernelClient::new_with_slots(0x3, 0x4) {
        Ok(c) => Ok(c),
        Err(_) => {
            let _ = nexus_abi::debug_println("dsoftbusd: netstackd slots fail");
            Err(())
        }
    }
}

pub(crate) fn resolve_local_ip_with_wait(
    pending: &mut nexus_ipc::reqrep::ReplyBuffer<16, 512>,
    net: &nexus_ipc::KernelClient,
    nonce_ctr: &mut u64,
) -> [u8; 4] {
    let _ = nexus_abi::debug_println("dsoftbusd: waiting for local ip");
    let mut local_ip = DEFAULT_LOCAL_IP;
    let mut local_ip_resolved = false;
    for i in 0..300u32 {
        if let Some(ip) = get_local_ip(pending, net, nonce_ctr, i) {
            local_ip = ip;
            local_ip_resolved = true;
            break;
        }
        if i % 50 == 0 && i > 0 {
            let _ = nexus_abi::debug_println("dsoftbusd: local ip wait");
        }
        for _ in 0..500 {
            let _ = nexus_abi::yield_();
        }
    }
    if !local_ip_resolved {
        let _ = nexus_abi::debug_println("dsoftbusd: local ip fallback");
    } else {
        let _ = nexus_abi::debug_println("dsoftbusd: local ip ok");
    }
    let _ = nexus_abi::debug_println("dsoftbusd: ip phase done");
    local_ip
}

pub(crate) fn bind_discovery_udp_with_wait(
    pending: &mut nexus_ipc::reqrep::ReplyBuffer<16, 512>,
    net: &nexus_ipc::KernelClient,
    nonce_ctr: &mut u64,
    disc_port: u16,
) -> u32 {
    let _ = nexus_abi::debug_println("dsoftbusd: udp bind begin");
    let mut req = [0u8; 18];
    req[0] = MAGIC0;
    req[1] = MAGIC1;
    req[2] = VERSION;
    req[3] = OP_UDP_BIND;
    req[4..8].copy_from_slice(&[0, 0, 0, 0]); // 0.0.0.0
    req[8..10].copy_from_slice(&disc_port.to_le_bytes());
    let mut bind_rsp = None;
    let mut bind_err_logged = false;
    for _ in 0..500 {
        let nonce = next_nonce(nonce_ctr);
        req[10..18].copy_from_slice(&nonce.to_le_bytes());
        match rpc_nonce(pending, net, &req, OP_UDP_BIND | 0x80, nonce) {
            Ok(rsp) => {
                if rsp[0] == MAGIC0
                    && rsp[1] == MAGIC1
                    && rsp[2] == VERSION
                    && rsp[3] == (OP_UDP_BIND | 0x80)
                    && rsp[4] == STATUS_OK
                {
                    bind_rsp = Some(rsp);
                    break;
                }
                if !bind_err_logged {
                    bind_err_logged = true;
                    let _ = nexus_abi::debug_println("dsoftbusd: udp bind FAIL");
                }
            }
            Err(_) => {
                if !bind_err_logged {
                    bind_err_logged = true;
                    let _ = nexus_abi::debug_println("dsoftbusd: udp bind rpc err");
                }
            }
        }
        let _ = nexus_abi::yield_();
    }
    let Some(rsp) = bind_rsp else {
        let _ = nexus_abi::debug_println("dsoftbusd: udp bind rpc timeout");
        loop {
            let _ = nexus_abi::yield_();
        }
    };
    let _ = nexus_abi::debug_println("dsoftbusd: udp bind ok");
    let udp_id = u32::from_le_bytes([rsp[5], rsp[6], rsp[7], rsp[8]]);
    let _ = nexus_abi::debug_println("dsoftbusd: discovery up (udp loopback)");
    udp_id
}

pub(crate) fn listen_with_retry(
    pending: &mut nexus_ipc::reqrep::ReplyBuffer<16, 512>,
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
        let rsp = rpc_nonce(pending, net, &req, OP_LISTEN | 0x80, nonce)?;
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
            let _ = nexus_abi::debug_println("dsoftbusd: listen FAIL");
            loop {
                let _ = nexus_abi::yield_();
            }
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
    pending: &mut nexus_ipc::reqrep::ReplyBuffer<16, 512>,
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
        let rsp = rpc_nonce(pending, net, &c, OP_CONNECT | 0x80, nonce)?;
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
    pending: &mut nexus_ipc::reqrep::ReplyBuffer<16, 512>,
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
        let rsp = rpc_nonce(pending, net, &a, OP_ACCEPT | 0x80, nonce)?;
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
    pending: &mut nexus_ipc::reqrep::ReplyBuffer<16, 512>,
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
        let rsp = rpc_nonce(pending, net, &r, OP_READ | 0x80, nonce)?;
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
    pending: &mut nexus_ipc::reqrep::ReplyBuffer<16, 512>,
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
    let rsp = rpc_nonce(pending, net, &w[..10 + data.len() + 8], OP_WRITE | 0x80, nonce)?;
    if rsp[4] == STATUS_OK {
        Ok(())
    } else {
        Err(())
    }
}

pub(crate) fn stream_read(
    pending: &mut nexus_ipc::reqrep::ReplyBuffer<16, 512>,
    net: &nexus_ipc::KernelClient,
    nonce_ctr: &mut u64,
    sid: u32,
    buf: &mut [u8],
) -> core::result::Result<(), ()> {
    dual_stream_read(pending, net, nonce_ctr, sid, buf)
}

pub(crate) fn stream_write(
    pending: &mut nexus_ipc::reqrep::ReplyBuffer<16, 512>,
    net: &nexus_ipc::KernelClient,
    nonce_ctr: &mut u64,
    sid: u32,
    data: &[u8],
) -> core::result::Result<(), ()> {
    dual_stream_write(pending, net, nonce_ctr, sid, data)
}
