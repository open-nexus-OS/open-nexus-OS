//! Bounded stream and UDP/TCP helper operations for cross-VM path.

use super::ids::{ListenerId, SessionId, UdpSocketId};
use super::rpc::{next_nonce, rpc_nonce};
use super::validate::{
    is_valid_udp_payload_len, parse_read_ok_len, parse_status_frame, parse_write_ok_wrote,
};
use nexus_ipc::reqrep::ReplyBuffer;
use nexus_ipc::KernelClient;

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
const OP_WAIT_WRITABLE: u8 = 12;
const OP_CLOSE: u8 = 11;
const STATUS_OK: u8 = 0;
pub(crate) const STATUS_WOULD_BLOCK: u8 = 3;
pub(crate) const STATUS_IO: u8 = 4;
const STREAM_WOULD_BLOCK_BUDGET: u32 = 1_024;

pub(crate) fn stream_write_all(
    pending: &mut ReplyBuffer<16, 512>,
    nonce_ctr: &mut u64,
    net: &KernelClient,
    sid: SessionId,
    data: &[u8],
    reply_recv_slot: u32,
    reply_send_slot: u32,
) -> core::result::Result<(), ()> {
    fn stream_wait_writable(
        pending: &mut ReplyBuffer<16, 512>,
        nonce_ctr: &mut u64,
        net: &KernelClient,
        sid: SessionId,
        reply_recv_slot: u32,
        reply_send_slot: u32,
    ) -> core::result::Result<bool, ()> {
        let nonce = next_nonce(nonce_ctr);
        let mut req = [0u8; 16];
        req[0] = MAGIC0;
        req[1] = MAGIC1;
        req[2] = VERSION;
        req[3] = OP_WAIT_WRITABLE;
        req[4..8].copy_from_slice(&sid.as_raw().to_le_bytes());
        req[8..16].copy_from_slice(&nonce.to_le_bytes());
        let rsp = rpc_nonce(
            pending,
            net,
            &req,
            OP_WAIT_WRITABLE | 0x80,
            nonce,
            reply_recv_slot,
            reply_send_slot,
        )?;
        let status = parse_status_frame(&rsp, OP_WAIT_WRITABLE | 0x80)?;
        if status == STATUS_OK {
            return Ok(true);
        }
        if status == STATUS_WOULD_BLOCK {
            return Ok(false);
        }
        Err(())
    }

    let mut off = 0usize;
    let mut would_block_spins: u32 = 0;
    while off < data.len() {
        if would_block_spins == 0 || (would_block_spins & 0xff) == 0 {
            match stream_wait_writable(
                pending,
                nonce_ctr,
                net,
                sid,
                reply_recv_slot,
                reply_send_slot,
            ) {
                Ok(true) => {}
                Ok(false) => {
                    would_block_spins = would_block_spins.wrapping_add(1);
                    if would_block_spins >= STREAM_WOULD_BLOCK_BUDGET {
                        let _ = nexus_abi::debug_println("dbg:dsoftbusd: stream write timeout");
                        return Err(());
                    }
                    let _ = nexus_abi::yield_();
                    continue;
                }
                Err(()) => return Err(()),
            }
        }
        let chunk = core::cmp::min(480, data.len() - off);
        let nonce = next_nonce(nonce_ctr);
        let mut w = [0u8; 512 + 8];
        w[0] = MAGIC0;
        w[1] = MAGIC1;
        w[2] = VERSION;
        w[3] = OP_WRITE;
        w[4..8].copy_from_slice(&sid.as_raw().to_le_bytes());
        w[8..10].copy_from_slice(&(chunk as u16).to_le_bytes());
        w[10..10 + chunk].copy_from_slice(&data[off..off + chunk]);
        w[10 + chunk..10 + chunk + 8].copy_from_slice(&nonce.to_le_bytes());
        let rsp = rpc_nonce(
            pending,
            net,
            &w[..10 + chunk + 8],
            OP_WRITE | 0x80,
            nonce,
            reply_recv_slot,
            reply_send_slot,
        )?;
        let status = parse_status_frame(&rsp, OP_WRITE | 0x80)?;
        if status == STATUS_OK {
            let wrote = parse_write_ok_wrote(&rsp)?;
            would_block_spins = 0;
            off = off.saturating_add(wrote);
            continue;
        }
        if status == STATUS_WOULD_BLOCK {
            would_block_spins = would_block_spins.wrapping_add(1);
            if would_block_spins >= STREAM_WOULD_BLOCK_BUDGET {
                let _ = nexus_abi::debug_println("dbg:dsoftbusd: stream write timeout");
                return Err(());
            }
            let _ = nexus_abi::yield_();
            continue;
        }
        return Err(());
    }
    Ok(())
}

