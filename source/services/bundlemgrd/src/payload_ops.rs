// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
#![cfg(all(nexus_env = "os", feature = "os-lite"))]

//! CONTEXT: bundlemgrd's VMO-serving ops — the header-last shared-memory
//! contract (plus, TASK-0324 P7-d, a reply AFTER the header write on the
//! sender's moved reply cap; the VMO is armed first with `OP_ARM_VMO`) shared by GET_PAYLOAD (TASK-0080D: an app's ui-program bytes
//! from the build-time table) and the TASK-0321 system-volume ops
//! (QUERY_BUNDLE / GET_BUNDLE_ELF / VOLUME_STATUS: a service's verified
//! `payload.elf` out of `volume.rs`). Split from `os_lite.rs` under the
//! structure ratchet; the main loop only routes frames here.
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU ladder (`bundlemgrd: payload served`, `bundlemgrd:
//!   bundle served (name=…)`, `init: spawn from volume …`)
//! ADR: docs/adr/0060-verified-system-volume-bundlemgrd-verifier-init-spawner.md

use nexus_ipc::{KernelServer, Server as _, Wait};

use crate::os_lite::{
    emit_line, emit_sender_denied, is_allowed_sender, metrics_counter_inc_best_effort, rsp,
    STATUS_MALFORMED, STATUS_OK, STATUS_UNSUPPORTED,
};

/// Serves one GET_PAYLOAD request (TASK-0321 P5: from the system volume):
/// the app id must be an APP bundle of the verified volume (its
/// `meta/app.properties` sidecar is what makes it one — a service's ELF is
/// never served as a ui-program); its `payload.nxir` entry is bulk-read
/// into the moved VMO, hashed against the index digest, and the 16-byte
/// header is written LAST (header-last = release ordering — the consumer
/// polls the header, so a visible header guarantees complete, verified
/// payload bytes). The moved VMO capability is closed on every path.
pub(crate) fn handle_get_payload(
    volume: &mut crate::volume::VolumeState,
    frame: &[u8],
    vmo_slot: Option<u32>,
) -> (u16, u32) {
    use nexus_abi::bundlemgrd as wire;
    use nexus_abi::{payload_vmo as vmo_hdr, status as code};
    let Some(vmo) = vmo_slot else {
        emit_line("bundlemgrd: FAIL get_payload (no vmo cap)");
        return (code::VfsError::Invalid.code(), 0);
    };
    let outcome = (|| -> (u16, u32) {
        let Some(app_id) = wire::decode_get_payload(frame) else {
            return (code::VfsError::Invalid.code(), 0);
        };
        let vol = match volume.ensure() {
            Ok(v) => v,
            Err(_) => return (code::VfsError::Io.code(), 0),
        };
        if vol.app(app_id).is_none() {
            return (code::VfsError::NotFound.code(), 0);
        }
        // ui-program bundles carry `payload.nxir` (nxb-pack names the payload by
        // kind); a service ELF is never served here.
        let Some(entry) = vol.lookup_entry(app_id, b"payload.nxir") else {
            return (code::VfsError::NotFound.code(), 0);
        };
        match vol.stream_entry_into_vmo(entry, vmo, vmo_hdr::DATA_OFFSET) {
            Ok((len, _, _)) => (code::CODE_OK, len),
            Err(crate::volume::VolumeFail::Digest) => (code::VfsError::Integrity.code(), 0),
            Err(crate::volume::VolumeFail::Bounds) => (code::VfsError::TooBig.code(), 0),
            Err(_) => (code::VfsError::Io.code(), 0),
        }
    })();
    let (status, len) = outcome;
    let hdr = vmo_hdr::encode_header(status, len);
    let _ = nexus_abi::vmo_write(vmo, 0, &hdr);
    let _ = nexus_abi::cap_close(vmo);
    if status == code::CODE_OK {
        metrics_counter_inc_best_effort("bundlemgrd.get_payload.ok");
        emit_line("bundlemgrd: payload served");
    } else {
        metrics_counter_inc_best_effort("bundlemgrd.get_payload.fail");
        emit_line("bundlemgrd: FAIL get_payload (status)");
    }
    (status, len)
}

/// TASK-0321: init (the sole spawner; kernel-attributed `init-lite` /
/// `nexus-init` id) may pull bundle ELFs; the read-only QUERY/STATUS ops
/// are open to init and the boot-safe allowlist (selftest cross-checks).
fn is_init_sender(sender_service_id: u64) -> bool {
    sender_service_id == nexus_abi::service_id_from_name(b"init-lite")
        || sender_service_id == nexus_abi::service_id_from_name(b"nexus-init")
}

