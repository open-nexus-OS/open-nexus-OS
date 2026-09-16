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

/// imed's statefsd leg (TASK-0204) as declared — a `PrivateInbox` route (TASK-0324 P4f-1b): the
/// request SEND plus a reply inbox of its own (the SEND half is cloned + moved per request).
const STATEFS_SEND_SLOT: u32 = nexus_service_topology::slots::imed::STATEFSD.send;
const STATEFS_REPLY_RECV_SLOT: u32 = nexus_service_topology::slots::imed::STATEFSD.recv;
const STATEFS_REPLY_SEND_SLOT: u32 = nexus_service_topology::slots::imed::STATEFSD_INBOX_SEND;

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

/// ONE waited exchange with statefsd on imed's PRIVATE pinned pair (TASK-0324 P7-d):
/// statefsd's answer or its death, never a clock. The doc-comment's "500 ms deadline" left
/// with the deadline itself; the frame filter stays because this pair carries every statefs op
/// imed issues.
fn request_reply(req: &[u8]) -> Option<Vec<u8>> {
    let mut buf = [0u8; 1024];
    let len = nexus_ipc::exchange::call_matching(
        STATEFS_SEND_SLOT,
        nexus_ipc::SlotPair::new(STATEFS_REPLY_SEND_SLOT, STATEFS_REPLY_RECV_SLOT),
        req,
        &mut buf,
        |rsp| (rsp.len() >= 4 && rsp[0] == SF_MAGIC0 && rsp[1] == SF_MAGIC1).then(|| rsp.len()),
    )
    .ok()?;
    Some(buf[..len].to_vec())
}