pub(crate) fn stream_read_exact(
    pending: &mut ReplyBuffer<16, 512>,
    nonce_ctr: &mut u64,
    net: &KernelClient,
    sid: SessionId,
    out: &mut [u8],
    reply_recv_slot: u32,
    reply_send_slot: u32,
) -> core::result::Result<(), ()> {
    let mut off = 0usize;
    let mut would_block_spins: u32 = 0;
    while off < out.len() {
        let want = core::cmp::min(460, out.len() - off);
        let nonce = next_nonce(nonce_ctr);
        let mut r = [0u8; 18];
        r[0] = MAGIC0;
        r[1] = MAGIC1;
        r[2] = VERSION;
        r[3] = OP_READ;
        r[4..8].copy_from_slice(&sid.as_raw().to_le_bytes());
        r[8..10].copy_from_slice(&(want as u16).to_le_bytes());
        r[10..18].copy_from_slice(&nonce.to_le_bytes());
        let rsp = rpc_nonce(
            pending,
            net,
            &r,
            OP_READ | 0x80,
            nonce,
            reply_recv_slot,
            reply_send_slot,
        )?;
        let status = parse_status_frame(&rsp, OP_READ | 0x80)?;
        if status == STATUS_OK {
            let n = parse_read_ok_len(&rsp)?;
            would_block_spins = 0;
            out[off..off + n].copy_from_slice(&rsp[7..7 + n]);
            off += n;
            continue;
        }
        if status == STATUS_WOULD_BLOCK {
            would_block_spins = would_block_spins.wrapping_add(1);
            if would_block_spins >= STREAM_WOULD_BLOCK_BUDGET {
                let _ = nexus_abi::debug_println("dbg:dsoftbusd: stream read timeout");
                return Err(());
            }
            let _ = nexus_abi::yield_();
            continue;
        }
        return Err(());
    }
    Ok(())
}

pub(crate) fn udp_bind(
    pending: &mut ReplyBuffer<16, 512>,
    nonce_ctr: &mut u64,
    net: &KernelClient,
    ip: [u8; 4],
    port: u16,
    reply_recv_slot: u32,
    reply_send_slot: u32,
) -> core::result::Result<UdpSocketId, ()> {
    let nonce = next_nonce(nonce_ctr);
    let mut req = [0u8; 18];
    req[0] = MAGIC0;
    req[1] = MAGIC1;
    req[2] = VERSION;
    req[3] = OP_UDP_BIND;
    req[4..8].copy_from_slice(&ip);
    req[8..10].copy_from_slice(&port.to_le_bytes());
    req[10..18].copy_from_slice(&nonce.to_le_bytes());
    let rsp = rpc_nonce(
        pending,
        net,
        &req,
        OP_UDP_BIND | 0x80,
        nonce,
        reply_recv_slot,
        reply_send_slot,
    )
    .map_err(|_| ())?;
    let status = parse_status_frame(&rsp, OP_UDP_BIND | 0x80).map_err(|_| ())?;
    if status != STATUS_OK {
        return Err(());
    }
    Ok(UdpSocketId::from_raw(u32::from_le_bytes([rsp[5], rsp[6], rsp[7], rsp[8]])))
}

