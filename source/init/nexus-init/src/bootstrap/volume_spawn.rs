// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
#![allow(unsafe_code)]

//! CONTEXT: The second spawn pass (TASK-0321 P2, RFC-0089 §12.3, ADR-0060):
//! services listed in `scripts/system-volume-services.txt` are NOT in the
//! embedded image table — init spawns them from the verified system volume
//! after the MMIO grants (the block plane is live from here) and before
//! `wire_services` (so the usual per-service wiring, routes and the wave-2
//! resume treat them exactly like embedded services). Per service: ask
//! bundlemgrd for the bundle (`QUERY_BUNDLE` → size + launch params), hand
//! it a fresh VMO (`GET_BUNDLE_ELF`, CAP_MOVE; bundlemgrd writes the
//! verified `payload.elf` and the header LAST), poll the header, map the
//! VMO read-only at a kernel-chosen address, and `exec_v2` the mapped
//! slice — a mapped VMO is a valid ELF source, no kernel change. The
//! mapping is kept for the process lifetime (the VMO arena never frees; a
//! respawn re-execs the same bytes, ADR-0057). P4 runs this pass from the
//! CORE-plane stage (`core_plane.rs`), BEFORE any per-pid endpoint mint. Failure is honest absence:
//! `init: volume spawn FAIL svc=<n> reason=<r>` and the service is simply
//! not there (its consumers are best-effort by design — the pilot is
//! `SAFE_EXCLUDED` metricsd).
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU ladder (`init: spawn from volume svc=metricsd …`,
//!   `metricsd: ready` unchanged); the SSOT list is host-tested in
//!   `service_source.rs`.
//! ADR: docs/adr/0060-verified-system-volume-bundlemgrd-verifier-init-spawner.md

use alloc::vec::Vec;

use crate::bootstrap::helpers::{debug_write_bytes, debug_write_str};
use crate::bootstrap::CtrlChannel;
use crate::os_payload::InitError;
use nexus_abi::bundlemgrd as wire;
use nexus_abi::page_flags;

/// One request/reply budget against bundlemgrd (it may still be attaching
/// the block plane on the first call — its own attach window is 2 s).
const REQ_DEADLINE_NS: u64 = 4_000_000_000;
/// Header-poll budget after GET_BUNDLE_ELF (streams ≤ a few MiB over the
/// 6 KiB block plane; metricsd is ~80 KiB).
const HEADER_POLL_YIELDS: usize = 200_000;
const PAGE: usize = 4096;

/// A service spawned from the volume. Its read-only ELF mapping stays
/// alive for the process lifetime: the respawn arm (ADR-0057, P4) re-execs
/// exactly these verified bytes with the same launch parameters — never a
/// second block-plane round trip, never a re-verify of what the mapping
/// already proves.
pub(crate) struct VolumeSpawned {
    pub(crate) name: &'static str,
    pub(crate) pid: u32,
    /// The mapped `payload.elf` (bundlemgrd-verified, header-last).
    pub(crate) elf: &'static [u8],
    pub(crate) stack_pages: u32,
    pub(crate) global_pointer: u64,
}

/// Why a volume spawn was skipped — the marker vocabulary.
#[derive(Clone, Copy)]
enum Fail {
    Query,
    Unavailable,
    NotFound,
    Vmo,
    Send,
    Header,
    Digest,
    Map,
    Exec,
}

impl Fail {
    fn label(self) -> &'static str {
        match self {
            Self::Query => "query",
            Self::Unavailable => "volume-unavailable",
            Self::NotFound => "not-found",
            Self::Vmo => "vmo",
            Self::Send => "send",
            Self::Header => "header",
            Self::Digest => "digest",
            Self::Map => "map",
            Self::Exec => "exec",
        }
    }
}

