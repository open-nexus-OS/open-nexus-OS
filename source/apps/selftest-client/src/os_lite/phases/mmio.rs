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
    // RFC-0068 mmio migration: the TASK-0010 userspace MMIO-MAP probe (mapped the virtio-net MMIO
    // window via the blocking `mmio_map` syscall) is retired — networking moved off virtio-mmio
    // (netstackd / GPU-PCIe), so that map now BLOCKS and hung the whole selftest after ipc_kernel.
    // Live MMIO mapping is already proven by the real drivers (`rngd: mmio window mapped ok`,
    // `virtio-blk` queue setup). The non-blocking cap-query probes below stay (the `cap_query`
    // syscall itself is still exercised). See task #103. The cap_query on the NET MMIO cap (slot 48)
    // is also retired — it hits the same dead virtio-net cap and blocks. The VMO cap_query below is
    // independent (allocates its own VMO) and stays.
    if mmio::cap_query_vmo_probe().is_ok() {
        emit_line(crate::markers::M_SELFTEST_CAP_QUERY_VMO_OK);
    } else {
        emit_line(crate::markers::M_SELFTEST_CAP_QUERY_VMO_FAIL);
    }
    Ok(())
}