pub(crate) fn handle_volume_op(
    volume: &mut crate::volume::VolumeState,
    armed: &mut crate::armed_vmo::ArmedVmos,
    frame: &[u8],
    sender_service_id: u64,
    reply: Option<nexus_ipc::ReplyCap>,
    server: &KernelServer,
) {
    use nexus_abi::bundlemgrd as wire;
    use nexus_abi::status as code;
    let op = wire::decode_request_op(frame).unwrap_or(0);
    if matches!(op, wire::OP_GET_BUNDLE_ELF | wire::OP_GET_INDEX | wire::OP_GET_FILE_VMO) {
        // TASK-0324 P7-d: the destination is the VMO this SENDER armed (`OP_ARM_VMO`); the
        // moved cap is the reply cap, answered AFTER the header write.
        let vmo_slot = armed.take(sender_service_id);
        // ELFs go only to the spawner; the index + file reads (TASK-0321
        // P5) also to packagefsd and the boot-safe allowlist.
        let allowed = is_init_sender(sender_service_id)
            || (op != wire::OP_GET_BUNDLE_ELF && is_allowed_sender(sender_service_id));
        let (status, len) = if !allowed {
            emit_sender_denied(sender_service_id);
            if let Some(slot) = vmo_slot {
                let _ = nexus_abi::cap_close(slot);
            }
            (code::VfsError::Access.code(), 0)
        } else {
            match op {
                wire::OP_GET_BUNDLE_ELF => handle_get_bundle_elf(volume, frame, vmo_slot),
                wire::OP_GET_INDEX => handle_get_index(volume, vmo_slot),
                _ => handle_get_file_vmo(volume, frame, vmo_slot),
            }
        };
        reply_done(reply, server, op, status, len);
        return;
    }
    let allowed = is_init_sender(sender_service_id) || is_allowed_sender(sender_service_id);
    let mut out = [0u8; 80];
    let n = if !allowed {
        emit_sender_denied(sender_service_id);
        let r = rsp(op, STATUS_UNSUPPORTED, 0);
        out[..8].copy_from_slice(&r);
        8
    } else if op == wire::OP_QUERY_BUNDLE {
        handle_query_bundle(volume, frame, &mut out)
    } else {
        let n = handle_volume_status(volume, &mut out);
        emit_line("bundlemgrd: volume status served");
        n
    };
    if let Some(reply) = reply {
        let _ = reply.reply_and_close_wait(&out[..n], Wait::Blocking);
    } else {
        let _ = server.send(&out[..n], Wait::Blocking);
    }
}

fn handle_query_bundle(
    volume: &mut crate::volume::VolumeState,
    frame: &[u8],
    out: &mut [u8; 80],
) -> usize {
    use nexus_abi::bundlemgrd as wire;
    let Some(name) = wire::decode_query_bundle(frame) else {
        return wire::encode_query_bundle_rsp(STATUS_MALFORMED, 0, 0, 0, &[], &[], out)
            .unwrap_or(0);
    };
    let vol = match volume.ensure() {
        Ok(v) => v,
        Err(_) => {
            return wire::encode_query_bundle_rsp(wire::STATUS_UNAVAILABLE, 0, 0, 0, &[], &[], out)
                .unwrap_or(0)
        }
    };
    match vol.lookup(name) {
        Some((row, entry)) => wire::encode_query_bundle_rsp(
            STATUS_OK,
            entry.data_len as u32,
            row.stack_pages,
            row.global_pointer,
            &row.sha256[..8],
            row.version.as_bytes(),
            out,
        )
        .unwrap_or(0),
        None => wire::encode_query_bundle_rsp(wire::STATUS_NOT_FOUND, 0, 0, 0, &[], &[], out)
            .unwrap_or(0),
    }
}

fn handle_volume_status(volume: &mut crate::volume::VolumeState, out: &mut [u8; 80]) -> usize {
    use nexus_abi::bundlemgrd as wire;
    match volume.ensure() {
        Ok(v) => {
            let end = v.build_id.iter().position(|&b| b == 0).unwrap_or(8).min(8);
            wire::encode_volume_status_rsp(
                STATUS_OK,
                v.slot,
                1,
                v.index.bundles.len().min(u16::MAX as usize) as u16,
                &v.build_id[..end],
                out,
            )
            .unwrap_or(0)
        }
        Err(_) => {
            wire::encode_volume_status_rsp(wire::STATUS_UNAVAILABLE, 0, 0, 0, &[], out).unwrap_or(0)
        }
    }
}

