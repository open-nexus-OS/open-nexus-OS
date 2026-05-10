// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Cross-VM remote pkgfs proof — `dsoftbusd_remote_pkgfs_read_once`
//!   issues a bounded read against the peer node's pkgfs surface via the
//!   dsoftbusd RPC bridge (RFC-0019 nonce-correlated request/reply).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU 2-VM marker ladder (`tools/os2vm.sh` / Node A); single-VM
//!   smoke skips this proof by design.
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

extern crate alloc;

use nexus_ipc::{Client, Wait as IpcWait};

use super::super::super::ipc::clients::cached_dsoftbusd_client;
use super::REMOTE_DSOFTBUS_WAIT_MS;
use crate::markers::emit_line;

pub(crate) fn dsoftbusd_remote_pkgfs_stat(path: &str) -> core::result::Result<(u64, u16), ()> {
    const D0: u8 = b'D';
    const D1: u8 = b'S';
    const VER: u8 = 1;
    const OP: u8 = 3;
    const STATUS_OK: u8 = 0;
    let d = cached_dsoftbusd_client().map_err(|_| ())?;
    let p = path.as_bytes();
    if p.is_empty() || p.len() > 192 {
        return Err(());
    }
    let mut req = alloc::vec::Vec::with_capacity(5 + p.len());
    req.extend_from_slice(&[D0, D1, VER, OP, p.len() as u8]);
    req.extend_from_slice(p);
    d.send(&req, IpcWait::Timeout(core::time::Duration::from_millis(REMOTE_DSOFTBUS_WAIT_MS)))
        .map_err(|_| ())?;
    let rsp = d
        .recv(IpcWait::Timeout(core::time::Duration::from_millis(REMOTE_DSOFTBUS_WAIT_MS)))
        .map_err(|_| ())?;
    if rsp.len() != 15 || rsp[0] != D0 || rsp[1] != D1 || rsp[2] != VER || rsp[3] != (OP | 0x80) {
        return Err(());
    }
    if rsp[4] != STATUS_OK {
        return Err(());
    }
    let size =
        u64::from_le_bytes([rsp[5], rsp[6], rsp[7], rsp[8], rsp[9], rsp[10], rsp[11], rsp[12]]);
    let kind = u16::from_le_bytes([rsp[13], rsp[14]]);
    Ok((size, kind))
}

pub(crate) fn dsoftbusd_remote_pkgfs_open(path: &str) -> core::result::Result<u32, ()> {
    const D0: u8 = b'D';
    const D1: u8 = b'S';
    const VER: u8 = 1;
    const OP: u8 = 4;
    const STATUS_OK: u8 = 0;
    let d = cached_dsoftbusd_client().map_err(|_| ())?;
    let p = path.as_bytes();
    if p.is_empty() || p.len() > 192 {
        return Err(());
    }
    let mut req = alloc::vec::Vec::with_capacity(5 + p.len());
    req.extend_from_slice(&[D0, D1, VER, OP, p.len() as u8]);
    req.extend_from_slice(p);
    d.send(&req, IpcWait::Timeout(core::time::Duration::from_millis(REMOTE_DSOFTBUS_WAIT_MS)))
        .map_err(|_| ())?;
    let rsp = d
        .recv(IpcWait::Timeout(core::time::Duration::from_millis(REMOTE_DSOFTBUS_WAIT_MS)))
        .map_err(|_| ())?;
    if rsp.len() != 9 || rsp[0] != D0 || rsp[1] != D1 || rsp[2] != VER || rsp[3] != (OP | 0x80) {
        return Err(());
    }
    if rsp[4] != STATUS_OK {
        return Err(());
    }
    Ok(u32::from_le_bytes([rsp[5], rsp[6], rsp[7], rsp[8]]))
}

