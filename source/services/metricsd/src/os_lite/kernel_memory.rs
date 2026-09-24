// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the kernel's memory record (`nexus_abi::mm_stats`, syscall 61 —
//! RFC-0098 C4, TASK-0286 P5) as metricsd gauges. Split out of `os_lite.rs`
//! (structure gate).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU marker `metricsd: mm snapshot ok (…)` (required where metricsd runs)

use crate::Registry;
use alloc::format;

/// TASK-0286 P5 (RFC-0098 C4): the kernel's memory record (`mm_stats`) as live gauges in
/// the registry under metricsd's own identity, read once at readiness. Not into retention:
/// a write there at readiness makes metricsd the WAL's first writer and moves its one-shot
/// `retention wal verified` evidence to the start of the boot, where logd's bounded ring
/// evicts it before the harness looks (measured 2026-09-24: `ci-os-smp1` red, standalone
/// smp1 green). Retained memory samples come with the pressure events of M4 (TASK-0287),
/// never from a poll.
pub(super) fn record(registry: &mut Registry) {
    let Ok(stats) = nexus_abi::mm_stats() else {
        let _ = nexus_abi::debug_println("metricsd: mm snapshot FAIL");
        return;
    };
    let own = nexus_abi::service_id_from_name(b"metricsd");
    let frame = nexus_abi::FRAME_BYTES;
    let gauges: [(&[u8], u64); 5] = [
        (b"mm.bytes.total", stats.total * frame),
        (b"mm.bytes.free", stats.free * frame),
        (b"mm.vmo.bytes", stats.vmo_bytes),
        (b"mm.dma.bytes", stats.dma_bytes),
        (b"mm.exhausted", stats.exhausted),
    ];
    let mut recorded = 0u32;
    for (name, value) in gauges {
        let value = i64::try_from(value).unwrap_or(i64::MAX);
        if registry.gauge_set(own, name, b"", value).is_ok() {
            recorded += 1;
        }
    }
    let _ = nexus_abi::debug_println(&format!(
        "metricsd: mm snapshot ok (total={} free={} vmo_bytes={} dma_bytes={} own_rss={} gauges={})",
        stats.total * frame,
        stats.free * frame,
        stats.vmo_bytes,
        stats.dma_bytes,
        stats.own_rss_bytes,
        recorded
    ));
}