/// Serves one GET_BUNDLE_ELF: verifies the volume (once), streams the
/// bundle's `payload.elf` into the moved VMO while hashing it against the
/// index entry digest, then writes the payload header LAST. A digest
/// mismatch leaves an `Integrity` header — the spawner never
/// sees an `OK` header over bytes that did not verify.
fn handle_get_bundle_elf(
    volume: &mut crate::volume::VolumeState,
    frame: &[u8],
    vmo_slot: Option<u32>,
) -> (u16, u32) {
    use nexus_abi::bundlemgrd as wire;
    use nexus_abi::{payload_vmo as vmo_hdr, status as code};
    let Some(vmo) = vmo_slot else {
        emit_line("bundlemgrd: FAIL get_bundle_elf (no vmo cap)");
        return (code::VfsError::Invalid.code(), 0);
    };
    let (status, len, read_ms, hash_ms) = (|| -> (u16, u32, u32, u32) {
        let Some(name) = wire::decode_get_bundle_elf(frame) else {
            return (code::VfsError::Invalid.code(), 0, 0, 0);
        };
        let vol = match volume.ensure() {
            Ok(v) => v,
            Err(_) => return (code::VfsError::Io.code(), 0, 0, 0),
        };
        let Some((_row, entry)) = vol.lookup(name) else {
            return (code::VfsError::NotFound.code(), 0, 0, 0);
        };
        match vol.stream_entry_into_vmo(entry, vmo, vmo_hdr::DATA_OFFSET) {
            Ok((len, read_ms, hash_ms)) => (code::CODE_OK, len, read_ms, hash_ms),
            Err(crate::volume::VolumeFail::Digest) => (code::VfsError::Integrity.code(), 0, 0, 0),
            Err(crate::volume::VolumeFail::Bounds) => (code::VfsError::TooBig.code(), 0, 0, 0),
            Err(_) => (code::VfsError::Io.code(), 0, 0, 0),
        }
    })();
    let hdr = vmo_hdr::encode_header(status, len);
    let _ = nexus_abi::vmo_write(vmo, 0, &hdr);
    let _ = nexus_abi::cap_close(vmo);
    if status == code::CODE_OK {
        emit_bundle_served(frame, read_ms, hash_ms);
    } else {
        emit_line("bundlemgrd: FAIL get_bundle_elf (status)");
    }
    (status, len)
}

/// GET_INDEX (TASK-0321 P5): the NXSV-verified index bytes into the moved
/// VMO at the payload-VMO data offset, header LAST. packagefsd derives `pkg:/`
/// from exactly what bundlemgrd verified.
fn handle_get_index(volume: &mut crate::volume::VolumeState, vmo_slot: Option<u32>) -> (u16, u32) {
    use nexus_abi::{payload_vmo as vmo_hdr, status as code};
    let Some(vmo) = vmo_slot else {
        emit_line("bundlemgrd: FAIL get_index (no vmo cap)");
        return (code::VfsError::Invalid.code(), 0);
    };
    let (status, len) = match volume.ensure() {
        Ok(v) => {
            if nexus_abi::vmo_write(vmo, vmo_hdr::DATA_OFFSET, &v.index_bytes).is_err() {
                (code::VfsError::TooBig.code(), 0)
            } else {
                (code::CODE_OK, v.index_bytes.len() as u32)
            }
        }
        Err(_) => (code::VfsError::Io.code(), 0),
    };
    let hdr = vmo_hdr::encode_header(status, len);
    let _ = nexus_abi::vmo_write(vmo, 0, &hdr);
    let _ = nexus_abi::cap_close(vmo);
    if status != code::CODE_OK {
        emit_line("bundlemgrd: FAIL get_index (status)");
    }
    (status, len)
}

