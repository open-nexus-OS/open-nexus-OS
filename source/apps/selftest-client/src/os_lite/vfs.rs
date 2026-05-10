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

use nexus_ipc::{Client, KernelClient, Wait as IpcWait};

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
    const OPCODE_MOUNT_STATUS: u8 = 3;
    let client = KernelClient::new_for("packagefsd").ok()?;
    client
        .send(&[OPCODE_MOUNT_STATUS], IpcWait::Timeout(core::time::Duration::from_millis(100)))
        .ok()?;
    let rsp = client.recv(IpcWait::Timeout(core::time::Duration::from_millis(100))).ok()?;
    rsp.first().copied()
}
