// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(all(nexus_env = "os", feature = "os-lite"))]

//! CONTEXT: vfsd's VMO-splice data plane (RFC-0072 Phase 3; split from
//! `os_lite.rs` under the structure ratchet). The `/data` provider STREAMS
//! in fixed windows straight into the caller's VMO — this service runs on
//! a bump heap that never frees, so a per-request window buffer would leak
//! the size of every file ever spliced (that exact shape killed the
//! service on a 19 MB OTA container). Payload lands FIRST and the header
//! LAST: the magic is the release fence a polling client waits on.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU ladder (`vfsd: vmo splice …` markers)
//! ADR: docs/adr/0043-user-data-in-dedicated-cow-fs-statefs-stays-service-kv.md

extern crate alloc;

use alloc::format;

use nexus_vfs_types::VfsError;

use crate::namespace::Namespace;
use crate::os_lite::{debug_print, is_home_path, vmo_len};

/// Writes the splice header at VMO offset 0. Call this AFTER the payload write
/// (release ordering): a client that sees the magic must see complete bytes.
fn write_splice_header(vmo: u32, status: u16, len: u32) {
    let hdr = nexus_vfs_types::encode_splice_header(status, len);
    let _ = nexus_abi::vmo_write(vmo, 0, &hdr);
}

/// Serves an `OP_READ_VMO` request (RFC-0072 Phase 3): resolve the path to
/// bytes (nxfs `/data` or read-only `pkg:/`), write them into the caller's
/// moved VMO payload-first + header-last, then close the moved cap. The header
/// carries the RFC-0072 status; oversize-for-VMO is `E2BIG`, never truncated.
#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_read_vmo(
    frame: &[u8],
    vmo_slot: Option<u32>,
    namespace: &Namespace,
    data: &mut Option<nxfsd::DataStore>,
    data_attempts: &mut u8,
    max_data_attempts: u8,
    splice_bytes: &mut u64,
    splice_fallbacks: &mut u64,
    window: &mut [u8],
) {
    let Some(vmo) = vmo_slot else {
        *splice_fallbacks += 1;
        debug_print("vfsd: FAIL splice (no vmo cap)\n");
        return;
    };
    let path = match nexus_vfs_types::decode_read_vmo_request(&frame[1..]) {
        Some(path) => path,
        None => {
            write_splice_header(vmo, VfsError::Invalid.code(), 0);
            let _ = nexus_abi::cap_close(vmo);
            return;
        }
    };
    // The caller's VMO capacity bounds the payload (minus the header prefix).
    let max_payload = match vmo_len(vmo) {
        Some(cap) if cap > nexus_vfs_types::SPLICE_DATA_OFFSET => {
            cap - nexus_vfs_types::SPLICE_DATA_OFFSET
        }
        _ => {
            *splice_fallbacks += 1;
            write_splice_header(vmo, VfsError::Io.code(), 0);
            let _ = nexus_abi::cap_close(vmo);
            return;
        }
    };
    // Resolve bytes from the owning provider (one surface, two providers).
    // /data (nxfs) STREAMS in 64 KiB windows straight into the VMO — the
    // TASK-0179 apply engine pulls ~19 MB containers through here and this
    // service runs on a bump heap that must never see a file-sized Vec.
    if is_home_path(&path) {
        if data.is_none() && *data_attempts < max_data_attempts {
            *data_attempts += 1;
            *data = nxfsd::DataStore::acquire();
        }
        let Some(store) = data.as_ref() else {
            write_splice_header(vmo, VfsError::Io.code(), 0);
            let _ = nexus_abi::cap_close(vmo);
            return;
        };
        let size = match store.stat_size(&path) {
            Ok(size) if size as usize <= max_payload => size as usize,
            Ok(_) => {
                write_splice_header(vmo, VfsError::TooBig.code(), 0);
                let _ = nexus_abi::cap_close(vmo);
                return;
            }
            Err(err) => {
                write_splice_header(vmo, err.code(), 0);
                let _ = nexus_abi::cap_close(vmo);
                return;
            }
        };
        let mut off = 0usize;
        while off < size {
            let take = window.len().min(size - off);
            match store.read_window(&path, off as u64, &mut window[..take]) {
                Ok(filled) if filled == take => {
                    if nexus_abi::vmo_write(
                        vmo,
                        nexus_vfs_types::SPLICE_DATA_OFFSET + off,
                        &window[..take],
                    )
                    .is_err()
                    {
                        write_splice_header(vmo, VfsError::Io.code(), 0);
                        let _ = nexus_abi::cap_close(vmo);
                        return;
                    }
                }
                _ => {
                    write_splice_header(vmo, VfsError::Io.code(), 0);
                    let _ = nexus_abi::cap_close(vmo);
                    return;
                }
            }
            off += take;
            let _ = nexus_abi::yield_();
        }
        // Payload landed first; the header is the release fence.
        write_splice_header(vmo, nexus_vfs_types::CODE_OK, size as u32);
        *splice_bytes = splice_bytes.saturating_add(size as u64);
        debug_print(&format!(
            "vfsd: vmo splice stream ok (bytes={size}, fallbacks={})\n",
            *splice_fallbacks
        ));
        let _ = nexus_abi::cap_close(vmo);
        return;
    }
    if !path.starts_with("pkg:/") {
        write_splice_header(vmo, VfsError::NotFound.code(), 0);
        let _ = nexus_abi::cap_close(vmo);
        return;
    }
    // THE PASS-THROUGH (TASK-0033 P2, RFC-0097 §2). The caller's VMO goes on to
    // packagefsd and from there to bundlemgrd, which verifies the entry digest,
    // writes the bytes, and writes the success header LAST. vfsd reads no byte
    // of it and writes no success header: this hop is not the authority, and it
    // used to carry the whole entry through its own bump heap on the way — which
    // is why 22 of the volume's 115 entries could not be read at all.
    //
    // `max_payload` is still computed above so an obviously-too-small VMO is
    // refused before a capability is moved; the authoritative bound is
    // bundlemgrd's, against this same VMO.
    let _ = max_payload;
    match namespace.packagefs_forward_vmo(&path, vmo) {
        Ok(()) => {
            if let Some((_, len)) = read_back_header(vmo) {
                *splice_bytes = splice_bytes.saturating_add(u64::from(len));
                debug_print(&format!(
                    "vfsd: vmo splice forwarded ok (bytes={len}, fallbacks={})\n",
                    *splice_fallbacks
                ));
            }
        }
        Err(err) => {
            *splice_fallbacks += 1;
            // Only an ERROR header, and only because the hop that should have
            // written one could not be reached.
            write_splice_header(vmo, err.code(), 0);
        }
    }
    let _ = nexus_abi::cap_close(vmo);
}

/// Reads back the header the authority wrote, for accounting only.
fn read_back_header(vmo: u32) -> Option<(u16, u32)> {
    let mut hdr = [0u8; nexus_vfs_types::SPLICE_HEADER_LEN];
    nexus_abi::vmo_read(vmo, 0, &mut hdr).ok()?;
    nexus_vfs_types::decode_splice_header(&hdr)
}
