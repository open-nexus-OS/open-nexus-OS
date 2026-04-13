//! CONTEXT: Metrics/logd helper functions for dsoftbusd OS path.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered transitively by dsoftbusd QEMU proofs.
//!
//! ADR: docs/adr/0005-dsoftbus-architecture.md

use super::service_clients::{
    cached_client_slots, invalidate_cached_slots, LOGD_RECV_SLOT_CACHE, LOGD_SEND_SLOT_CACHE,
};

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
    use nexus_ipc::Wait;

    const MAGIC0: u8 = b'L';
    const MAGIC1: u8 = b'O';
    const VERSION: u8 = 2;
    const OP_APPEND: u8 = 1;
    const LEVEL_INFO: u8 = 2;
    static NONCE: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(1);

    if scope.is_empty() || scope.len() > 64 || msg.is_empty() || msg.len() > 256 {
        // #region agent log
        let _ = nexus_abi::debug_println("dbg:h2: logd_probe input reject");
        // #endregion
        return false;
    }

    let mut logd_opt =
        cached_client_slots("logd", &LOGD_SEND_SLOT_CACHE, &LOGD_RECV_SLOT_CACHE, false);
    if logd_opt.is_none() {
        logd_opt = cached_client_slots("logd", &LOGD_SEND_SLOT_CACHE, &LOGD_RECV_SLOT_CACHE, true);
    }
    let Some(logd) = logd_opt else {
        invalidate_cached_slots(&LOGD_SEND_SLOT_CACHE, &LOGD_RECV_SLOT_CACHE);
        // #region agent log
        let _ = nexus_abi::debug_println("dbg:h2: logd_probe client missing");
        // #endregion
        return false;
    };
    // Use deterministic init-lite distributed reply inbox slots for dsoftbusd (recv=0x5 send=0x6).
    // Avoid relying on routing v1 here (uncorrelated replies under bring-up).
    let reply_send_slot: u32 = 0x6;
    let reply_recv_slot: u32 = 0x5;
    let moved = match nexus_abi::cap_clone(reply_send_slot) {
        Ok(slot) => slot,
        Err(_) => {
            // #region agent log
            let _ = nexus_abi::debug_println("dbg:h2: logd_probe cap clone fail");
            // #endregion
            return false;
        }
    };
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

    // Use CAP_MOVE so the logd response does not pollute selftest-client's logd recv queue.
    if logd
        .send_with_cap_move_wait(&frame, moved, Wait::NonBlocking)
        .is_err()
    {
        let _ = nexus_abi::cap_close(moved);
        invalidate_cached_slots(&LOGD_SEND_SLOT_CACHE, &LOGD_RECV_SLOT_CACHE);
        // #region agent log
        let _ = nexus_abi::debug_println("dbg:h2: logd_probe send fail");
        // #endregion
        return false;
    }

    // Deterministic: wait (bounded) for the APPEND ack so the shared inbox cannot fill.
    const STATUS_OK: u8 = 0;
    let start = nexus_abi::nsec().ok().unwrap_or(0);
    let deadline = start.saturating_add(250_000_000); // 250ms
    let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
    let mut buf = [0u8; 64];
    let mut spins: usize = 0;
    loop {
        if (spins & 0x7f) == 0 {
            let now = nexus_abi::nsec().ok().unwrap_or(0);
            if now >= deadline {
                // #region agent log
                let _ = nexus_abi::debug_println("dbg:h2: logd_probe ack timeout");
                // #endregion
                return false;
            }
        }
        match nexus_abi::ipc_recv_v1(
            reply_recv_slot,
            &mut hdr,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => {
                let n = core::cmp::min(n as usize, buf.len());
                if n >= 13
                    && buf[0] == MAGIC0
                    && buf[1] == MAGIC1
                    && buf[2] == VERSION
                    && buf[3] == (OP_APPEND | 0x80)
                {
                    if let Ok((status, got_nonce)) =
                        nexus_ipc::logd_wire::parse_append_response_v2_prefix(&buf[..n])
                    {
                        if got_nonce == nonce {
                            // #region agent log
                            let _ = nexus_abi::debug_println("dbg:h2: logd_probe ack matched");
                            // #endregion
                            return status == STATUS_OK;
                        }
                    }
                }
                let _ = nexus_abi::yield_();
            }
            Err(nexus_abi::IpcError::QueueEmpty) => {
                let _ = nexus_abi::yield_();
            }
            Err(_) => return false,
        }
        spins = spins.wrapping_add(1);
    }
}
