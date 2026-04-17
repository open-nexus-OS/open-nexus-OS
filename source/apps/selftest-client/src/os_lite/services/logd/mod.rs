// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: logd IPC client + selftest probes — append/query roundtrip,
//!   evidence-anchor and minidump-evidence anchors, oversize/forbidden
//!   rejects, journaling readiness gate.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) — logd phase.
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

extern crate alloc;

use alloc::vec::Vec;

use nexus_abi::yield_;
use nexus_ipc::KernelClient;

use super::super::ipc::reply::recv_large_bounded;
use crate::markers::emit_line;

pub(crate) fn logd_append_status_v2(
    logd: &KernelClient,
    scope: &[u8],
    message: &[u8],
    fields: &[u8],
) -> core::result::Result<u8, ()> {
    const MAGIC0: u8 = b'L';
    const MAGIC1: u8 = b'O';
    const VERSION: u8 = 2;
    const OP_APPEND: u8 = 1;
    const LEVEL_INFO: u8 = 2;
    static NONCE: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(1);

    if scope.len() > 255 || message.len() > u16::MAX as usize || fields.len() > u16::MAX as usize {
        return Err(());
    }

    let nonce = NONCE.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    let mut frame = Vec::with_capacity(18 + scope.len() + message.len() + fields.len());
    frame.extend_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_APPEND]);
    frame.extend_from_slice(&nonce.to_le_bytes());
    frame.push(LEVEL_INFO);
    frame.push(scope.len() as u8);
    frame.extend_from_slice(&(message.len() as u16).to_le_bytes());
    frame.extend_from_slice(&(fields.len() as u16).to_le_bytes());
    frame.extend_from_slice(scope);
    frame.extend_from_slice(message);
    frame.extend_from_slice(fields);

    let clock = nexus_ipc::budget::OsClock;
    // Use CAP_MOVE replies so we don't depend on the dedicated response endpoint.
    const REPLY_RECV_SLOT: u32 = 0x17;
    const REPLY_SEND_SLOT: u32 = 0x18;
    let (send_slot, _recv_slot) = logd.slots();
    let reply_send_clone = nexus_abi::cap_clone(REPLY_SEND_SLOT).map_err(|_| {
        emit_line("SELFTEST: logd append reply clone fail");
        ()
    })?;
    let hdr = nexus_abi::MsgHeader::new(
        reply_send_clone,
        0,
        0,
        nexus_abi::ipc_hdr::CAP_MOVE,
        frame.len() as u32,
    );
    let deadline_ns = nexus_ipc::budget::deadline_after(&clock, core::time::Duration::from_secs(2))
        .map_err(|_| ())?;
    nexus_ipc::budget::raw::send_budgeted(&clock, send_slot, &hdr, &frame, deadline_ns).map_err(
        |_| {
            emit_line("SELFTEST: logd append send fail");
            ()
        },
    )?;
    let mut rsp_buf = [0u8; 64];
    // Shared reply inbox: ignore unrelated CAP_MOVE replies.
    let mut rsp_len: Option<usize> = None;
    for _ in 0..64 {
        let n = match recv_large_bounded(
            REPLY_RECV_SLOT,
            &mut rsp_buf,
            core::time::Duration::from_millis(50),
        ) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let rsp = &rsp_buf[..n];
        if rsp.len() >= 13
            && rsp[0] == MAGIC0
            && rsp[1] == MAGIC1
            && rsp[2] == VERSION
            && rsp[3] == (OP_APPEND | 0x80)
        {
            let (_status, got_nonce) =
                nexus_ipc::logd_wire::parse_append_response_v2_prefix(rsp).map_err(|_| ())?;
            if got_nonce == nonce {
                rsp_len = Some(n);
                break;
            }
        }
    }
    let Some(n) = rsp_len else {
        emit_line("SELFTEST: logd append recv fail");
        return Err(());
    };
    let rsp = &rsp_buf[..n];
    if rsp.len() < 29 || rsp[0] != MAGIC0 || rsp[1] != MAGIC1 || rsp[2] != VERSION {
        emit_line("SELFTEST: logd append rsp malformed");
        return Err(());
    }
    if rsp[3] != (OP_APPEND | 0x80) {
        emit_line("SELFTEST: logd append rsp bad-op");
        return Err(());
    }
    let (status, got_nonce) =
        nexus_ipc::logd_wire::parse_append_response_v2_prefix(rsp).map_err(|_| ())?;
    if got_nonce != nonce {
        emit_line("SELFTEST: logd append rsp bad-nonce");
        return Err(());
    }
    Ok(status)
}

pub(crate) fn logd_append_probe(logd: &KernelClient) -> core::result::Result<(), ()> {
    const STATUS_OK: u8 = 0;
    let status = logd_append_status_v2(logd, b"selftest", b"logd hello", b"")?;
    if status != STATUS_OK {
        emit_line("SELFTEST: logd append rsp bad-status");
        return Err(());
    }
    Ok(())
}

