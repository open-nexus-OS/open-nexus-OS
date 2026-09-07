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
    statefs_send_recv_deadline(client, frame, 2_000_000_000)
}

/// Same exchange with a caller-chosen budget — the fsck ops re-replay the
/// journal over virtio (512B/QD1) and legitimately exceed the default 2s.
pub(crate) fn statefs_send_recv_deadline(
    client: &KernelClient,
    frame: &[u8],
    budget_ns: u64,
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
    let deadline = start.saturating_add(budget_ns);
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

/// One `put` and its wire status (RFC-0091 seam proofs need the status,
/// not a pass/fail).
pub(crate) fn statefs_put_status(
    client: &KernelClient,
    key: &str,
    value: &[u8],
) -> core::result::Result<u8, ()> {
    let put = statefs_proto::encode_put_request(key, value).map_err(|_| ())?;
    let rsp = statefs_send_recv(client, &put)?;
    statefs_proto::decode_status_response(statefs_proto::OP_PUT, &rsp).map_err(|_| ())
}

/// RFC-0072 quota proof against the shipped `[quota."selftest-client"]`
/// (prefix `/state/app/selftest/quota/`, soft 256, hard 512): two puts fit
/// (the second crosses soft), the third is refused `STATUS_QUOTA_EXCEEDED`
/// before it reaches the journal, a delete frees room and the same put then
/// succeeds. Returns the refusing status when the sequence does not hold.
pub(crate) fn statefs_quota_probe(client: &KernelClient) -> core::result::Result<(), u8> {
    const A: &str = "/state/app/selftest/quota/a";
    const B: &str = "/state/app/selftest/quota/b";
    const C: &str = "/state/app/selftest/quota/c";
    let value = [b'q'; 180]; // 180 + 27-byte key = 207 per entry
    let put = |key: &str| statefs_put_status(client, key, &value).map_err(|_| 0xffu8);
    // Fresh tree (a keep-blk boot may carry the previous run's entries).
    for key in [A, B, C] {
        let del = statefs_proto::encode_key_only_request(statefs_proto::OP_DEL, key)
            .map_err(|_| 0xfeu8)?;
        let _ = statefs_send_recv(client, &del);
    }
    if put(A)? != statefs_proto::STATUS_OK {
        return Err(1);
    }
    if put(B)? != statefs_proto::STATUS_OK {
        return Err(2);
    }
    // 414 used + 207 = 621 > 512 ⇒ EDQUOTA.
    match put(C)? {
        statefs_proto::STATUS_QUOTA_EXCEEDED => {}
        other => return Err(other),
    }
    let del =
        statefs_proto::encode_key_only_request(statefs_proto::OP_DEL, A).map_err(|_| 0xfdu8)?;
    let rsp = statefs_send_recv(client, &del).map_err(|_| 0xfcu8)?;
    if statefs_proto::decode_status_response(statefs_proto::OP_DEL, &rsp)
        != Ok(statefs_proto::STATUS_OK)
    {
        return Err(3);
    }
    if put(C)? != statefs_proto::STATUS_OK {
        return Err(4);
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

// TASK-0051B: the at-rest truth is the canonical `.nxcd` container — this
// is what "dump present" means since the NMD1 seam closed.
pub(crate) fn statefs_has_crash_dump(client: &KernelClient) -> core::result::Result<bool, ()> {
    const CHILD_ARTIFACT_PATH: &str = "/state/crash/child.demo.minidump.nxcd";
    let get = statefs_proto::encode_key_only_request(statefs_proto::OP_GET, CHILD_ARTIFACT_PATH)
        .map_err(|_| ())?;
    let rsp = statefs_send_recv(client, &get)?;
    Ok(statefs_proto::decode_get_response(&rsp).is_ok())
}

/// TASK-0051B artifact proof: the `.nxcd` container exists WITH its magic,
/// and the `.nmd` intermediate is gone (conversion deleted it).
pub(crate) fn statefs_crash_artifact_ok(client: &KernelClient) -> core::result::Result<bool, ()> {
    const CHILD_ARTIFACT_PATH: &str = "/state/crash/child.demo.minidump.nxcd";
    const CHILD_DUMP_PATH: &str = "/state/crash/child.demo.minidump.nmd";
    let get = statefs_proto::encode_key_only_request(statefs_proto::OP_GET, CHILD_ARTIFACT_PATH)
        .map_err(|_| ())?;
    let rsp = statefs_send_recv(client, &get)?;
    let Ok(bytes) = statefs_proto::decode_get_response(&rsp) else {
        return Ok(false);
    };
    // `.nxcd` container magic (userspace/crash/nxcd container.rs contract).
    let magic_ok = bytes.len() >= 4 && &bytes[..4] == b"NXCD";
    let get_nmd = statefs_proto::encode_key_only_request(statefs_proto::OP_GET, CHILD_DUMP_PATH)
        .map_err(|_| ())?;
    let rsp = statefs_send_recv(client, &get_nmd)?;
    let nmd_gone =
        matches!(statefs_proto::decode_get_response(&rsp), Err(statefs::StatefsError::NotFound));
    Ok(magic_ok && nmd_gone)
}

/// TASK-0051B redaction proof over the at-rest container's section table
/// (`.nxcd` layout: count u16le at [6], 12-byte entries from [16], kind =
/// entry[0]): only header/frames/maps (0/1/2) + the redaction-gated
/// previews stack/code (6/7) may exist, and the stack preview must be
/// present exactly when the source frame carried one (level stack-only is
/// the granted default; `crash.attach.full` stays denied).
pub(crate) fn statefs_crash_redaction_ok(
    client: &KernelClient,
    expect_stack: bool,
) -> core::result::Result<bool, ()> {
    const CHILD_ARTIFACT_PATH: &str = "/state/crash/child.demo.minidump.nxcd";
    let get = statefs_proto::encode_key_only_request(statefs_proto::OP_GET, CHILD_ARTIFACT_PATH)
        .map_err(|_| ())?;
    let rsp = statefs_send_recv(client, &get)?;
    let Ok(bytes) = statefs_proto::decode_get_response(&rsp) else {
        return Ok(false);
    };
    if bytes.len() < 16 || &bytes[..4] != b"NXCD" {
        return Ok(false);
    }
    let count = u16::from_le_bytes([bytes[6], bytes[7]]) as usize;
    let mut has_stack = false;
    for i in 0..count {
        let Some(&kind) = bytes.get(16 + i * 12) else { return Ok(false) };
        match kind {
            0 | 1 | 2 | 7 => {}
            6 => has_stack = true,
            // Anything else at rest (logs/spans/regs/unknown) violates the
            // redaction contract for this producer.
            _ => return Ok(false),
        }
    }
    Ok(has_stack == expect_stack)
}

// TASK-0049 reanimation (2026-08-19): the former `grant_statefs_caps_to_child`
// helper is retired. It transferred the statefs route into the demo.minidump
// child AFTER spawn — reliable only while the #102 bug kept children suspended
// forever. With the resume fix the transfer raced the child's exit (a transfer
// into a reaped task collapses to EPERM, see kernel errno.rs Transfer arm).
// The route is now granted by EXECD before resume (grants-before-resume):
// `grant_minidump_statefs_route` in source/services/execd/src/os_lite.rs.

// Wired again since the TASK-0049 reanimation (2026-08-19).
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