pub(crate) fn dsoftbusd_remote_pkgfs_read(
    handle: u32,
    offset: u32,
    read_len: u16,
) -> core::result::Result<alloc::vec::Vec<u8>, ()> {
    const D0: u8 = b'D';
    const D1: u8 = b'S';
    const VER: u8 = 1;
    const OP: u8 = 5;
    const STATUS_OK: u8 = 0;
    let d = cached_dsoftbusd_client().map_err(|_| ())?;
    if read_len == 0 || read_len > 128 {
        return Err(());
    }
    let mut req = [0u8; 14];
    req[0] = D0;
    req[1] = D1;
    req[2] = VER;
    req[3] = OP;
    req[4..8].copy_from_slice(&handle.to_le_bytes());
    req[8..12].copy_from_slice(&offset.to_le_bytes());
    req[12..14].copy_from_slice(&read_len.to_le_bytes());
    d.send(&req, IpcWait::Timeout(core::time::Duration::from_millis(REMOTE_DSOFTBUS_WAIT_MS)))
        .map_err(|_| ())?;
    let rsp = d
        .recv(IpcWait::Timeout(core::time::Duration::from_millis(REMOTE_DSOFTBUS_WAIT_MS)))
        .map_err(|_| ())?;
    if rsp.len() < 7 || rsp[0] != D0 || rsp[1] != D1 || rsp[2] != VER || rsp[3] != (OP | 0x80) {
        return Err(());
    }
    if rsp[4] != STATUS_OK {
        return Err(());
    }
    let n = u16::from_le_bytes([rsp[5], rsp[6]]) as usize;
    if n > 128 || rsp.len() < 7 + n {
        return Err(());
    }
    Ok(rsp[7..7 + n].to_vec())
}

pub(crate) fn dsoftbusd_remote_pkgfs_close(handle: u32) -> core::result::Result<(), ()> {
    const D0: u8 = b'D';
    const D1: u8 = b'S';
    const VER: u8 = 1;
    const OP: u8 = 6;
    const STATUS_OK: u8 = 0;
    let d = cached_dsoftbusd_client().map_err(|_| ())?;
    let mut req = [0u8; 8];
    req[0] = D0;
    req[1] = D1;
    req[2] = VER;
    req[3] = OP;
    req[4..8].copy_from_slice(&handle.to_le_bytes());
    d.send(&req, IpcWait::Timeout(core::time::Duration::from_millis(REMOTE_DSOFTBUS_WAIT_MS)))
        .map_err(|_| ())?;
    let rsp = d
        .recv(IpcWait::Timeout(core::time::Duration::from_millis(REMOTE_DSOFTBUS_WAIT_MS)))
        .map_err(|_| ())?;
    if rsp.len() != 5 || rsp[0] != D0 || rsp[1] != D1 || rsp[2] != VER || rsp[3] != (OP | 0x80) {
        return Err(());
    }
    if rsp[4] != STATUS_OK {
        return Err(());
    }
    Ok(())
}

pub(crate) fn dsoftbusd_remote_pkgfs_read_once(
    path: &str,
    read_len: u16,
) -> core::result::Result<alloc::vec::Vec<u8>, ()> {
    let (_size, kind) = match dsoftbusd_remote_pkgfs_stat(path) {
        Ok(v) => v,
        Err(()) => return Err(()),
    };
    if kind != 0 {
        return Err(());
    }
    emit_line(crate::markers::M_SELFTEST_REMOTE_PKGFS_STAT_OK);
    let handle = match dsoftbusd_remote_pkgfs_open(path) {
        Ok(v) => v,
        Err(()) => return Err(()),
    };
    emit_line(crate::markers::M_SELFTEST_REMOTE_PKGFS_OPEN_OK);
    let read_bytes = match dsoftbusd_remote_pkgfs_read(handle, 0, read_len) {
        Ok(v) => v,
        Err(()) => {
            let _ = dsoftbusd_remote_pkgfs_close(handle);
            return Err(());
        }
    };
    if read_bytes.is_empty() {
        let _ = dsoftbusd_remote_pkgfs_close(handle);
        return Err(());
    }
    emit_line(crate::markers::M_SELFTEST_REMOTE_PKGFS_READ_STEP_OK);
    if dsoftbusd_remote_pkgfs_close(handle).is_err() {
        return Err(());
    }
    emit_line(crate::markers::M_SELFTEST_REMOTE_PKGFS_CLOSE_OK);
    Ok(read_bytes)
}
