// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: statefs IPC client + selftest probes — namespaced KV CRUD,
//!   persist/restore roundtrip, oversize-key/value rejects, and the cross-VM
//!   roundtrip helper consumed by `phases::remote`.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) — bringup + remote phases.
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use crash::{deterministic_build_id, MinidumpFrame};
use nexus_abi::{yield_, Pid};
use nexus_ipc::{Client, KernelClient, Wait as IpcWait};
use statefs::protocol as statefs_proto;
use statefs::StatefsError;

use crate::markers::{emit_bytes, emit_hex_u64, emit_line};

pub(crate) fn statefs_send_recv(
    client: &KernelClient,
    frame: &[u8],
) -> core::result::Result<Vec<u8>, ()> {
    // Deterministic: upgrade request to SF v2 (nonce) and only accept the matching reply.
    static NONCE: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(1);
    let nonce = NONCE.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    if frame.len() < 4 {
        return Err(());
    }
    let mut v2 = Vec::with_capacity(frame.len().saturating_add(8));
    v2.extend_from_slice(&frame[..4]);
    v2[2] = statefs_proto::VERSION_V2;
    v2.extend_from_slice(&nonce.to_le_bytes());
    v2.extend_from_slice(&frame[4..]);

    if let Err(err) = client.send(&v2, IpcWait::Timeout(core::time::Duration::from_millis(2000))) {
        match err {
            nexus_ipc::IpcError::WouldBlock => {
                emit_line(crate::markers::M_SELFTEST_STATEFS_SEND_WOULD_BLOCK)
            }
            nexus_ipc::IpcError::Timeout => {
                emit_line(crate::markers::M_SELFTEST_STATEFS_SEND_TIMEOUT)
            }
            nexus_ipc::IpcError::Disconnected => {
                emit_line(crate::markers::M_SELFTEST_STATEFS_SEND_DISCONNECTED)
            }
            nexus_ipc::IpcError::NoSpace => {
                emit_line(crate::markers::M_SELFTEST_STATEFS_SEND_NO_SPACE)
            }
            nexus_ipc::IpcError::Kernel(_) => {
                emit_line(crate::markers::M_SELFTEST_STATEFS_SEND_KERNEL_ERROR)
            }
            nexus_ipc::IpcError::Unsupported => {
                emit_line(crate::markers::M_SELFTEST_STATEFS_SEND_UNSUPPORTED)
            }
            _ => emit_line(crate::markers::M_SELFTEST_STATEFS_SEND_OTHER),
        }
        emit_line(crate::markers::M_SELFTEST_STATEFS_SEND_FAIL);
        return Err(());
    }
    let start = nexus_abi::nsec().map_err(|_| ())?;
    let deadline = start.saturating_add(2_000_000_000);
    loop {
        let now = nexus_abi::nsec().map_err(|_| ())?;
        if now >= deadline {
            emit_line(crate::markers::M_SELFTEST_STATEFS_RECV_TIMEOUT);
            return Err(());
        }
        match client.recv(IpcWait::NonBlocking) {
            Ok(rsp) => {
                if rsp.len() < 13
                    || rsp[0] != statefs_proto::MAGIC0
                    || rsp[1] != statefs_proto::MAGIC1
                    || rsp[2] != statefs_proto::VERSION_V2
                {
                    continue;
                }
                let got_nonce = u64::from_le_bytes([
                    rsp[5], rsp[6], rsp[7], rsp[8], rsp[9], rsp[10], rsp[11], rsp[12],
                ]);
                if got_nonce != nonce {
                    continue;
                }
                return Ok(rsp);
            }
            Err(nexus_ipc::IpcError::WouldBlock) => {
                let _ = yield_();
            }
            Err(_) => return Err(()),
        }
    }
}