pub(crate) fn udp_send_to(
    pending: &mut ReplyBuffer<16, 512>,
    nonce_ctr: &mut u64,
    net: &KernelClient,
    udp_id: UdpSocketId,
    ip: [u8; 4],
    port: u16,
    payload: &[u8],
    reply_recv_slot: u32,
    reply_send_slot: u32,
) -> core::result::Result<(), ()> {
    if !is_valid_udp_payload_len(payload.len()) {
        return Err(());
    }
    let nonce = next_nonce(nonce_ctr);
    let mut send = [0u8; 16 + 256 + 8];
    send[0] = MAGIC0;
    send[1] = MAGIC1;
    send[2] = VERSION;
    send[3] = OP_UDP_SEND_TO;
    send[4..8].copy_from_slice(&udp_id.as_raw().to_le_bytes());
    send[8..12].copy_from_slice(&ip);
    send[12..14].copy_from_slice(&port.to_le_bytes());
    send[14..16].copy_from_slice(&(payload.len() as u16).to_le_bytes());
    send[16..16 + payload.len()].copy_from_slice(payload);
    let end = 16 + payload.len();
    send[end..end + 8].copy_from_slice(&nonce.to_le_bytes());
    let rsp = rpc_nonce(
        pending,
        net,
        &send[..end + 8],
        OP_UDP_SEND_TO | 0x80,
        nonce,
        reply_recv_slot,
        reply_send_slot,
    )?;
    let status = parse_status_frame(&rsp, OP_UDP_SEND_TO | 0x80)?;
    if status == STATUS_OK || status == STATUS_WOULD_BLOCK {
        return Ok(());
    }
    Err(())
}

pub(crate) fn tcp_listen(
    pending: &mut ReplyBuffer<16, 512>,
    nonce_ctr: &mut u64,
    net: &KernelClient,
    ip: [u8; 4],
    port: u16,
    reply_recv_slot: u32,
    reply_send_slot: u32,
) -> core::result::Result<ListenerId, ()> {
    let nonce = next_nonce(nonce_ctr);
    let mut req = [0u8; 18];
    req[0] = MAGIC0;
    req[1] = MAGIC1;
    req[2] = VERSION;
    req[3] = OP_LISTEN;
    req[4..8].copy_from_slice(&ip);
    req[8..10].copy_from_slice(&port.to_le_bytes());
    req[10..18].copy_from_slice(&nonce.to_le_bytes());
    let rsp = rpc_nonce(
        pending,
        net,
        &req,
        OP_LISTEN | 0x80,
        nonce,
        reply_recv_slot,
        reply_send_slot,
    )?;
    let status = parse_status_frame(&rsp, OP_LISTEN | 0x80)?;
    if status != STATUS_OK {
        return Err(());
    }
    Ok(ListenerId::from_raw(u32::from_le_bytes([rsp[5], rsp[6], rsp[7], rsp[8]])))
}

pub(crate) fn tcp_connect(
    pending: &mut ReplyBuffer<16, 512>,
    nonce_ctr: &mut u64,
    net: &KernelClient,
    ip: [u8; 4],
    port: u16,
    reply_recv_slot: u32,
    reply_send_slot: u32,
) -> core::result::Result<SessionId, u8> {
    let nonce = next_nonce(nonce_ctr);
    let mut c = [0u8; 18];
    c[0] = MAGIC0;
    c[1] = MAGIC1;
    c[2] = VERSION;
    c[3] = OP_CONNECT;
    c[4..8].copy_from_slice(&ip);
    c[8..10].copy_from_slice(&port.to_le_bytes());
    c[10..18].copy_from_slice(&nonce.to_le_bytes());
    let rsp = rpc_nonce(
        pending,
        net,
        &c,
        OP_CONNECT | 0x80,
        nonce,
        reply_recv_slot,
        reply_send_slot,
    )
    .map_err(|_| 0xfd)?;
    let status = parse_status_frame(&rsp, OP_CONNECT | 0x80).map_err(|_| 0xfe)?;
    if status == STATUS_OK {
        return Ok(SessionId::from_raw(u32::from_le_bytes([rsp[5], rsp[6], rsp[7], rsp[8]])));
    }
    Err(status)
}

