// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The gateway's netstackd client over init's FIXED slots (a seam
//! never routes from its hot loop): request SEND on slot 8, replies on the
//! CAP_MOVE inbox 5/6. One RPC at a time, bounded by a deadline, foreign
//! frames on the inbox skipped — the same dance every facade client does,
//! without allocation. Wire = netstackd v1 (`N`,`S`), including the
//! RFC-0092 `OP_PEER_ADDR`.
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! TEST_COVERAGE: QEMU (`ingressd: port open`, `SELFTEST: ingress allow ok`)

use nexus_abi::{IpcError, MsgHeader};

use super::slots::{NETSTACKD_SEND_SLOT, REPLY_RECV_SLOT, REPLY_SEND_SLOT};

const MAGIC0: u8 = b'N';
const MAGIC1: u8 = b'S';
const VERSION: u8 = 1;

const OP_LISTEN: u8 = 1;
const OP_ACCEPT: u8 = 2;
const OP_CONNECT: u8 = 3;
const OP_READ: u8 = 4;
const OP_WRITE: u8 = 5;
const OP_CLOSE: u8 = 11;
const OP_PEER_ADDR: u8 = 13;

const STATUS_OK: u8 = 0;
const STATUS_NOT_FOUND: u8 = 1;
const STATUS_WOULD_BLOCK: u8 = 3;
const STATUS_TIMED_OUT: u8 = 5;
const STATUS_DENY: u8 = 6;

/// One facade RPC payload (`OP_READ`/`OP_WRITE` carry ≤ 480 bytes).
pub(crate) const MAX_IO_BYTES: usize = 480;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NetErr {
    WouldBlock,
    /// The seam refused the tuple (policyd).
    Deny,
    /// The stream is gone.
    Closed,
    Io,
    Timeout,
}

fn header(op: u8, out: &mut [u8]) {
    out[0] = MAGIC0;
    out[1] = MAGIC1;
    out[2] = VERSION;
    out[3] = op;
}

fn status_err(status: u8) -> NetErr {
    match status {
        STATUS_WOULD_BLOCK => NetErr::WouldBlock,
        STATUS_DENY => NetErr::Deny,
        STATUS_NOT_FOUND => NetErr::Closed,
        STATUS_TIMED_OUT => NetErr::Timeout,
        _ => NetErr::Io,
    }
}

/// Sends `req` with a CAP_MOVE reply cap and waits for the `op|0x80` reply.
fn rpc(req: &[u8], op: u8, out: &mut [u8]) -> Result<usize, NetErr> {
    // One waited exchange (TASK-0324 P7-d): the send waits for queue space, the receive for
    // netstackd's answer or its death (EOF) — no clock. Foreign inbox frames are skipped.
    let reply_cap = nexus_abi::cap_clone(REPLY_SEND_SLOT).map_err(|_| NetErr::Io)?;
    let hdr = MsgHeader::new(reply_cap, 0, 0, nexus_abi::ipc_hdr::CAP_MOVE, req.len() as u32);
    if nexus_abi::ipc_send_v1(NETSTACKD_SEND_SLOT, &hdr, req, 0, 0).is_err() {
        let _ = nexus_abi::cap_close(reply_cap);
        return Err(NetErr::Io);
    }
    loop {
        let mut rh = MsgHeader::new(0, 0, 0, 0, 0);
        match nexus_abi::ipc_recv_v1(
            REPLY_RECV_SLOT,
            &mut rh,
            out,
            nexus_abi::IPC_SYS_TRUNCATE | nexus_abi::IPC_SYS_EOF,
            0,
        ) {
            Ok(n) => {
                let n = (n as usize).min(out.len());
                if n >= 5 && out[0] == MAGIC0 && out[1] == MAGIC1 && out[3] == (op | 0x80) {
                    return if out[4] == STATUS_OK { Ok(n) } else { Err(status_err(out[4])) };
                }
            }
            Err(IpcError::PeerClosed) => return Err(NetErr::Timeout),
            Err(_) => return Err(NetErr::Io),
        }
    }
}

fn u32_at(buf: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes([*buf.get(i)?, *buf.get(i + 1)?, *buf.get(i + 2)?, *buf.get(i + 3)?]))
}

