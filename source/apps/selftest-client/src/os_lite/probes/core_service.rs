// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Generic "is this core service alive and answering?" probes used
//! by the logd phase to verify per-service log emission. Pre-P2-17 these
//! lived in `services/mod.rs` (which forced that file to host both the
//! per-service submodule wiring and probe bodies); P2-17 relocates them
//! here so `services/mod.rs` is a pure aggregator (no fn bodies).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal (binary crate)
//! TEST_COVERAGE: QEMU `just test-os` -- "core services log ok" marker.
//! ADR: docs/adr/0017-service-architecture.md, docs/rfcs/RFC-0038-*.md

use nexus_ipc::KernelClient;

/// Send the unsupported-op probe (0x7f) to a generic core service that
/// uses the standard 4-byte request / 5-byte response shape (e.g. samgrd,
/// bundlemgrd) and verify the reply byte-for-byte.
///
/// Behavior is byte-for-byte identical to the pre-P2-17 implementation;
/// only the file location changed.
pub(crate) fn core_service_probe(
    svc: &KernelClient,
    magic0: u8,
    magic1: u8,
    version: u8,
    op: u8,
) -> core::result::Result<(), ()> {
    let frame = [magic0, magic1, version, op];
    let clock = nexus_ipc::budget::OsClock;
    nexus_ipc::budget::send_budgeted(&clock, svc, &frame, core::time::Duration::from_millis(200))
        .map_err(|_| ())?;
    let rsp = nexus_ipc::budget::recv_budgeted(&clock, svc, core::time::Duration::from_millis(200))
        .map_err(|_| ())?;
    if rsp.len() < 5 || rsp[0] != magic0 || rsp[1] != magic1 || rsp[2] != version {
        return Err(());
    }
    if rsp[3] != (op | 0x80) || rsp[4] != 0 {
        return Err(());
    }
    Ok(())
}

/// policyd-specific variant (frames are 6 bytes per v1 response shape).
///
/// Behavior is byte-for-byte identical to the pre-P2-17 implementation;
/// only the file location changed.
pub(crate) fn core_service_probe_policyd(svc: &KernelClient) -> core::result::Result<(), ()> {
    // policyd expects frames to be at least 6 bytes (v1 response shape).
    let frame = [b'P', b'O', 1, 0x7f, 0, 0];
    let clock = nexus_ipc::budget::OsClock;
    nexus_ipc::budget::send_budgeted(&clock, svc, &frame, core::time::Duration::from_millis(200))
        .map_err(|_| ())?;
    let rsp = nexus_ipc::budget::recv_budgeted(&clock, svc, core::time::Duration::from_millis(200))
        .map_err(|_| ())?;
    if rsp.len() < 6 || rsp[0] != b'P' || rsp[1] != b'O' || rsp[2] != 1 {
        return Err(());
    }
    if rsp[3] != (0x7f | 0x80) || rsp[4] != 0 {
        return Err(());
    }
    Ok(())
}
