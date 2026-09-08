// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: TASK-0052 P3 (RFC-0092 Layer B) proofs through the real gateway
//! and the real facade. The selftest owns three declared exposures
//! (`policies/base.toml`), listens on their loopback backends, registers
//! each intent with ingressd by kernel identity, then connects to its own
//! exposed ports over the interface address (the facade hairpins onto the
//! gateway's NIC-facing listener):
//! - 8080: bytes cross gateway → backend and back (`ingress allow ok`);
//! - an undeclared port: the intent is refused, reason=policy
//!   (`ingress intent deny ok`);
//! - 8081: the allow-list excludes the interface address, the gateway closes
//!   the accepted connection — observed as end-of-stream
//!   (`ingress cidr deny ok`);
//! - 8082: burst 2 at 1/s — the third connection is closed
//!   (`ingress rate ok`).
//! Every marker is emitted only after the observed behaviour; each step is
//! deadline-bounded.
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! TEST_COVERAGE: QEMU (`SELFTEST: ingress allow|intent deny|cidr deny|rate ok`);
//!   host tests/ingress_host/ (the same verdicts on the core)
//! RFC: docs/rfcs/RFC-0092-service-exposure-contract-ingress-policy-ingressd.md

use nexus_abi::yield_;
use nexus_ipc::KernelClient;

use crate::markers::emit_line;
use crate::os_lite::ipc::clients::{cached_netstackd_client, cached_reply_client};

// netstackd wire v1.
const NS_MAGIC0: u8 = b'N';
const NS_MAGIC1: u8 = b'S';
const NS_VERSION: u8 = 1;
const OP_LISTEN: u8 = 1;
const OP_ACCEPT: u8 = 2;
const OP_CONNECT: u8 = 3;
const OP_READ: u8 = 4;
const OP_WRITE: u8 = 5;
const OP_CLOSE: u8 = 11;
const NS_STATUS_OK: u8 = 0;
const NS_STATUS_WOULD_BLOCK: u8 = 3;

// ingressd wire v1 (RFC-0092 §3).
const IG_MAGIC0: u8 = b'I';
const IG_MAGIC1: u8 = b'G';
const IG_VERSION: u8 = 1;
const OP_EXPOSE: u8 = 1;
const PROTO_TCP: u8 = 0;
const IG_STATUS_ALLOW: u8 = 0;
const IG_STATUS_DENY: u8 = 1;
const REASON_POLICY: u8 = 1;

/// Declared in `policies/base.toml` (`[[expose."selftest-client"]]`).
const ALLOW_PORT: u16 = 8080;
const ALLOW_BACKEND: u16 = 18_080;
const CIDR_PORT: u16 = 8081;
const CIDR_BACKEND: u16 = 18_081;
const RATE_PORT: u16 = 8082;
const RATE_BACKEND: u16 = 18_082;
/// Not declared anywhere: the intent must be refused.
const UNDECLARED_PORT: u16 = 8099;

const LOOPBACK: [u8; 4] = [127, 0, 0, 1];
/// Per-step bound (the gateway polls accepts every 20 ms).
const STEP_DEADLINE_NS: u64 = 3_000_000_000;

fn now_ns() -> u64 {
    nexus_abi::nsec().unwrap_or(0)
}

/// One CAP_MOVE request/reply on the selftest's `@reply` inbox; foreign
/// frames are skipped; `status` is the facade's byte 4.
fn net_rpc(net: &KernelClient, req: &[u8], op: u8, out: &mut [u8]) -> Result<usize, ()> {
    let reply = cached_reply_client().map_err(|_| ())?;
    let (reply_send_slot, reply_recv_slot) = reply.slots();
    let reply_send_clone = nexus_abi::cap_clone(reply_send_slot).map_err(|_| ())?;
    net.send_with_cap_move(req, reply_send_clone).map_err(|_| ())?;
    let deadline = now_ns().saturating_add(STEP_DEADLINE_NS);
    loop {
        let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        match nexus_abi::ipc_recv_v1(
            reply_recv_slot,
            &mut hdr,
            out,
            nexus_abi::IPC_SYS_TRUNCATE,
            deadline,
        ) {
            Ok(n) => {
                let n = (n as usize).min(out.len());
                if n >= 5 && out[0] == NS_MAGIC0 && out[1] == NS_MAGIC1 && out[3] == (op | 0x80) {
                    return Ok(n);
                }
            }
            Err(nexus_abi::IpcError::QueueEmpty) | Err(nexus_abi::IpcError::TimedOut) => {
                if now_ns() >= deadline {
                    return Err(());
                }
                let _ = yield_();
            }
            Err(_) => return Err(()),
        }
    }
}

