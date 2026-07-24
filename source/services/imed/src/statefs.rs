// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: TASK-0204 / RFC-0075 Phase 4 — imed's statefsd-backed `BlobIo`: the
//! ime-ranker personalization blob lives under `/state/ime/<lang>/personal` in
//! statefsd's journaled KV store. Transport = imed's own fixed-slot recipe (the
//! `persist_layout` settingsd leg): a SEND clone CAP_MOVEd on the pinned statefs
//! request slot, bounded drain on the private reply inbox. Wire = statefsd v1
//! (`'S','F'` / OP_GET / OP_PUT), mirroring settingsd's `statefs_client`.
//! Best-effort throughout: any routing/IPC/status failure degrades to no-blob /
//! no-persist (fail-closed load stays empty) — never a crash, never a boot hang.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal (binary crate)
//! TEST_COVERAGE: `SELFTEST: ime ranking persist ok` (imed boot round-trip).

#![cfg(all(nexus_env = "os", feature = "os-lite"))]

use alloc::vec::Vec;

use ime_ranker::BlobIo;

/// imed's statefsd route slots (init `provision_imed_legs`, TASK-0204): a SEND
/// clone of statefsd's request endpoint (0x0B) + a private CAP_MOVE reply inbox
/// (RECV 0x0C / SEND 0x0D — the SEND is cloned + moved per request).
const STATEFS_SEND_SLOT: u32 = 0x0B;
const STATEFS_REPLY_RECV_SLOT: u32 = 0x0C;
const STATEFS_REPLY_SEND_SLOT: u32 = 0x0D;

// statefsd v1 wire (userspace/statefs `protocol`).
const SF_MAGIC0: u8 = b'S';
const SF_MAGIC1: u8 = b'F';
const SF_VERSION: u8 = 1;
const SF_VERSION_V2: u8 = 2;
const SF_OP_PUT: u8 = 1;
const SF_OP_GET: u8 = 2;
const SF_STATUS_OK: u8 = 0;

/// statefsd-backed [`BlobIo`] for the ranking store. `path` is the statefsd key
/// (statefsd's journal accepts only `/state/`-rooted keys).
pub(crate) struct StatefsBlobIo;

impl BlobIo for StatefsBlobIo {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        let mut req = Vec::with_capacity(6 + path.len());
        req.extend_from_slice(&[SF_MAGIC0, SF_MAGIC1, SF_VERSION, SF_OP_GET]);
        req.extend_from_slice(&(path.len() as u16).to_le_bytes());
        req.extend_from_slice(path.as_bytes());
        decode_get_value(&request_reply(&req)?)
    }

    fn write(&mut self, path: &str, bytes: &[u8]) -> bool {
        let mut req = Vec::with_capacity(10 + path.len() + bytes.len());
        req.extend_from_slice(&[SF_MAGIC0, SF_MAGIC1, SF_VERSION, SF_OP_PUT]);
        req.extend_from_slice(&(path.len() as u16).to_le_bytes());
        req.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        req.extend_from_slice(path.as_bytes());
        req.extend_from_slice(bytes);
        request_reply(&req).map(|rsp| decode_put_ok(&rsp)).unwrap_or(false)
    }
}

/// Parse a statefsd GET response value (v1 9-byte or v2 17-byte header); `None`
/// on any non-OK status or malformed frame.
fn decode_get_value(frame: &[u8]) -> Option<Vec<u8>> {
    if frame.len() < 5 || frame[0] != SF_MAGIC0 || frame[1] != SF_MAGIC1 {
        return None;
    }
    if frame[3] != (SF_OP_GET | 0x80) || frame[4] != SF_STATUS_OK {
        return None;
    }
    let (hdr, len_at) = match frame[2] {
        SF_VERSION => (9usize, 5usize),
        SF_VERSION_V2 => (17usize, 13usize),
        _ => return None,
    };
    if frame.len() < hdr {
        return None;
    }
    let vlen = u32::from_le_bytes([
        frame[len_at],
        frame[len_at + 1],
        frame[len_at + 2],
        frame[len_at + 3],
    ]) as usize;
    (frame.len() == hdr + vlen).then(|| frame[hdr..hdr + vlen].to_vec())
}

/// True when a statefsd PUT response reports OK (v1 or v2 status frame).
fn decode_put_ok(frame: &[u8]) -> bool {
    frame.len() >= 5
        && frame[0] == SF_MAGIC0
        && frame[1] == SF_MAGIC1
        && frame[3] == (SF_OP_PUT | 0x80)
        && frame[4] == SF_STATUS_OK
}

/// One bounded CAP_MOVE request/reply with statefsd on imed's pinned slots.
/// Clones the reply-send, CAP_MOVEs it with the request, drains the private
/// reply inbox until the matching `'S','F'` frame or the 500 ms deadline.
fn request_reply(req: &[u8]) -> Option<Vec<u8>> {
    let reply_send = nexus_abi::cap_clone(STATEFS_REPLY_SEND_SLOT).ok()?;
    let hdr =
        nexus_abi::MsgHeader::new(reply_send, 0, 0, nexus_abi::ipc_hdr::CAP_MOVE, req.len() as u32);
    if nexus_abi::ipc_send_v1(STATEFS_SEND_SLOT, &hdr, req, nexus_abi::IPC_SYS_NONBLOCK, 0).is_err()
    {
        let _ = nexus_abi::cap_close(reply_send);
        return None;
    }
    // The CAP_MOVE consumed the clone; do NOT close it on the success path.
    let deadline = nexus_abi::nsec().unwrap_or(0).saturating_add(500_000_000);
    loop {
        let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 1024];
        let mut sid: u64 = 0;
        match nexus_abi::ipc_recv_v2(
            STATEFS_REPLY_RECV_SLOT,
            &mut rh,
            &mut buf,
            &mut sid,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => {
                let n = core::cmp::min(n as usize, buf.len());
                if n >= 4 && buf[0] == SF_MAGIC0 && buf[1] == SF_MAGIC1 {
                    return Some(buf[..n].to_vec());
                }
                if nexus_abi::nsec().unwrap_or(0) >= deadline {
                    return None;
                }
                let _ = nexus_abi::yield_();
            }
            Err(nexus_abi::IpcError::QueueEmpty) => {
                if nexus_abi::nsec().unwrap_or(0) >= deadline {
                    return None;
                }
                let _ = nexus_abi::yield_();
            }
            Err(_) => return None,
        }
    }
}
