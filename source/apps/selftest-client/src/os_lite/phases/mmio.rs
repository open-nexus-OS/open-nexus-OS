// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Phase 8 of 12 — mmio (TASK-0010 userspace MMIO capability mapping:
//!   mmio_map_probe / cap_query_mmio_probe / cap_query_vmo_probe).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) — MMIO / cap-query slice.
//!
//! Extracted in Cut P2-04 of TASK-0023B. Marker order and marker strings are
//! byte-identical to the pre-cut body. No service routing performed here;
//! mmio probes only invoke kernel cap APIs.
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

use crate::markers::emit_line;
use crate::os_lite::context::PhaseCtx;
use crate::os_lite::mmio;

pub(crate) fn run(_ctx: &mut PhaseCtx) -> core::result::Result<(), ()> {
    // TASK-0010: userspace MMIO capability mapping proof (virtio-mmio magic register).
    if mmio::mmio_map_probe().is_ok() {
        emit_line(crate::markers::M_SELFTEST_MMIO_MAP_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_MMIO_MAP_FAIL);
    }
    // Pre-req for virtio DMA: userland can query (base,len) for address-bearing caps.
    if mmio::cap_query_mmio_probe().is_ok() {
        emit_line(crate::markers::M_SELFTEST_CAP_QUERY_MMIO_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_CAP_QUERY_MMIO_FAIL);
    }
    if mmio::cap_query_vmo_probe().is_ok() {
        emit_line(crate::markers::M_SELFTEST_CAP_QUERY_VMO_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_CAP_QUERY_VMO_FAIL);
    }
    Ok(())
}