fn ns_header(op: u8, req: &mut [u8]) {
    req[0] = NS_MAGIC0;
    req[1] = NS_MAGIC1;
    req[2] = NS_VERSION;
    req[3] = op;
}

fn id_at(buf: &[u8], n: usize) -> Result<u32, ()> {
    if n < 9 || buf[4] != NS_STATUS_OK {
        return Err(());
    }
    let id = u32::from_le_bytes([buf[5], buf[6], buf[7], buf[8]]);
    if id == 0 {
        Err(())
    } else {
        Ok(id)
    }
}

fn ns_listen(net: &KernelClient, ip: [u8; 4], port: u16) -> Result<u32, ()> {
    let mut req = [0u8; 10];
    ns_header(OP_LISTEN, &mut req);
    req[4..8].copy_from_slice(&ip);
    req[8..10].copy_from_slice(&port.to_le_bytes());
    let mut out = [0u8; 32];
    let n = net_rpc(net, &req, OP_LISTEN, &mut out)?;
    id_at(&out, n)
}

/// Connects, retrying `WOULD_BLOCK` until the step deadline.
fn ns_connect(net: &KernelClient, ip: [u8; 4], port: u16) -> Result<u32, ()> {
    let mut req = [0u8; 10];
    ns_header(OP_CONNECT, &mut req);
    req[4..8].copy_from_slice(&ip);
    req[8..10].copy_from_slice(&port.to_le_bytes());
    let deadline = now_ns().saturating_add(STEP_DEADLINE_NS);
    loop {
        let mut out = [0u8; 32];
        let n = net_rpc(net, &req, OP_CONNECT, &mut out)?;
        if n >= 5 && out[4] == NS_STATUS_WOULD_BLOCK {
            if now_ns() >= deadline {
                return Err(());
            }
            let _ = yield_();
            continue;
        }
        return id_at(&out, n);
    }
}

/// Accepts one connection on `listener`, retrying until the step deadline.
fn ns_accept(net: &KernelClient, listener: u32) -> Result<u32, ()> {
    let mut req = [0u8; 8];
    ns_header(OP_ACCEPT, &mut req);
    req[4..8].copy_from_slice(&listener.to_le_bytes());
    let deadline = now_ns().saturating_add(STEP_DEADLINE_NS);
    loop {
        let mut out = [0u8; 32];
        let n = net_rpc(net, &req, OP_ACCEPT, &mut out)?;
        if n >= 5 && out[4] == NS_STATUS_WOULD_BLOCK {
            if now_ns() >= deadline {
                return Err(());
            }
            let _ = yield_();
            continue;
        }
        return id_at(&out, n);
    }
}

/// Reads until `want` bytes arrived (`Ok(true)`), end-of-stream came first
/// (`Ok(false)`), or the step deadline passed (`Err`).
fn ns_read_until(net: &KernelClient, stream: u32, want: usize, buf: &mut [u8]) -> Result<bool, ()> {
    let mut have = 0usize;
    let deadline = now_ns().saturating_add(STEP_DEADLINE_NS);
    loop {
        let mut req = [0u8; 10];
        ns_header(OP_READ, &mut req);
        req[4..8].copy_from_slice(&stream.to_le_bytes());
        let max = (want - have).min(buf.len() - have) as u16;
        req[8..10].copy_from_slice(&max.to_le_bytes());
        let mut out = [0u8; 512];
        let n = net_rpc(net, &req, OP_READ, &mut out)?;
        if n >= 5 && out[4] == NS_STATUS_WOULD_BLOCK {
            if now_ns() >= deadline {
                return Err(());
            }
            let _ = yield_();
            continue;
        }
        if n < 7 || out[4] != NS_STATUS_OK {
            return Err(());
        }
        let len = usize::from(u16::from_le_bytes([out[5], out[6]]));
        if len == 0 {
            return Ok(false);
        }
        if 7 + len > n || have + len > buf.len() {
            return Err(());
        }
        buf[have..have + len].copy_from_slice(&out[7..7 + len]);
        have += len;
        if have >= want {
            return Ok(true);
        }
    }
}