pub(crate) fn tcp_accept(
    pending: &mut ReplyBuffer<16, 512>,
    nonce_ctr: &mut u64,
    net: &KernelClient,
    lid: ListenerId,
    reply_recv_slot: u32,
    reply_send_slot: u32,
) -> core::result::Result<SessionId, ()> {
    let nonce = next_nonce(nonce_ctr);
    let mut a = [0u8; 16];
    a[0] = MAGIC0;
    a[1] = MAGIC1;
    a[2] = VERSION;
    a[3] = OP_ACCEPT;
    a[4..8].copy_from_slice(&lid.as_raw().to_le_bytes());
    a[8..16].copy_from_slice(&nonce.to_le_bytes());
    let rsp = rpc_nonce(
        pending,
        net,
        &a,
        OP_ACCEPT | 0x80,
        nonce,
        reply_recv_slot,
        reply_send_slot,
    )?;
    let status = parse_status_frame(&rsp, OP_ACCEPT | 0x80)?;
    if status == STATUS_OK {
        return Ok(SessionId::from_raw(u32::from_le_bytes([rsp[5], rsp[6], rsp[7], rsp[8]])));
    }
    if status == STATUS_WOULD_BLOCK {
        return Err(());
    }
    Err(())
}

pub(crate) fn tcp_close(
    pending: &mut ReplyBuffer<16, 512>,
    nonce_ctr: &mut u64,
    net: &KernelClient,
    sid: SessionId,
    reply_recv_slot: u32,
    reply_send_slot: u32,
) -> core::result::Result<(), ()> {
    let nonce = next_nonce(nonce_ctr);
    let mut c = [0u8; 16];
    c[0] = MAGIC0;
    c[1] = MAGIC1;
    c[2] = VERSION;
    c[3] = OP_CLOSE;
    c[4..8].copy_from_slice(&sid.as_raw().to_le_bytes());
    c[8..16].copy_from_slice(&nonce.to_le_bytes());
    let rsp = rpc_nonce(
        pending,
        net,
        &c,
        OP_CLOSE | 0x80,
        nonce,
        reply_recv_slot,
        reply_send_slot,
    )?;
    let status = parse_status_frame(&rsp, OP_CLOSE | 0x80)?;
    if status == STATUS_OK {
        return Ok(());
    }
    Err(())
}

pub(crate) struct CrossVmTransport<'a> {
    pending: &'a mut ReplyBuffer<16, 512>,
    nonce_ctr: &'a mut u64,
    net: &'a KernelClient,
    reply_recv_slot: u32,
    reply_send_slot: u32,
}

impl<'a> CrossVmTransport<'a> {
    pub(crate) fn new(
        pending: &'a mut ReplyBuffer<16, 512>,
        nonce_ctr: &'a mut u64,
        net: &'a KernelClient,
        reply_recv_slot: u32,
        reply_send_slot: u32,
    ) -> Self {
        Self {
            pending,
            nonce_ctr,
            net,
            reply_recv_slot,
            reply_send_slot,
        }
    }

    pub(crate) fn connect(&mut self, ip: [u8; 4], port: u16) -> core::result::Result<SessionId, u8> {
        tcp_connect(
            self.pending,
            self.nonce_ctr,
            self.net,
            ip,
            port,
            self.reply_recv_slot,
            self.reply_send_slot,
        )
    }

    pub(crate) fn accept(&mut self, lid: ListenerId) -> core::result::Result<SessionId, ()> {
        tcp_accept(
            self.pending,
            self.nonce_ctr,
            self.net,
            lid,
            self.reply_recv_slot,
            self.reply_send_slot,
        )
    }

    pub(crate) fn close(&mut self, sid: SessionId) -> core::result::Result<(), ()> {
        tcp_close(
            self.pending,
            self.nonce_ctr,
            self.net,
            sid,
            self.reply_recv_slot,
            self.reply_send_slot,
        )
    }

    pub(crate) fn write_all(&mut self, sid: SessionId, data: &[u8]) -> core::result::Result<(), ()> {
        stream_write_all(
            self.pending,
            self.nonce_ctr,
            self.net,
            sid,
            data,
            self.reply_recv_slot,
            self.reply_send_slot,
        )
    }

    pub(crate) fn read_exact(&mut self, sid: SessionId, out: &mut [u8]) -> core::result::Result<(), ()> {
        stream_read_exact(
            self.pending,
            self.nonce_ctr,
            self.net,
            sid,
            out,
            self.reply_recv_slot,
            self.reply_send_slot,
        )
    }
}