pub(crate) fn logd_hardening_reject_probe(logd: &KernelClient) -> core::result::Result<(), ()> {
    const STATUS_INVALID_ARGS: u8 = 4;
    const STATUS_OVER_LIMIT: u8 = 5;
    const STATUS_RATE_LIMITED: u8 = 6;

    // Invalid args: payload identity spoof attempt must be rejected.
    let invalid_status = logd_append_status_v2(
        logd,
        b"selftest",
        b"logd spoof attempt",
        b"sender_service_id=9999\nk=v\n",
    )?;
    if invalid_status != STATUS_INVALID_ARGS {
        return Err(());
    }

    // Over limit: oversized fields beyond v1/v2 logd bound must be rejected.
    let oversized_fields = [b'x'; 513];
    let over_limit_status =
        logd_append_status_v2(logd, b"selftest", b"logd over-limit attempt", &oversized_fields)?;
    if over_limit_status != STATUS_OVER_LIMIT {
        return Err(());
    }

    // Rate-limited: deterministic burst from same sender within one window.
    let mut rate_limited_seen = false;
    for _ in 0..48 {
        let st = logd_append_status_v2(logd, b"selftest", b"logd rate burst", b"")?;
        if st == STATUS_RATE_LIMITED {
            rate_limited_seen = true;
            break;
        }
    }
    if !rate_limited_seen {
        return Err(());
    }
    Ok(())
}

pub(crate) fn logd_query_probe(logd: &KernelClient) -> core::result::Result<bool, ()> {
    // Use the paged query helper to avoid truncation false negatives when the log grows.
    logd_query_contains_since_paged(logd, 0, b"logd hello")
}

pub(crate) fn logd_stats_total(logd: &KernelClient) -> core::result::Result<u64, ()> {
    const REPLY_RECV_SLOT: u32 = 0x17;
    const REPLY_SEND_SLOT: u32 = 0x18;
    static NONCE: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(1000);
    let nonce = NONCE.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    let mut frame = [0u8; 12];
    frame[0] = nexus_ipc::logd_wire::MAGIC0;
    frame[1] = nexus_ipc::logd_wire::MAGIC1;
    frame[2] = nexus_ipc::logd_wire::VERSION_V2;
    frame[3] = nexus_ipc::logd_wire::OP_STATS;
    frame[4..12].copy_from_slice(&nonce.to_le_bytes());
    let clock = nexus_ipc::budget::OsClock;
    let (send_slot, _recv_slot) = logd.slots();
    let reply_send_clone = nexus_abi::cap_clone(REPLY_SEND_SLOT).map_err(|_| ())?;
    let hdr = nexus_abi::MsgHeader::new(
        reply_send_clone,
        0,
        0,
        nexus_abi::ipc_hdr::CAP_MOVE,
        frame.len() as u32,
    );
    let deadline_ns = nexus_ipc::budget::deadline_after(&clock, core::time::Duration::from_secs(2))
        .map_err(|_| ())?;
    nexus_ipc::budget::raw::send_budgeted(&clock, send_slot, &hdr, &frame, deadline_ns)
        .map_err(|_| ())?;
    let _ = nexus_abi::cap_close(reply_send_clone);

    let mut rsp_buf = [0u8; 256];
    for _ in 0..128 {
        let n = match recv_large_bounded(
            REPLY_RECV_SLOT,
            &mut rsp_buf,
            core::time::Duration::from_millis(50),
        ) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let rsp = &rsp_buf[..n];
        if rsp.len() >= 29
            && rsp[0] == nexus_ipc::logd_wire::MAGIC0
            && rsp[1] == nexus_ipc::logd_wire::MAGIC1
            && rsp[2] == nexus_ipc::logd_wire::VERSION_V2
            && rsp[3] == (nexus_ipc::logd_wire::OP_STATS | 0x80)
            && nexus_ipc::logd_wire::extract_nonce_v2(rsp) == Some(nonce)
        {
            let (got_nonce, p) =
                nexus_ipc::logd_wire::parse_stats_response_prefix_v2(rsp).map_err(|_| ())?;
            if got_nonce != nonce {
                return Err(());
            }
            if p.status != nexus_ipc::logd_wire::STATUS_OK {
                return Err(());
            }
            return Ok(p.total_records);
        }
    }
    Err(())
}