/// Writes all of `data` (short frames; the facade takes ≤ 480 per RPC).
fn ns_write_all(net: &KernelClient, stream: u32, data: &[u8]) -> Result<(), ()> {
    let mut done = 0usize;
    let deadline = now_ns().saturating_add(STEP_DEADLINE_NS);
    while done < data.len() {
        let chunk = &data[done..];
        let mut req = [0u8; 10 + 64];
        let len = chunk.len().min(64);
        ns_header(OP_WRITE, &mut req);
        req[4..8].copy_from_slice(&stream.to_le_bytes());
        req[8..10].copy_from_slice(&(len as u16).to_le_bytes());
        req[10..10 + len].copy_from_slice(&chunk[..len]);
        let mut out = [0u8; 32];
        let n = net_rpc(net, &req[..10 + len], OP_WRITE, &mut out)?;
        if n >= 5 && out[4] == NS_STATUS_WOULD_BLOCK {
            if now_ns() >= deadline {
                return Err(());
            }
            let _ = yield_();
            continue;
        }
        if n < 7 || out[4] != NS_STATUS_OK {
            return Err(());
        }
        done += usize::from(u16::from_le_bytes([out[5], out[6]])).max(1).min(len);
    }
    Ok(())
}

fn ns_close(net: &KernelClient, stream: u32) {
    let mut req = [0u8; 8];
    ns_header(OP_CLOSE, &mut req);
    req[4..8].copy_from_slice(&stream.to_le_bytes());
    let mut out = [0u8; 32];
    let _ = net_rpc(net, &req, OP_CLOSE, &mut out);
}

/// `OP_EXPOSE(port, tcp)` to ingressd; returns `(status, reason)`.
fn expose(ing: &KernelClient, nonce: u32, port: u16) -> Result<(u8, u8), ()> {
    let mut req = [0u8; 11];
    req[0] = IG_MAGIC0;
    req[1] = IG_MAGIC1;
    req[2] = IG_VERSION;
    req[3] = OP_EXPOSE;
    req[4..8].copy_from_slice(&nonce.to_le_bytes());
    req[8..10].copy_from_slice(&port.to_le_bytes());
    req[10] = PROTO_TCP;
    let reply = cached_reply_client().map_err(|_| ())?;
    let (reply_send_slot, reply_recv_slot) = reply.slots();
    let reply_send_clone = nexus_abi::cap_clone(reply_send_slot).map_err(|_| ())?;
    ing.send_with_cap_move(&req, reply_send_clone).map_err(|_| ())?;
    let deadline = now_ns().saturating_add(STEP_DEADLINE_NS);
    loop {
        let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut out = [0u8; 32];
        match nexus_abi::ipc_recv_v1(
            reply_recv_slot,
            &mut hdr,
            &mut out,
            nexus_abi::IPC_SYS_TRUNCATE,
            deadline,
        ) {
            Ok(n) => {
                let n = (n as usize).min(out.len());
                if n >= 10
                    && out[0] == IG_MAGIC0
                    && out[1] == IG_MAGIC1
                    && out[3] == OP_EXPOSE
                    && u32::from_le_bytes([out[4], out[5], out[6], out[7]]) == nonce
                {
                    return Ok((out[8], out[9]));
                }
            }
            Err(nexus_abi::IpcError::QueueEmpty) | Err(nexus_abi::IpcError::TimedOut) => {
                if now_ns() >= deadline {
                    return Err(());
                }
                let _ = yield_();
            }
            Err(_) => return Err(()),
        }
    }
}

/// Bytes cross the gateway both ways.
fn allow_proof(
    net: &KernelClient,
    ing: &KernelClient,
    ip: [u8; 4],
    backend: u32,
) -> Result<(), ()> {
    if expose(ing, 0x5201, ALLOW_PORT)? != (IG_STATUS_ALLOW, 0) {
        return Err(());
    }
    let client = ns_connect(net, ip, ALLOW_PORT)?;
    ns_write_all(net, client, b"ping")?;
    let server = ns_accept(net, backend)?;
    let mut buf = [0u8; 8];
    if ns_read_until(net, server, 4, &mut buf)? != true || &buf[..4] != b"ping" {
        ns_close(net, client);
        ns_close(net, server);
        return Err(());
    }
    ns_write_all(net, server, b"pong")?;
    let got = ns_read_until(net, client, 4, &mut buf);
    ns_close(net, client);
    ns_close(net, server);
    if got? && &buf[..4] == b"pong" {
        Ok(())
    } else {
        Err(())
    }
}

