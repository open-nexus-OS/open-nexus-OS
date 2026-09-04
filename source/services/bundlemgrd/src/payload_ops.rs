// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
#![cfg(all(nexus_env = "os", feature = "os-lite"))]

//! CONTEXT: bundlemgrd's VMO-serving ops — the header-last shared-memory
//! contract shared by GET_PAYLOAD (TASK-0080D: an app's ui-program bytes
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
) {
    use nexus_abi::bundlemgrd as wire;
    let Some(vmo) = vmo_slot else {
        emit_line("bundlemgrd: FAIL get_payload (no vmo cap)");
        return;
    };
    let outcome = (|| -> (u8, u32) {
        let Some(app_id) = wire::decode_get_payload(frame) else {
            return (wire::STATUS_MALFORMED, 0);
        };
        let vol = match volume.ensure() {
            Ok(v) => v,
            Err(_) => return (wire::STATUS_UNAVAILABLE, 0),
        };
        if vol.app(app_id).is_none() {
            return (wire::PAYLOAD_STATUS_UNKNOWN, 0);
        }
        // ui-program bundles carry `payload.nxir` (nxb-pack names the payload by
        // kind); a service ELF is never served here.
        let Some(entry) = vol.lookup_entry(app_id, b"payload.nxir") else {
            return (wire::PAYLOAD_STATUS_UNKNOWN, 0);
        };
        match vol.stream_entry_into_vmo(entry, vmo, wire::PAYLOAD_DATA_OFFSET) {
            Ok((len, _, _)) => (wire::PAYLOAD_STATUS_OK, len),
            Err(crate::volume::VolumeFail::Digest) => (wire::PAYLOAD_STATUS_DIGEST, 0),
            Err(crate::volume::VolumeFail::Bounds) => (wire::PAYLOAD_STATUS_TOO_LARGE, 0),
            Err(_) => (wire::STATUS_UNAVAILABLE, 0),
        }
    })();
    let (status, len) = outcome;
    let hdr = wire::encode_payload_header(status, len);
    let _ = nexus_abi::vmo_write(vmo, 0, &hdr);
    let _ = nexus_abi::cap_close(vmo);
    if status == wire::PAYLOAD_STATUS_OK {
        metrics_counter_inc_best_effort("bundlemgrd.get_payload.ok");
        emit_line("bundlemgrd: payload served");
    } else {
        metrics_counter_inc_best_effort("bundlemgrd.get_payload.fail");
        emit_line("bundlemgrd: FAIL get_payload (status)");
    }
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
    frame: &[u8],
    sender_service_id: u64,
    reply: Option<nexus_ipc::ReplyCap>,
    server: &KernelServer,
) {
    use nexus_abi::bundlemgrd as wire;
    let op = wire::decode_request_op(frame).unwrap_or(0);
    let mut reply = reply;
    if matches!(op, wire::OP_GET_BUNDLE_ELF | wire::OP_GET_INDEX | wire::OP_GET_FILE_VMO) {
        let vmo_slot = reply.take().map(|cap| {
            let slot = cap.slot();
            core::mem::forget(cap);
            slot
        });
        // ELFs go only to the spawner; the index + file reads (TASK-0321
        // P5) also to packagefsd and the boot-safe allowlist.
        let allowed = is_init_sender(sender_service_id)
            || (op != wire::OP_GET_BUNDLE_ELF && is_allowed_sender(sender_service_id));
        if !allowed {
            emit_sender_denied(sender_service_id);
            if let Some(slot) = vmo_slot {
                let _ = nexus_abi::cap_close(slot);
            }
            return;
        }
        match op {
            wire::OP_GET_BUNDLE_ELF => handle_get_bundle_elf(volume, frame, vmo_slot),
            wire::OP_GET_INDEX => handle_get_index(volume, vmo_slot),
            _ => handle_get_file_vmo(volume, frame, vmo_slot),
        }
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
/// mismatch leaves a `PAYLOAD_STATUS_DIGEST` header — the spawner never
/// sees an `OK` header over bytes that did not verify.
fn handle_get_bundle_elf(
    volume: &mut crate::volume::VolumeState,
    frame: &[u8],
    vmo_slot: Option<u32>,
) {
    use nexus_abi::bundlemgrd as wire;
    let Some(vmo) = vmo_slot else {
        emit_line("bundlemgrd: FAIL get_bundle_elf (no vmo cap)");
        return;
    };
    let (status, len, read_ms, hash_ms) = (|| -> (u8, u32, u32, u32) {
        let Some(name) = wire::decode_get_bundle_elf(frame) else {
            return (STATUS_MALFORMED, 0, 0, 0);
        };
        let vol = match volume.ensure() {
            Ok(v) => v,
            Err(_) => return (wire::STATUS_UNAVAILABLE, 0, 0, 0),
        };
        let Some((_row, entry)) = vol.lookup(name) else {
            return (wire::PAYLOAD_STATUS_UNKNOWN, 0, 0, 0);
        };
        match vol.stream_entry_into_vmo(entry, vmo, wire::PAYLOAD_DATA_OFFSET) {
            Ok((len, read_ms, hash_ms)) => (wire::PAYLOAD_STATUS_OK, len, read_ms, hash_ms),
            Err(crate::volume::VolumeFail::Digest) => (wire::PAYLOAD_STATUS_DIGEST, 0, 0, 0),
            Err(crate::volume::VolumeFail::Bounds) => (wire::PAYLOAD_STATUS_TOO_LARGE, 0, 0, 0),
            Err(_) => (wire::STATUS_UNAVAILABLE, 0, 0, 0),
        }
    })();
    let hdr = wire::encode_payload_header(status, len);
    let _ = nexus_abi::vmo_write(vmo, 0, &hdr);
    let _ = nexus_abi::cap_close(vmo);
    if status == wire::PAYLOAD_STATUS_OK {
        emit_bundle_served(frame, read_ms, hash_ms);
    } else {
        emit_line("bundlemgrd: FAIL get_bundle_elf (status)");
    }
}

/// GET_INDEX (TASK-0321 P5): the NXSV-verified index bytes into the moved
/// VMO at `PAYLOAD_DATA_OFFSET`, header LAST. packagefsd derives `pkg:/`
/// from exactly what bundlemgrd verified.
fn handle_get_index(volume: &mut crate::volume::VolumeState, vmo_slot: Option<u32>) {
    use nexus_abi::bundlemgrd as wire;
    let Some(vmo) = vmo_slot else {
        emit_line("bundlemgrd: FAIL get_index (no vmo cap)");
        return;
    };
    let (status, len) = match volume.ensure() {
        Ok(v) => {
            if nexus_abi::vmo_write(vmo, wire::PAYLOAD_DATA_OFFSET, &v.index_bytes).is_err() {
                (wire::PAYLOAD_STATUS_TOO_LARGE, 0)
            } else {
                (wire::PAYLOAD_STATUS_OK, v.index_bytes.len() as u32)
            }
        }
        Err(_) => (wire::STATUS_UNAVAILABLE, 0),
    };
    let hdr = wire::encode_payload_header(status, len);
    let _ = nexus_abi::vmo_write(vmo, 0, &hdr);
    let _ = nexus_abi::cap_close(vmo);
    if status != wire::PAYLOAD_STATUS_OK {
        emit_line("bundlemgrd: FAIL get_index (status)");
    }
}

/// GET_FILE_VMO (TASK-0321 P5): one entry's bytes (any bundle, any path)
/// bulk-read into the moved VMO, hashed against its index digest, header
/// LAST — the `pkg:/` read path.
fn handle_get_file_vmo(
    volume: &mut crate::volume::VolumeState,
    frame: &[u8],
    vmo_slot: Option<u32>,
) {
    use nexus_abi::bundlemgrd as wire;
    let Some(vmo) = vmo_slot else {
        emit_line("bundlemgrd: FAIL get_file (no vmo cap)");
        return;
    };
    let (status, len) = (|| -> (u8, u32) {
        let Some((bundle, path)) = wire::decode_get_file_vmo(frame) else {
            return (STATUS_MALFORMED, 0);
        };
        let vol = match volume.ensure() {
            Ok(v) => v,
            Err(_) => return (wire::STATUS_UNAVAILABLE, 0),
        };
        let Some(entry) = vol.lookup_entry(bundle, path) else {
            return (wire::PAYLOAD_STATUS_UNKNOWN, 0);
        };
        match vol.stream_entry_into_vmo(entry, vmo, wire::PAYLOAD_DATA_OFFSET) {
            Ok((len, _, _)) => (wire::PAYLOAD_STATUS_OK, len),
            Err(crate::volume::VolumeFail::Digest) => (wire::PAYLOAD_STATUS_DIGEST, 0),
            Err(crate::volume::VolumeFail::Bounds) => (wire::PAYLOAD_STATUS_TOO_LARGE, 0),
            Err(_) => (wire::STATUS_UNAVAILABLE, 0),
        }
    })();
    let hdr = wire::encode_payload_header(status, len);
    let _ = nexus_abi::vmo_write(vmo, 0, &hdr);
    let _ = nexus_abi::cap_close(vmo);
    if status != wire::PAYLOAD_STATUS_OK {
        emit_line("bundlemgrd: FAIL get_file (status)");
    }
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