pub(crate) fn logd_query_count(logd: &KernelClient) -> core::result::Result<u64, ()> {
    static NONCE: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(2000);
    let nonce = NONCE.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    let mut frame = [0u8; 12];
    frame[0] = nexus_ipc::logd_wire::MAGIC0;
    frame[1] = nexus_ipc::logd_wire::MAGIC1;
    frame[2] = nexus_ipc::logd_wire::VERSION_V2;
    frame[3] = nexus_ipc::logd_wire::OP_STATS;
    frame[4..12].copy_from_slice(&nonce.to_le_bytes());
    let clock = nexus_ipc::budget::OsClock;
    nexus_ipc::budget::send_budgeted(&clock, logd, &frame, core::time::Duration::from_secs(2))
        .map_err(|_| ())?;
    let rsp = nexus_ipc::budget::recv_budgeted(&clock, logd, core::time::Duration::from_secs(2))
        .map_err(|_| ())?;
    let (got_nonce, p) =
        nexus_ipc::logd_wire::parse_stats_response_prefix_v2(&rsp).map_err(|_| ())?;
    if got_nonce != nonce {
        return Err(());
    }
    if p.status != nexus_ipc::logd_wire::STATUS_OK {
        return Err(());
    }
    Ok(p.total_records)
}

pub(crate) fn logd_query_contains_since_paged(
    logd: &KernelClient,
    mut since_nsec: u64,
    needle: &[u8],
) -> core::result::Result<bool, ()> {
    let clock = nexus_ipc::budget::OsClock;
    const REPLY_RECV_SLOT: u32 = 0x17;
    const REPLY_SEND_SLOT: u32 = 0x18;
    let (send_slot, _recv_slot) = logd.slots();
    let mut emitted = false;
    let mut empty_pages = 0usize;
    static NONCE: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(10_000);
    for _ in 0..64 {
        let nonce = NONCE.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        // Allocation-free QUERY frame v2 (22 bytes).
        let mut frame = [0u8; 22];
        frame[0] = nexus_ipc::logd_wire::MAGIC0;
        frame[1] = nexus_ipc::logd_wire::MAGIC1;
        frame[2] = nexus_ipc::logd_wire::VERSION_V2;
        frame[3] = nexus_ipc::logd_wire::OP_QUERY;
        frame[4..12].copy_from_slice(&nonce.to_le_bytes());
        frame[12..20].copy_from_slice(&since_nsec.to_le_bytes());
        frame[20..22].copy_from_slice(&8u16.to_le_bytes()); // max_count (page cap)

        // Send with CAP_MOVE so replies arrive on the reply inbox.
        let reply_send_clone = nexus_abi::cap_clone(REPLY_SEND_SLOT).map_err(|_| {
            if !emitted {
                emit_line("SELFTEST: logd query reply clone fail");
                emitted = true;
            }
            ()
        })?;
        let hdr = nexus_abi::MsgHeader::new(
            reply_send_clone,
            0,
            0,
            nexus_abi::ipc_hdr::CAP_MOVE,
            frame.len() as u32,
        );
        let deadline_ns =
            nexus_ipc::budget::deadline_after(&clock, core::time::Duration::from_secs(2))
                .map_err(|_| ())?;
        nexus_ipc::budget::raw::send_budgeted(&clock, send_slot, &hdr, &frame, deadline_ns)
            .map_err(|_| {
                if !emitted {
                    emit_line("SELFTEST: logd query send fail");
                    emitted = true;
                }
                ()
            })?;

        // Allocation-free receive into a stack buffer (bump allocator friendly).
        let mut rsp_buf = [0u8; 1024];
        // Shared reply inbox: ignore unrelated CAP_MOVE replies.
        let mut rsp_len: Option<usize> = None;
        for _ in 0..128 {
            let n = match recv_large_bounded(
                REPLY_RECV_SLOT,
                &mut rsp_buf,
                core::time::Duration::from_millis(50),
            ) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let rsp = &rsp_buf[..n];
            if rsp.len() >= 13
                && rsp[0] == nexus_ipc::logd_wire::MAGIC0
                && rsp[1] == nexus_ipc::logd_wire::MAGIC1
                && rsp[2] == nexus_ipc::logd_wire::VERSION_V2
                && rsp[3] == (nexus_ipc::logd_wire::OP_QUERY | 0x80)
            {
                if nexus_ipc::logd_wire::extract_nonce_v2(rsp) == Some(nonce) {
                    rsp_len = Some(n);
                    break;
                }
            }
        }
        let Some(n) = rsp_len else {
            if !emitted {
                emit_line("SELFTEST: logd query recv fail");
            }
            return Err(());
        };
        let rsp = &rsp_buf[..n];
        let scan = nexus_ipc::logd_wire::scan_query_page_v2(rsp, nonce, needle).map_err(|_| {
            if !emitted {
                emit_line("SELFTEST: logd query rsp parse fail");
                emitted = true;
            }
            ()
        })?;
        if scan.count == 0 {
            // Empty pages can happen transiently while CAP_MOVE log writes are still in flight.
            empty_pages = empty_pages.saturating_add(1);
            if empty_pages >= 8 {
                return Ok(false);
            }
            let _ = yield_();
            continue;
        }
        empty_pages = 0;
        if scan.found {
            return Ok(true);
        }
        let Some(next_since) =
            nexus_ipc::logd_wire::next_since_nsec(since_nsec, scan.max_timestamp_nsec)
        else {
            return Ok(false);
        };
        since_nsec = next_since;
    }
    Ok(false)
}