/// Init-side request/reply against bundlemgrd on the pre-minted pair
/// (`bnd_req` SEND; replies via a CAP_MOVEd clone of `reply_send`,
/// received on `reply_recv` — the `bundlemgrd_set_active_slot` shape).
fn request(
    pending: &mut nexus_ipc::reqrep::FrameStash<8, 16>,
    bnd_req: u32,
    reply_send: u32,
    reply_recv: u32,
    req: &[u8],
    moved_cap: Option<u32>,
    want_op: u8,
    out: &mut [u8],
) -> Option<usize> {
    // The single moved cap: either the destination VMO (GET_BUNDLE_ELF —
    // no reply frame, the header IS the reply) or a reply-send clone.
    let cap = match moved_cap {
        Some(vmo) => vmo,
        None => nexus_abi::cap_clone(reply_send).ok()?,
    };
    let hdr = nexus_abi::MsgHeader::new(cap, 0, 0, nexus_abi::ipc_hdr::CAP_MOVE, req.len() as u32);
    let deadline = nexus_abi::nsec().ok()?.saturating_add(REQ_DEADLINE_NS);
    nexus_abi::ipc_send_v1(bnd_req, &hdr, req, 0, deadline).ok()?;
    if moved_cap.is_some() {
        return Some(0);
    }
    let is_want = |f: &[u8]| {
        f.len() >= 4 && f[0] == wire::MAGIC0 && f[1] == wire::MAGIC1 && f[3] == (want_op | 0x80)
    };
    if let Some(n) = pending.take_into_where(out, is_want) {
        return Some(n);
    }
    let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
    loop {
        match nexus_abi::ipc_recv_v1(
            reply_recv,
            &mut rh,
            out,
            nexus_abi::IPC_SYS_TRUNCATE,
            deadline,
        ) {
            Ok(n) => {
                let n = core::cmp::min(n as usize, out.len());
                if is_want(&out[..n]) {
                    return Some(n);
                }
                let _ = pending.push(&out[..n]);
            }
            Err(nexus_abi::IpcError::QueueEmpty) => {
                if nexus_abi::nsec().unwrap_or(u64::MAX) >= deadline {
                    return None;
                }
                let _ = nexus_abi::yield_();
            }
            // Deadline hit or a hard error: give up this spawn (honest absence).
            Err(_) => return None,
        }
    }
}

/// Lowercase hex, two digits per byte (`debug_write_hex` prints a usize).
fn debug_write_hex_bytes(bytes: &[u8]) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for &b in bytes {
        let pair = [HEX[(b >> 4) as usize], HEX[(b & 0xf) as usize]];
        debug_write_bytes(&pair);
    }
}

fn emit_fail(name: &str, fail: Fail) {
    debug_write_bytes(b"init: volume spawn FAIL svc=");
    debug_write_str(name);
    debug_write_bytes(b" reason=");
    debug_write_str(fail.label());
    debug_write_bytes(b"\n");
}

/// SAFETY: `va..va+len` is a live read-only mapping returned by `vm_map` on
/// a VMO init created and never unmaps or destroys (the mapping outlives
/// the spawned process by design — respawn re-execs the same bytes);
/// bundlemgrd wrote the bytes BEFORE the header became visible and nothing
/// writes them afterwards (the VMO cap was moved to bundlemgrd and closed
/// there). Hence a `'static` shared view is sound.
fn ro_slice(va: usize, len: usize) -> &'static [u8] {
    unsafe { core::slice::from_raw_parts(va as *const u8, len) }
}

