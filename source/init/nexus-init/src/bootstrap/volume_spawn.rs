// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The second spawn pass (TASK-0321 P2, RFC-0089 §12.3, ADR-0060):
//! services listed in `scripts/system-volume-services.txt` are NOT in the
//! embedded image table — init spawns them from the verified system volume
//! after the MMIO grants (the block plane is live from here) and before
//! `wire_services` (so the usual per-service wiring, routes and the wave-2
//! resume treat them exactly like embedded services). Per service: ask
//! bundlemgrd for the bundle (`QUERY_BUNDLE` → size + launch params), hand
//! it a fresh object (`GET_BUNDLE_IMAGE`, armed by CAP_MOVE; bundlemgrd fills
//! it from byte 0, SEALS it and hashes the sealed bytes against the index
//! digest — ADR-0080), then `exec_vmo` the sealed image: its text and rodata
//! are the object's own frames, nothing is mapped into init. The capability
//! is kept for the process lifetime (a respawn execs the same object,
//! ADR-0057). P4 runs this pass from the
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

const PAGE: usize = 4096;

/// A service spawned from the volume. Its sealed image stays init's for the process lifetime:
/// the respawn arm (ADR-0057, P4) execs exactly this verified object with the same launch
/// parameters — never a second block-plane round trip, never a re-verify of what the seal
/// already guards (ADR-0080).
pub(crate) struct VolumeSpawned {
    pub(crate) name: &'static str,
    pub(crate) pid: u32,
    /// init's capability slot of the sealed image (bundlemgrd-filled and -verified).
    pub(crate) image: u32,
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
            Self::Exec => "exec",
        }
    }
}

/// One request/reply against bundlemgrd on init's reply inbox: a reply-SEND clone rides the
/// request (CAP_MOVE), the answer is WAITED for — bundlemgrd's reply or its death (RFC-0079
/// EOF), never a clock (TASK-0324 P7-d). Foreign frames on the inbox are stashed for their
/// own exchange. `Some(len)` = the wanted reply is in `out`.
fn request(
    bnd_req: u32,
    ask: nexus_ipc::SlotPair,
    req: &[u8],
    want_op: u8,
    out: &mut [u8],
) -> Option<usize> {
    nexus_ipc::exchange::call_matching(bnd_req, ask, req, out, |f| {
        (f.len() >= 4 && f[0] == wire::MAGIC0 && f[1] == wire::MAGIC1 && f[3] == (want_op | 0x80))
            .then(|| f.len())
    })
    .ok()
}

/// Arms `vmo` as the destination of init's next VMO op (`OP_ARM_VMO`, TASK-0324 P7-d): a
/// clone travels as the moved cap; the send waits for queue space (no clock).
fn arm_vmo(bnd_req: u32, vmo: u32) -> Option<()> {
    let moved = nexus_abi::cap_clone(vmo).ok()?;
    let mut req = [0u8; 4];
    wire::encode_arm_vmo(&mut req);
    if nexus_ipc::exchange::send_with_cap(bnd_req, &req, moved).is_err() {
        let _ = nexus_abi::cap_close(moved);
        return None;
    }
    Some(())
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

fn spawn_one(
    name: &'static str,
    bnd_req: u32,
    ask: nexus_ipc::SlotPair,
) -> Result<VolumeSpawned, Fail> {
    // 1. QUERY_BUNDLE → size + launch params (+ sha8/version for the marker).
    let mut req = [0u8; 64];
    let n = wire::encode_query_bundle(name.as_bytes(), &mut req).ok_or(Fail::Query)?;
    let mut rsp = [0u8; 96];
    let rn =
        request(bnd_req, ask, &req[..n], wire::OP_QUERY_BUNDLE, &mut rsp).ok_or(Fail::Query)?;
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

    // 2. A fresh object sized for the ELF, ARMED at bundlemgrd, then GET_BUNDLE_IMAGE with a
    //    reply cap: bundlemgrd fills it from byte 0 through a bounce the block device writes,
    //    SEALS it and hashes the sealed bytes against the index digest (ADR-0080) — the answer
    //    IS the completion (TASK-0324 P7-d).
    let total = (size as usize).div_ceil(PAGE) * PAGE;
    let vmo = nexus_abi::vmo_create(total).map_err(|_| Fail::Vmo)?;
    let served = (|| -> Result<(), Fail> {
        arm_vmo(bnd_req, vmo).ok_or(Fail::Send)?;
        let n = wire::encode_get_bundle_image(name.as_bytes(), &mut req).ok_or(Fail::Send)?;
        let rn = request(bnd_req, ask, &req[..n], wire::OP_GET_BUNDLE_IMAGE, &mut rsp)
            .ok_or(Fail::Send)?;
        // 3. The answer: complete AND hashed to the index digest (bundlemgrd's contract). A
        //    wrong image is sealed too — it can never run; it is closed here.
        let (status, len) = wire::decode_payload_done_rsp(&rsp[..rn], wire::OP_GET_BUNDLE_IMAGE)
            .ok_or(Fail::Header)?;
        match status {
            s if s == nexus_abi::status::CODE_OK && len == size => Ok(()),
            s if s == nexus_abi::status::VfsError::Integrity.code() => Err(Fail::Digest),
            _ => Err(Fail::Header),
        }
    })();
    if let Err(fail) = served {
        let _ = nexus_abi::cap_close(vmo);
        return Err(fail);
    }

    // 4. exec from the sealed image; the capability stays init's for every respawn.
    let pid = match nexus_abi::exec_vmo(vmo, stack_pages as usize, global_pointer, name, false) {
        Ok(pid) => pid,
        Err(_) => {
            let _ = nexus_abi::cap_close(vmo);
            return Err(Fail::Exec);
        }
    };

    // `init: spawn from volume svc=<n> bundle=<n>@<v> sha=<8>` — a proof
    // witness (ladder); folds in interactive boots (RFC-0068).
    if crate::bootstrap::diag::raw_or_expanded(name) {
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
    }
    Ok(VolumeSpawned { name, pid, image: vmo, stack_pages, global_pointer })
}

/// Spawns every volume service (SSOT `service_source::volume_services`),
/// attaching each one's control channel exactly like the embedded loop
/// did. Returns the spawned set; missing ones were reported and skipped.
pub(crate) fn spawn_volume_services(
    ctrls: &mut Vec<CtrlChannel>,
    bnd_req: u32,
    ask: nexus_ipc::SlotPair,
    init_fold: bool,
    stage_fence: u32,
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
        match spawn_one(name, bnd_req, ask) {
            Ok(v) => {
                let (ctrl, _send, _recv) =
                    crate::bootstrap::spawn::attach_ctrl_channel(name, v.pid, stage_fence)?;
                ctrls.push(ctrl);
                // `init: up <svc>` is emitted by the responder on `@ready` (RFC-0093 §2).
                spawned.push(v);
            }
            Err(fail) => emit_fail(name, fail),
        }
    }
    Ok(spawned)
}