/// `listen(ip:port)` → listener id.
pub(crate) fn listen(ip: [u8; 4], port: u16) -> Result<u32, NetErr> {
    let mut req = [0u8; 10];
    header(OP_LISTEN, &mut req);
    req[4..8].copy_from_slice(&ip);
    req[8..10].copy_from_slice(&port.to_le_bytes());
    let mut out = [0u8; 32];
    let n = rpc(&req, OP_LISTEN, &mut out)?;
    u32_at(&out[..n], 5).filter(|id| *id != 0).ok_or(NetErr::Io)
}

/// `accept(listener)` → stream id (`WouldBlock` when nothing is pending).
pub(crate) fn accept(listener: u32) -> Result<u32, NetErr> {
    let mut req = [0u8; 8];
    header(OP_ACCEPT, &mut req);
    req[4..8].copy_from_slice(&listener.to_le_bytes());
    let mut out = [0u8; 32];
    let n = rpc(&req, OP_ACCEPT, &mut out)?;
    u32_at(&out[..n], 5).filter(|id| *id != 0).ok_or(NetErr::Io)
}

/// `connect(ip:port)` → stream id.
pub(crate) fn connect(ip: [u8; 4], port: u16) -> Result<u32, NetErr> {
    let mut req = [0u8; 10];
    header(OP_CONNECT, &mut req);
    req[4..8].copy_from_slice(&ip);
    req[8..10].copy_from_slice(&port.to_le_bytes());
    let mut out = [0u8; 32];
    let n = rpc(&req, OP_CONNECT, &mut out)?;
    u32_at(&out[..n], 5).filter(|id| *id != 0).ok_or(NetErr::Io)
}

/// Reads up to `buf.len()` (≤ 480) bytes; `Ok(0)` is end-of-stream.
pub(crate) fn read(stream: u32, buf: &mut [u8]) -> Result<usize, NetErr> {
    let max = buf.len().min(MAX_IO_BYTES) as u16;
    let mut req = [0u8; 10];
    header(OP_READ, &mut req);
    req[4..8].copy_from_slice(&stream.to_le_bytes());
    req[8..10].copy_from_slice(&max.to_le_bytes());
    let mut out = [0u8; 512];
    let n = rpc(&req, OP_READ, &mut out)?;
    if n < 7 {
        return Err(NetErr::Io);
    }
    let len = usize::from(u16::from_le_bytes([out[5], out[6]]));
    if 7 + len > n || len > buf.len() {
        return Err(NetErr::Io);
    }
    buf[..len].copy_from_slice(&out[7..7 + len]);
    Ok(len)
}

/// Writes ≤ 480 bytes; returns how many the stack took.
pub(crate) fn write(stream: u32, data: &[u8]) -> Result<usize, NetErr> {
    let len = data.len().min(MAX_IO_BYTES);
    let mut req = [0u8; 10 + MAX_IO_BYTES];
    header(OP_WRITE, &mut req);
    req[4..8].copy_from_slice(&stream.to_le_bytes());
    req[8..10].copy_from_slice(&(len as u16).to_le_bytes());
    req[10..10 + len].copy_from_slice(&data[..len]);
    let mut out = [0u8; 32];
    let n = rpc(&req[..10 + len], OP_WRITE, &mut out)?;
    if n < 7 {
        return Err(NetErr::Io);
    }
    let wrote = usize::from(u16::from_le_bytes([out[5], out[6]]));
    if wrote == 0 {
        return Err(NetErr::WouldBlock);
    }
    Ok(wrote.min(len))
}

/// Closes a stream (best-effort; a gone stream is already closed).
pub(crate) fn close(stream: u32) {
    let mut req = [0u8; 8];
    header(OP_CLOSE, &mut req);
    req[4..8].copy_from_slice(&stream.to_le_bytes());
    let mut out = [0u8; 32];
    let _ = rpc(&req, OP_CLOSE, &mut out);
}

/// The remote `(ip, port)` of an accepted stream (RFC-0092 `OP_PEER_ADDR`).
pub(crate) fn peer_addr(stream: u32) -> Result<([u8; 4], u16), NetErr> {
    let mut req = [0u8; 8];
    header(OP_PEER_ADDR, &mut req);
    req[4..8].copy_from_slice(&stream.to_le_bytes());
    let mut out = [0u8; 32];
    let n = rpc(&req, OP_PEER_ADDR, &mut out)?;
    if n < 11 {
        return Err(NetErr::Io);
    }
    Ok(([out[5], out[6], out[7], out[8]], u16::from_le_bytes([out[9], out[10]])))
}