pub(crate) fn statefs_put_get_list(client: &KernelClient) -> core::result::Result<(), ()> {
    let key = "/state/selftest/ping";
    let value = b"ok";
    let put = statefs_proto::encode_put_request(key, value).map_err(|_| ())?;
    let rsp = statefs_send_recv(client, &put)?;
    let status =
        statefs_proto::decode_status_response(statefs_proto::OP_PUT, &rsp).map_err(|_| ())?;
    if status != statefs_proto::STATUS_OK {
        return Err(());
    }

    let get = statefs_proto::encode_key_only_request(statefs_proto::OP_GET, key).map_err(|_| ())?;
    let rsp = statefs_send_recv(client, &get)?;
    let got = match statefs_proto::decode_get_response(&rsp) {
        Ok(bytes) => bytes,
        Err(err) => {
            emit_bytes(crate::markers::M_SELFTEST_STATEFS_PERSIST_GET_ERR.as_bytes());
            emit_hex_u64(statefs_proto::status_from_error(err) as u64);
            emit_bytes(b" rsp_len=");
            emit_hex_u64(rsp.len() as u64);
            emit_bytes(b" b0=");
            emit_hex_u64(*rsp.get(0).unwrap_or(&0) as u64);
            emit_bytes(b" b3=");
            emit_hex_u64(*rsp.get(3).unwrap_or(&0) as u64);
            emit_line(")");
            return Err(());
        }
    };
    if got.as_slice() != value {
        return Err(());
    }

    let list = statefs_proto::encode_list_request("/state/selftest/", 16).map_err(|_| ())?;
    let rsp = statefs_send_recv(client, &list)?;
    let keys = statefs_proto::decode_list_response(&rsp).map_err(|_| ())?;
    if !keys.iter().any(|k| k == key) {
        return Err(());
    }
    Ok(())
}

pub(crate) fn statefs_unauthorized_access(client: &KernelClient) -> core::result::Result<(), ()> {
    let get = statefs_proto::encode_key_only_request(statefs_proto::OP_GET, "/state/keystore/deny")
        .map_err(|_| ())?;
    let rsp = statefs_send_recv(client, &get)?;
    match statefs_proto::decode_get_response(&rsp) {
        Err(StatefsError::AccessDenied) => Ok(()),
        _ => {
            if let Ok(status) = statefs_proto::decode_status_response(statefs_proto::OP_GET, &rsp) {
                if status == statefs_proto::STATUS_ACCESS_DENIED {
                    return Ok(());
                }
                emit_bytes(crate::markers::M_SELFTEST_STATEFS_UNAUTHORIZED_STATUS.as_bytes());
                emit_hex_u64(status as u64);
                emit_line(")");
            } else {
                emit_bytes(crate::markers::M_SELFTEST_STATEFS_UNAUTHORIZED_RSP_LEN.as_bytes());
                emit_hex_u64(rsp.len() as u64);
                emit_line(")");
            }
            Err(())
        }
    }
}

pub(crate) fn statefs_persist(client: &KernelClient) -> core::result::Result<(), ()> {
    emit_line(crate::markers::M_SELFTEST_STATEFS_PERSIST_BEGIN);
    let key = "/state/selftest/persist";
    let value = b"persist-ok";
    let put = statefs_proto::encode_put_request(key, value).map_err(|_| ())?;
    let rsp = statefs_send_recv(client, &put)?;
    let status = match statefs_proto::decode_status_response(statefs_proto::OP_PUT, &rsp) {
        Ok(status) => status,
        Err(_) => {
            emit_bytes(crate::markers::M_SELFTEST_STATEFS_PERSIST_PUT_RSP_LEN.as_bytes());
            emit_hex_u64(rsp.len() as u64);
            emit_bytes(b" b0=");
            emit_hex_u64(*rsp.get(0).unwrap_or(&0) as u64);
            emit_bytes(b" b3=");
            emit_hex_u64(*rsp.get(3).unwrap_or(&0) as u64);
            emit_line(")");
            return Err(());
        }
    };
    if status != statefs_proto::STATUS_OK {
        emit_bytes(crate::markers::M_SELFTEST_STATEFS_PERSIST_PUT_STATUS.as_bytes());
        emit_hex_u64(status as u64);
        emit_line(")");
        return Err(());
    }
    emit_line(crate::markers::M_SELFTEST_STATEFS_PERSIST_PUT_OK);

    let sync = statefs_proto::encode_sync_request();
    let rsp = statefs_send_recv(client, &sync)?;
    let status = match statefs_proto::decode_status_response(statefs_proto::OP_SYNC, &rsp) {
        Ok(status) => status,
        Err(_) => {
            emit_bytes(crate::markers::M_SELFTEST_STATEFS_PERSIST_SYNC_RSP_LEN.as_bytes());
            emit_hex_u64(rsp.len() as u64);
            emit_bytes(b" b0=");
            emit_hex_u64(*rsp.get(0).unwrap_or(&0) as u64);
            emit_bytes(b" b3=");
            emit_hex_u64(*rsp.get(3).unwrap_or(&0) as u64);
            emit_line(")");
            return Err(());
        }
    };
    if status != statefs_proto::STATUS_OK {
        emit_bytes(crate::markers::M_SELFTEST_STATEFS_PERSIST_SYNC_STATUS.as_bytes());
        emit_hex_u64(status as u64);
        emit_line(")");
        return Err(());
    }
    emit_line(crate::markers::M_SELFTEST_STATEFS_PERSIST_SYNC_OK);

    let reopen = statefs_proto::encode_reopen_request();
    let rsp = statefs_send_recv(client, &reopen)?;
    let status = match statefs_proto::decode_status_response(statefs_proto::OP_REOPEN, &rsp) {
        Ok(status) => status,
        Err(_) => {
            emit_bytes(crate::markers::M_SELFTEST_STATEFS_PERSIST_REOPEN_RSP_LEN.as_bytes());
            emit_hex_u64(rsp.len() as u64);
            emit_bytes(b" b0=");
            emit_hex_u64(*rsp.get(0).unwrap_or(&0) as u64);
            emit_bytes(b" b3=");
            emit_hex_u64(*rsp.get(3).unwrap_or(&0) as u64);
            emit_line(")");
            return Err(());
        }
    };
    if status != statefs_proto::STATUS_OK {
        emit_bytes(crate::markers::M_SELFTEST_STATEFS_PERSIST_REOPEN_STATUS.as_bytes());
        emit_hex_u64(status as u64);
        emit_line(")");
        return Err(());
    }
    emit_line(crate::markers::M_SELFTEST_STATEFS_PERSIST_REOPEN_OK);

    let get = statefs_proto::encode_key_only_request(statefs_proto::OP_GET, key).map_err(|_| ())?;
    let rsp = statefs_send_recv(client, &get)?;
    let got = statefs_proto::decode_get_response(&rsp).map_err(|_| ())?;
    if got.as_slice() != value {
        emit_line(crate::markers::M_SELFTEST_STATEFS_PERSIST_GET_MISMATCH);
        return Err(());
    }
    Ok(())
}

