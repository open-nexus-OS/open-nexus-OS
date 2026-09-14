// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Metrics/logd helper functions for dsoftbusd OS path.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered transitively by dsoftbusd QEMU proofs.
//!
//! ADR: docs/adr/0005-dsoftbus-architecture.md

pub(crate) fn metrics_counter_inc_best_effort(name: &str) {
    static LOGGED_COUNTER_NEW_FAIL: core::sync::atomic::AtomicBool =
        core::sync::atomic::AtomicBool::new(false);

    let Ok(metrics) = nexus_metrics::client::MetricsClient::new() else {
        if !LOGGED_COUNTER_NEW_FAIL.swap(true, core::sync::atomic::Ordering::Relaxed) {
            // #region agent log
            let _ = nexus_abi::debug_println("dbg:h6: metrics counter client new fail");
            // #endregion
        }
        return;
    };
    let _ = metrics.counter_inc(name, b"svc=dsoftbusd\n", 1);
}

pub(crate) fn metrics_hist_observe_best_effort(name: &str, value: u64) {
    static LOGGED_HIST_NEW_FAIL: core::sync::atomic::AtomicBool =
        core::sync::atomic::AtomicBool::new(false);

    let Ok(metrics) = nexus_metrics::client::MetricsClient::new() else {
        if !LOGGED_HIST_NEW_FAIL.swap(true, core::sync::atomic::Ordering::Relaxed) {
            // #region agent log
            let _ = nexus_abi::debug_println("dbg:h6: metrics hist client new fail");
            // #endregion
        }
        return;
    };
    let _ = metrics.hist_observe(name, b"svc=dsoftbusd\n", value);
}

pub(crate) fn append_probe_to_logd(scope: &[u8], msg: &[u8]) -> bool {
    const MAGIC0: u8 = b'L';
    const MAGIC1: u8 = b'O';
    const VERSION: u8 = 2;
    const OP_APPEND: u8 = 1;
    const LEVEL_INFO: u8 = 2;
    const STATUS_OK: u8 = 0;
    static NONCE: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(1);

    if scope.is_empty() || scope.len() > 64 || msg.is_empty() || msg.len() > 256 {
        return false;
    }
    // The declared legs (TASK-0324 P7-d): logd's request endpoint and our reply inbox; the
    // append is ONE waited exchange — logd's ack (nonce-matched) or logd's death ends it.
    let logd = nexus_service_topology::slots::dsoftbusd::LOGD;
    let reply = nexus_service_topology::slots::dsoftbusd::REPLY;
    let nonce = NONCE.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    let mut frame = alloc::vec::Vec::with_capacity(12 + 1 + 1 + 2 + 2 + scope.len() + msg.len());
    frame.extend_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_APPEND]);
    frame.extend_from_slice(&nonce.to_le_bytes());
    frame.push(LEVEL_INFO);
    frame.push(scope.len() as u8);
    frame.extend_from_slice(&(msg.len() as u16).to_le_bytes());
    frame.extend_from_slice(&0u16.to_le_bytes()); // fields_len
    frame.extend_from_slice(scope);
    frame.extend_from_slice(msg);
    let mut buf = [0u8; 64];
    let accept = |rsp: &[u8]| -> Option<bool> {
        if rsp.len() < 13 || rsp[0] != MAGIC0 || rsp[1] != MAGIC1 || rsp[2] != VERSION {
            return None;
        }
        if rsp[3] != (OP_APPEND | 0x80) {
            return None;
        }
        let (status, got_nonce) =
            nexus_ipc::logd_wire::parse_append_response_v2_prefix(rsp).ok()?;
        (got_nonce == nonce).then_some(status == STATUS_OK)
    };
    nexus_ipc::exchange::call_matching(
        logd.send,
        nexus_service_topology::SlotPair::new(reply.send, reply.recv),
        &frame,
        &mut buf,
        accept,
    )
    .unwrap_or(false)
}
