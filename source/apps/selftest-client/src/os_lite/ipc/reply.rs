// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Bounded reply-buffer helper used by selftest probes that need to
//! receive a single large reply within a deadline. Extracted verbatim from
//! the previous monolithic `os_lite` block in `main.rs` (TASK-0023B /
//! RFC-0038 phase 1, cut 3). No behavior, marker, or reject-path change.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal (binary crate)
//! TEST_COVERAGE: Indirect via QEMU `just test-os` (logd query/paged probes).
//! ADR: docs/adr/0017-service-architecture.md, docs/rfcs/RFC-0038-*.md

pub(crate) fn recv_large_bounded(
    recv_slot: u32,
    out: &mut [u8],
    budget: core::time::Duration,
) -> core::result::Result<usize, ()> {
    let clock = nexus_ipc::budget::OsClock;
    let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
    let deadline_ns = nexus_ipc::budget::deadline_after(&clock, budget).map_err(|_| ())?;
    let n = nexus_ipc::budget::raw::recv_budgeted(&clock, recv_slot, &mut hdr, out, deadline_ns)
        .map_err(|_| ())?;
    Ok(core::cmp::min(n, out.len()))
}