pub(crate) fn statefs_has_crash_dump(client: &KernelClient) -> core::result::Result<bool, ()> {
    const CHILD_DUMP_PATH: &str = "/state/crash/child.demo.minidump.nmd";
    let get = statefs_proto::encode_key_only_request(statefs_proto::OP_GET, CHILD_DUMP_PATH)
        .map_err(|_| ())?;
    let rsp = statefs_send_recv(client, &get)?;
    Ok(statefs_proto::decode_get_response(&rsp).is_ok())
}

pub(crate) fn grant_statefs_caps_to_child(
    statefs: &KernelClient,
    child_pid: Pid,
) -> core::result::Result<(), ()> {
    const CHILD_STATEFS_SEND_SLOT: u32 = 7;
    const CHILD_STATEFS_RECV_SLOT: u32 = 8;
    let (send_slot, recv_slot) = statefs.slots();
    let send_clone = nexus_abi::cap_clone(send_slot).map_err(|_| ())?;
    nexus_abi::cap_transfer_to_slot(
        child_pid,
        send_clone,
        nexus_abi::Rights::SEND,
        CHILD_STATEFS_SEND_SLOT,
    )
    .map_err(|_| ())?;
    let recv_clone = nexus_abi::cap_clone(recv_slot).map_err(|_| ())?;
    nexus_abi::cap_transfer_to_slot(
        child_pid,
        recv_clone,
        nexus_abi::Rights::RECV,
        CHILD_STATEFS_RECV_SLOT,
    )
    .map_err(|_| ())?;
    Ok(())
}

pub(crate) fn locate_minidump_for_crash(
    client: &KernelClient,
    pid: Pid,
    code: i32,
    name: &str,
) -> core::result::Result<(String, String, Vec<u8>), ()> {
    const CHILD_DUMP_PATH: &str = "/state/crash/child.demo.minidump.nmd";
    let expected_build_id = deterministic_build_id(name);
    let get = statefs_proto::encode_key_only_request(statefs_proto::OP_GET, CHILD_DUMP_PATH)
        .map_err(|_| ())?;
    let rsp = statefs_send_recv(client, &get)?;
    let dump_bytes = statefs_proto::decode_get_response(&rsp).map_err(|_| ())?;
    let decoded = MinidumpFrame::decode(dump_bytes.as_slice()).map_err(|_| ())?;
    decoded.validate().map_err(|_| ())?;
    if (decoded.pid == pid || decoded.pid == 0)
        && decoded.code == code
        && decoded.name.as_str() == name
        && decoded.build_id.as_str() == expected_build_id.as_str()
    {
        return Ok((decoded.build_id, String::from(CHILD_DUMP_PATH), dump_bytes));
    }
    Err(())
}
