// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Cross-VM remote statefs proof — `dsoftbusd_remote_statefs_rw_roundtrip`
//!   exercises a put/get/delete cycle against the peer node's statefs via the
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
use statefs::protocol as statefs_proto;

use super::super::super::ipc::clients::cached_dsoftbusd_client;
use super::REMOTE_DSOFTBUS_WAIT_MS;

pub(crate) fn dsoftbusd_remote_statefs_call(
    req: &[u8],
) -> core::result::Result<alloc::vec::Vec<u8>, ()> {
    const D0: u8 = b'D';
    const D1: u8 = b'S';
    const VER: u8 = 1;
    const OP: u8 = 7;
    const STATUS_OK: u8 = 0;

    let d = cached_dsoftbusd_client().map_err(|_| ())?;
    if req.is_empty() || req.len() > 256 {
        return Err(());
    }
    let mut frame = alloc::vec::Vec::with_capacity(6 + req.len());
    frame.extend_from_slice(&[D0, D1, VER, OP]);
    frame.extend_from_slice(&(req.len() as u16).to_le_bytes());
    frame.extend_from_slice(req);
    d.send(&frame, IpcWait::Timeout(core::time::Duration::from_millis(REMOTE_DSOFTBUS_WAIT_MS)))
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
    if n == 0 || rsp.len() != 7 + n {
        return Err(());
    }
    Ok(rsp[7..7 + n].to_vec())
}

pub(crate) fn dsoftbusd_remote_statefs_put(
    key: &str,
    value: &[u8],
) -> core::result::Result<(), ()> {
    let req = statefs_proto::encode_put_request(key, value).map_err(|_| ())?;
    let rsp = dsoftbusd_remote_statefs_call(&req)?;
    let status =
        statefs_proto::decode_status_response(statefs_proto::OP_PUT, &rsp).map_err(|_| ())?;
    if status == statefs_proto::STATUS_OK {
        Ok(())
    } else {
        Err(())
    }
}

pub(crate) fn dsoftbusd_remote_statefs_get(
    key: &str,
) -> core::result::Result<alloc::vec::Vec<u8>, ()> {
    let req = statefs_proto::encode_key_only_request(statefs_proto::OP_GET, key).map_err(|_| ())?;
    let rsp = dsoftbusd_remote_statefs_call(&req)?;
    statefs_proto::decode_get_response(&rsp).map_err(|_| ())
}

pub(crate) fn dsoftbusd_remote_statefs_delete(key: &str) -> core::result::Result<(), ()> {
    let req = statefs_proto::encode_key_only_request(statefs_proto::OP_DEL, key).map_err(|_| ())?;
    let rsp = dsoftbusd_remote_statefs_call(&req)?;
    let status =
        statefs_proto::decode_status_response(statefs_proto::OP_DEL, &rsp).map_err(|_| ())?;
    if status == statefs_proto::STATUS_OK {
        Ok(())
    } else {
        Err(())
    }
}

pub(crate) fn dsoftbusd_remote_statefs_rw_roundtrip() -> core::result::Result<(), ()> {
    let key = "/state/shared/selftest/remote-rw";
    let value = b"remote-statefs-rw-v1";
    dsoftbusd_remote_statefs_put(key, value)?;
    let got = dsoftbusd_remote_statefs_get(key)?;
    if got.as_slice() != value {
        return Err(());
    }
    dsoftbusd_remote_statefs_delete(key)?;
    match dsoftbusd_remote_statefs_get(key) {
        Ok(_) => Err(()),
        Err(_) => Ok(()),
    }
}