/// The gateway accepts, judges the peer, and closes: end-of-stream.
fn closed_by_gateway(net: &KernelClient, stream: u32) -> bool {
    let mut buf = [0u8; 8];
    let r = ns_read_until(net, stream, 1, &mut buf);
    ns_close(net, stream);
    matches!(r, Ok(false))
}

fn cidr_proof(net: &KernelClient, ing: &KernelClient, ip: [u8; 4]) -> Result<(), ()> {
    if expose(ing, 0x5202, CIDR_PORT)? != (IG_STATUS_ALLOW, 0) {
        return Err(());
    }
    let client = ns_connect(net, ip, CIDR_PORT)?;
    if closed_by_gateway(net, client) {
        Ok(())
    } else {
        Err(())
    }
}

fn rate_proof(net: &KernelClient, ing: &KernelClient, ip: [u8; 4]) -> Result<(), ()> {
    if expose(ing, 0x5203, RATE_PORT)? != (IG_STATUS_ALLOW, 0) {
        return Err(());
    }
    // burst = 2: the first two are admitted (and forwarded to the backend
    // listener, which never needs to accept them), the third is refused.
    let first = ns_connect(net, ip, RATE_PORT)?;
    let second = ns_connect(net, ip, RATE_PORT)?;
    let third = ns_connect(net, ip, RATE_PORT)?;
    let refused = closed_by_gateway(net, third);
    ns_close(net, first);
    ns_close(net, second);
    if refused {
        Ok(())
    } else {
        Err(())
    }
}

/// Runs the Layer-B proofs (single-VM; `local_ip` = the interface address).
pub(crate) fn ingress_gateway_proofs(local_ip: Option<[u8; 4]>) {
    let ip = local_ip.unwrap_or([10, 0, 2, 15]);
    let (Ok(net), Ok(ing)) = (cached_netstackd_client(), KernelClient::new_for("ingressd")) else {
        emit_line(crate::markers::M_SELFTEST_INGRESS_ALLOW_FAIL);
        emit_line(crate::markers::M_SELFTEST_INGRESS_INTENT_DENY_FAIL);
        emit_line(crate::markers::M_SELFTEST_INGRESS_CIDR_DENY_FAIL);
        emit_line(crate::markers::M_SELFTEST_INGRESS_RATE_FAIL);
        return;
    };
    // Loopback backends first so every forwarded connection finds one.
    let backend_allow = ns_listen(&net, LOOPBACK, ALLOW_BACKEND);
    let _ = ns_listen(&net, LOOPBACK, CIDR_BACKEND);
    let _ = ns_listen(&net, LOOPBACK, RATE_BACKEND);

    match backend_allow.and_then(|b| allow_proof(&net, &ing, ip, b)) {
        Ok(()) => emit_line(crate::markers::M_SELFTEST_INGRESS_ALLOW_OK),
        Err(()) => emit_line(crate::markers::M_SELFTEST_INGRESS_ALLOW_FAIL),
    }
    match expose(&ing, 0x5204, UNDECLARED_PORT) {
        Ok((IG_STATUS_DENY, REASON_POLICY)) => {
            emit_line(crate::markers::M_SELFTEST_INGRESS_INTENT_DENY_OK)
        }
        _ => emit_line(crate::markers::M_SELFTEST_INGRESS_INTENT_DENY_FAIL),
    }
    match cidr_proof(&net, &ing, ip) {
        Ok(()) => emit_line(crate::markers::M_SELFTEST_INGRESS_CIDR_DENY_OK),
        Err(()) => emit_line(crate::markers::M_SELFTEST_INGRESS_CIDR_DENY_FAIL),
    }
    match rate_proof(&net, &ing, ip) {
        Ok(()) => emit_line(crate::markers::M_SELFTEST_INGRESS_RATE_OK),
        Err(()) => emit_line(crate::markers::M_SELFTEST_INGRESS_RATE_FAIL),
    }
}