fn spawn_one(
    name: &'static str,
    pending: &mut nexus_ipc::reqrep::FrameStash<8, 16>,
    bnd_req: u32,
    reply_send: u32,
    reply_recv: u32,
) -> Result<VolumeSpawned, Fail> {
    // 1. QUERY_BUNDLE → size + launch params (+ sha8/version for the marker).
    let mut req = [0u8; 64];
    let n = wire::encode_query_bundle(name.as_bytes(), &mut req).ok_or(Fail::Query)?;
    let mut rsp = [0u8; 96];
    let rn = request(
        pending,
        bnd_req,
        reply_send,
        reply_recv,
        &req[..n],
        None,
        wire::OP_QUERY_BUNDLE,
        &mut rsp,
    )
    .ok_or(Fail::Query)?;
    let (status, size, stack_pages, global_pointer, sha8, version) =
        wire::decode_query_bundle_rsp(&rsp[..rn]).ok_or(Fail::Query)?;
    match status {
        wire::STATUS_OK => {}
        wire::STATUS_NOT_FOUND => return Err(Fail::NotFound),
        _ => return Err(Fail::Unavailable),
    }
    if size == 0 || stack_pages == 0 {
        return Err(Fail::NotFound);
    }
    let mut sha8_buf = [0u8; 8];
    let sha_len = sha8.len().min(8);
    sha8_buf[..sha_len].copy_from_slice(&sha8[..sha_len]);
    let mut version_buf = [0u8; 32];
    let ver_len = version.len().min(32);
    version_buf[..ver_len].copy_from_slice(&version[..ver_len]);

    // 2. A fresh VMO sized for header + ELF, moved to bundlemgrd.
    let total = (wire::PAYLOAD_DATA_OFFSET + size as usize).div_ceil(PAGE) * PAGE;
    let vmo = nexus_abi::vmo_create(total).map_err(|_| Fail::Vmo)?;
    let moved = nexus_abi::cap_clone(vmo).map_err(|_| Fail::Vmo)?;
    let n = wire::encode_get_bundle_elf(name.as_bytes(), &mut req).ok_or(Fail::Send)?;
    request(
        pending,
        bnd_req,
        reply_send,
        reply_recv,
        &req[..n],
        Some(moved),
        wire::OP_GET_BUNDLE_ELF,
        &mut rsp,
    )
    .ok_or(Fail::Send)?;

    // 3. Header-last poll: a visible header means the bytes are complete
    //    AND hashed to the index digest (bundlemgrd's contract).
    let mut hdr = [0u8; wire::PAYLOAD_DATA_OFFSET];
    let mut polls = 0usize;
    let len = loop {
        if nexus_abi::vmo_read(vmo, 0, &mut hdr).is_err() {
            return Err(Fail::Header);
        }
        if let Some((status, len)) = wire::decode_payload_header(&hdr) {
            match status {
                wire::PAYLOAD_STATUS_OK if len == size => break len as usize,
                wire::PAYLOAD_STATUS_DIGEST => return Err(Fail::Digest),
                _ => return Err(Fail::Header),
            }
        }
        polls += 1;
        if polls > HEADER_POLL_YIELDS {
            return Err(Fail::Header);
        }
        let _ = nexus_abi::yield_();
    };

    // 4. Map read-only (kernel-chosen VA) and exec the mapped slice.
    let va = nexus_abi::vm_map(vmo, 0, total, page_flags::USER | page_flags::READ)
        .map_err(|_| Fail::Map)?;
    let elf = &ro_slice(va, total)[wire::PAYLOAD_DATA_OFFSET..wire::PAYLOAD_DATA_OFFSET + len];
    let pid = nexus_abi::exec_v2(elf, stack_pages as usize, global_pointer, name)
        .map_err(|_| Fail::Exec)?;

    // `init: spawn from volume svc=<n> bundle=<n>@<v> sha=<8>`
    debug_write_bytes(b"init: spawn from volume svc=");
    debug_write_str(name);
    debug_write_bytes(b" bundle=");
    debug_write_str(name);
    debug_write_bytes(b"@");
    if let Ok(v) = core::str::from_utf8(&version_buf[..ver_len]) {
        debug_write_str(v);
    }
    debug_write_bytes(b" sha=");
    debug_write_hex_bytes(&sha8_buf[..sha_len]);
    debug_write_bytes(b"\n");
    Ok(VolumeSpawned { name, pid, elf, stack_pages, global_pointer })
}

/// Spawns every volume service (SSOT `service_source::volume_services`),
/// attaching each one's control channel exactly like the embedded loop
/// did. Returns the spawned set; missing ones were reported and skipped.
pub(crate) fn spawn_volume_services(
    ctrls: &mut Vec<CtrlChannel>,
    pending: &mut nexus_ipc::reqrep::FrameStash<8, 16>,
    bnd_req: u32,
    reply_send: u32,
    reply_recv: u32,
    init_fold: bool,
) -> Result<Vec<VolumeSpawned>, InitError> {
    let mut spawned = Vec::new();
    for name in crate::service_source::volume_services() {
        if ctrls.iter().any(|c| c.svc_name == name) {
            // Also embedded (P1 shape / NEXUS_VOLUME_SPAWN=0): the embedded
            // instance already runs; never spawn a second one.
            continue;
        }
        if !init_fold {
            debug_write_str("init: start ");
            debug_write_str(name);
            debug_write_bytes(b"\n");
        }
        match spawn_one(name, pending, bnd_req, reply_send, reply_recv) {
            Ok(v) => {
                let (ctrl, _send, _recv) =
                    crate::bootstrap::spawn::attach_ctrl_channel(name, v.pid)?;
                ctrls.push(ctrl);
                if !init_fold {
                    debug_write_str("init: up ");
                    debug_write_str(name);
                    debug_write_bytes(b"\n");
                }
                spawned.push(v);
            }
            Err(fail) => emit_fail(name, fail),
        }
    }
    Ok(spawned)
}