/// GET_FILE_VMO (TASK-0321 P5): one entry's bytes (any bundle, any path)
/// bulk-read into the moved VMO, hashed against its index digest, header
/// LAST — the `pkg:/` read path.
fn handle_get_file_vmo(
    volume: &mut crate::volume::VolumeState,
    frame: &[u8],
    vmo_slot: Option<u32>,
) -> (u16, u32) {
    use nexus_abi::bundlemgrd as wire;
    use nexus_abi::{payload_vmo as vmo_hdr, status as code};
    let Some(vmo) = vmo_slot else {
        emit_line("bundlemgrd: FAIL get_file (no vmo cap)");
        return (code::VfsError::Invalid.code(), 0);
    };
    let (status, len) = (|| -> (u16, u32) {
        let Some((bundle, path)) = wire::decode_get_file_vmo(frame) else {
            return (code::VfsError::Invalid.code(), 0);
        };
        let vol = match volume.ensure() {
            Ok(v) => v,
            Err(_) => return (code::VfsError::Io.code(), 0),
        };
        let Some(entry) = vol.lookup_entry(bundle, path) else {
            return (code::VfsError::NotFound.code(), 0);
        };
        match vol.stream_entry_into_vmo(entry, vmo, vmo_hdr::DATA_OFFSET) {
            Ok((len, _, _)) => (code::CODE_OK, len),
            Err(crate::volume::VolumeFail::Digest) => (code::VfsError::Integrity.code(), 0),
            Err(crate::volume::VolumeFail::Bounds) => (code::VfsError::TooBig.code(), 0),
            Err(_) => (code::VfsError::Io.code(), 0),
        }
    })();
    let hdr = vmo_hdr::encode_header(status, len);
    let _ = nexus_abi::vmo_write(vmo, 0, &hdr);
    let _ = nexus_abi::cap_close(vmo);
    if status != code::CODE_OK {
        emit_line("bundlemgrd: FAIL get_file (status)");
    }
    (status, len)
}

/// `bundlemgrd: bundle served (name=<n> read_ms=<r> hash_ms=<h>)` — bounded
/// (name ≤ 48 bytes); the two costs locate a slow spawn pass (device path
/// vs digest) without a profiler.
fn emit_bundle_served(frame: &[u8], read_ms: u32, hash_ms: u32) {
    use nexus_abi::bundlemgrd as wire;
    let Some(name) = wire::decode_get_bundle_elf(frame) else { return };
    let mut line = [0u8; 96];
    let prefix = b"bundlemgrd: bundle served (name=";
    line[..prefix.len()].copy_from_slice(prefix);
    let mut n = prefix.len();
    for &b in name.iter().take(48) {
        line[n] = if b.is_ascii_graphic() { b } else { b'_' };
        n += 1;
    }
    for (label, value) in [(&b" read_ms="[..], read_ms), (&b" hash_ms="[..], hash_ms)] {
        line[n..n + label.len()].copy_from_slice(label);
        n += label.len();
        n = put_dec(&mut line, n, value);
    }
    line[n] = b')';
    n += 1;
    if let Ok(s) = core::str::from_utf8(&line[..n]) {
        emit_line(s);
    }
}

fn put_dec(line: &mut [u8], at: usize, value: u32) -> usize {
    let mut digits = [0u8; 10];
    let mut d = 0;
    let mut v = value;
    loop {
        digits[d] = b'0' + (v % 10) as u8;
        d += 1;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    let mut n = at;
    while d > 0 {
        d -= 1;
        if n < line.len() {
            line[n] = digits[d];
            n += 1;
        }
    }
    n
}

/// Answers a VMO op (TASK-0324 P7-d): `[op|0x80, status, len]` on the moved reply cap — the
/// consumer's wake-up — or on the shared response endpoint when none was moved.
pub(crate) fn reply_done(
    reply: Option<nexus_ipc::ReplyCap>,
    server: &KernelServer,
    op: u8,
    status: u16,
    len: u32,
) {
    let rsp = nexus_abi::bundlemgrd::encode_payload_done_rsp(op, status, len);
    if let Some(reply) = reply {
        let _ = reply.reply_and_close_wait(&rsp, Wait::Blocking);
    } else {
        let _ = server.send(&rsp, Wait::Blocking);
    }
}

/// `OP_ARM_VMO` (TASK-0324 P7-d): the moved cap IS the destination VMO of the sender's next
/// VMO op — kept under the KERNEL sender identity. Denied senders and a full table release
/// the cap at once (fail-closed; the following op answers `Invalid`).
pub(crate) fn handle_arm_vmo(
    armed: &mut crate::armed_vmo::ArmedVmos,
    sender_service_id: u64,
    vmo_slot: Option<u32>,
) {
    let Some(vmo) = vmo_slot else {
        emit_line("bundlemgrd: FAIL arm_vmo (no vmo cap)");
        return;
    };
    if !(is_init_sender(sender_service_id) || is_allowed_sender(sender_service_id)) {
        emit_sender_denied(sender_service_id);
        let _ = nexus_abi::cap_close(vmo);
        return;
    }
    match armed.arm(sender_service_id, vmo) {
        crate::armed_vmo::Armed::Stored => {}
        crate::armed_vmo::Armed::Replaced(old) => {
            let _ = nexus_abi::cap_close(old);
        }
        crate::armed_vmo::Armed::Full(vmo) => {
            emit_line("bundlemgrd: FAIL arm_vmo (table full)");
            let _ = nexus_abi::cap_close(vmo);
        }
    }
}
