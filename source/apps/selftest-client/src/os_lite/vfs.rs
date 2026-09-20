// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Userspace VFS verification helper — `verify_vfs()` exercises the
//!   cross-process VFS surface (vfsd / packagefsd / pkgfs) over kernel IPC v1
//!   and emits the granular routing/lookup markers consumed by `phases::vfs`.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) — vfs phase, including explicit pkgimg mount-mode probe.
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

use nexus_ipc::KernelClient;

use crate::markers::emit_line;

pub(crate) fn verify_vfs() -> Result<(), ()> {
    // RFC-0005: name-based routing (slots are assigned by init-lite; lookup happens over a
    // private control endpoint).
    let _ = KernelClient::new_for("vfsd").map_err(|_| ())?;
    emit_line(crate::markers::M_SELFTEST_IPC_ROUTING_OK);
    let _ = KernelClient::new_for("packagefsd").map_err(|_| ())?;
    emit_line(crate::markers::M_SELFTEST_IPC_ROUTING_PACKAGEFSD_OK);

    // Use the nexus-vfs OS backend (no raw opcode frames in the app).
    let vfs = match nexus_vfs::VfsClient::new() {
        Ok(vfs) => vfs,
        Err(_) => {
            emit_line(crate::markers::M_SELFTEST_VFS_CLIENT_NEW_FAIL);
            return Err(());
        }
    };

    // stat
    let _meta = vfs.stat("pkg:/system/build.prop").map_err(|_| {
        emit_line(crate::markers::M_SELFTEST_VFS_STAT_FAIL);
    })?;
    emit_line(crate::markers::M_SELFTEST_VFS_STAT_OK);
    let mode = query_pkgimg_mount_mode().ok_or(())?;
    if mode == 0 {
        return Err(());
    }
    emit_line(crate::markers::M_SELFTEST_PKGIMG_MOUNT_OK);

    // open
    let fh = vfs.open("pkg:/system/build.prop").map_err(|_| {
        emit_line(crate::markers::M_SELFTEST_VFS_OPEN_FAIL);
    })?;

    // read
    let _bytes = vfs.read(fh, 0, 64).map_err(|_| {
        emit_line(crate::markers::M_SELFTEST_VFS_READ_FAIL);
    })?;
    emit_line(crate::markers::M_SELFTEST_VFS_READ_OK);
    emit_line(crate::markers::M_SELFTEST_CAPFD_READ_OK);

    // real data: deterministic bytes from packagefsd via vfsd
    let fh = vfs.open("pkg:/system/build.prop").map_err(|_| ())?;
    let got = vfs.read(fh, 0, 64).map_err(|_| ())?;
    let expect: &[u8] = b"ro.nexus.build=dev\n";
    if !got.as_slice().starts_with(expect) {
        emit_line(crate::markers::M_SELFTEST_VFS_REAL_DATA_FAIL);
        return Err(());
    }
    emit_line(crate::markers::M_SELFTEST_VFS_REAL_DATA_OK);
    emit_line(crate::markers::M_SELFTEST_PKGIMG_STAT_READ_OK);

    // readdir (RFC-0072 Phase 1 / TASK-0291): the namespace root lists the
    // active bundles; a real page with >= 1 entry is the positive proof.
    match vfs.read_dir("pkg:/", 0, 64) {
        Ok(page) if !page.entries.is_empty() => {
            emit_line(crate::markers::M_SELFTEST_VFS_READDIR_OK);
        }
        _ => return Err(()),
    }
    // readdir negative: an unknown bundle must fail with the stable
    // ENOTFOUND code (never an empty fake page).
    match vfs.read_dir("pkg:/__definitely_missing_bundle__", 0, 64) {
        Err(nexus_vfs::Error::Vfs(nexus_vfs_types::VfsError::NotFound)) => {
            emit_line(crate::markers::M_SELFTEST_VFS_READDIR_DENY_OK);
        }
        _ => return Err(()),
    }

    // Zero-copy VMO splice (RFC-0072 Phase 3 / TASK-0295): the same file read
    // through a CAP_MOVE'd VMO must be byte-identical to the inline read — this
    // proves the splice data plane actually engaged (not a silent fallback).
    let splice_fh = vfs.open("pkg:/system/build.prop").map_err(|_| ())?;
    let inline = vfs.read(splice_fh, 0, 4096).map_err(|_| ())?;
    match vfs.read_vmo("pkg:/system/build.prop", 64 * 1024) {
        Ok(spliced) if !spliced.is_empty() && spliced == inline => {
            emit_line(crate::markers::M_SELFTEST_VFS_SPLICE_ROUNDTRIP_OK);
        }
        _ => return Err(()),
    }
    // Inline reads above INLINE_IO_MAX are a protocol error (E2BIG), never a
    // silent slow path — the large read must use the VMO splice instead.
    match vfs.read(splice_fh, 0, 5000) {
        Err(nexus_vfs::Error::Vfs(nexus_vfs_types::VfsError::TooBig)) => {
            emit_line(crate::markers::M_SELFTEST_VFS_INLINE_OVERSIZE_DENY_OK);
        }
        _ => return Err(()),
    }
    vfs.close(splice_fh).map_err(|_| ())?;

    // TASK-0033 P2 (RFC-0097 §2): the same path with an entry from the class
    // that could not be read AT ALL before — `pkg:/` used to ship entry bytes
    // inline in packagefsd's reply, so 22 of the volume's 115 entries were both
    // unreadable and fatal to the service. Everything above this line is proven
    // on `build.prop`: 19 bytes.
    //
    // MountMode::SystemVolume == 4; the seed registry (recovery / direct-kernel
    // boots) has no bulk entry to read, and claiming one would be a lie.
    if mode == 4 {
        const BULK: &str = "pkg:/settings/payload.nxir";
        /// The old inline-reply ceiling: `IPC_PAYLOAD_MAX` minus the 11-byte
        /// metadata header. A "bulk" read at or below this proves nothing.
        const OLD_CEILING: u64 = 8181;
        let meta = vfs.stat(BULK).map_err(|_| {
            emit_line(crate::markers::M_SELFTEST_PKGIMG_VMO_FAIL_STAT);
        })?;
        if meta.size() <= OLD_CEILING {
            emit_line(crate::markers::M_SELFTEST_PKGIMG_VMO_FAIL_ENTRY_NOT_ABOVE_THE_OLD_CEILING);
            return Err(());
        }
        let cap = nexus_vfs_types::SPLICE_DATA_OFFSET + meta.size() as usize;
        // A VMO too small for the entry must be refused in the header, and the
        // service must still be serving afterwards — the read below is what
        // proves it survived. HALF the entry, not one byte less: `vmo_create`
        // rounds up to a page, so a one-byte shortfall is not a shortfall.
        let mut sample = [0u8; 64];
        match vfs.read_vmo_sample(BULK, cap / 2, &mut sample) {
            Err(nexus_vfs::Error::Vfs(nexus_vfs_types::VfsError::TooBig)) => {
                emit_line(crate::markers::M_SELFTEST_PKGIMG_VMO_OVERSIZE_DENY_OK);
            }
            _ => {
                emit_line(crate::markers::M_SELFTEST_PKGIMG_VMO_FAIL_UNDERSIZED_VMO_NOT_REFUSED);
                return Err(());
            }
        }
        match vfs.read_vmo_sample(BULK, cap, &mut sample) {
            Ok(len) if len as u64 == meta.size() && sample.iter().any(|&b| b != 0) => {
                // ONE write, not three. A marker assembled from several
                // writes can be SPLIT by another service's line landing
                // between them — the evidence assembler then sees a marker
                // that never existed and rejects the whole trace. Observed:
                // `SELFTEST: pkgimg vmo packagefsd: read vmo forwarded (...)`.
                let mut line = [0u8; 80];
                let mut n = 0usize;
                for &b in crate::markers::M_SELFTEST_PKGIMG_VMO_OK_BYTES_0X.as_bytes() {
                    if n < line.len() {
                        line[n] = b;
                        n += 1;
                    }
                }
                const HEX: &[u8; 16] = b"0123456789abcdef";
                let v = len as u64;
                for shift in (0..16).rev() {
                    if n < line.len() {
                        line[n] = HEX[((v >> (shift * 4)) & 0xF) as usize];
                        n += 1;
                    }
                }
                if n < line.len() {
                    line[n] = b')';
                    n += 1;
                }
                // Every byte written above is ASCII, so this cannot fail; if it
                // somehow did, the honest line is the FAILURE, never an "ok"
                // without the number that makes it checkable.
                match core::str::from_utf8(&line[..n]) {
                    Ok(line) => emit_line(line),
                    Err(_) => {
                        emit_line(crate::markers::M_SELFTEST_PKGIMG_VMO_FAIL_BULK_READ);
                        return Err(());
                    }
                }
            }
            _ => {
                emit_line(crate::markers::M_SELFTEST_PKGIMG_VMO_FAIL_BULK_READ);
                return Err(());
            }
        }
    }

    // traversal deny path (userspace confinement floor)
    if vfs.stat("pkg:/system/../secrets.txt").is_err() {
        emit_line(crate::markers::M_SELFTEST_SANDBOX_DENY_OK);
    } else {
        return Err(());
    }

    // Force one server-side deny path (not just client-side path prevalidation),
    // so the `vfsd: access denied` marker is backed by an actual vfsd decision.
    if vfs.stat("pkg:/system/__definitely_missing_for_deny_marker__.txt").is_ok() {
        return Err(());
    }

    // close
    vfs.close(fh).map_err(|_| ())?;

    // ebadf: read after close should fail
    if vfs.read(fh, 0, 1).is_err() {
        emit_line(crate::markers::M_SELFTEST_VFS_EBADF_OK);
        Ok(())
    } else {
        Err(())
    }
}

fn query_pkgimg_mount_mode() -> Option<u8> {
    // packagefsd os-lite control opcode for truthful mount-mode evidence.
    // It answers exactly the senders that moved a reply cap (TASK-0033 P2), so
    // this goes through the declared leg and the harness' own reply inbox.
    const OPCODE_MOUNT_STATUS: u8 = 3;
    let slots = nexus_service_topology::slots::selftest_client::PACKAGEFSD;
    let reply = nexus_service_topology::slots::selftest_client::REPLY;
    let mut rsp = [0u8; 8];
    let n =
        nexus_ipc::exchange::call_into(slots.send, reply, &[OPCODE_MOUNT_STATUS], &mut rsp).ok()?;
    if n == 0 {
        return None;
    }
    Some(rsp[0])
}
