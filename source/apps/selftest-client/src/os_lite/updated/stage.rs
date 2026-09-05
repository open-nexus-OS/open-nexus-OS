// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Stage / log-probe helpers for the `updated` submodule.
//! TASK-0179 recut: staging is PATH-BASED (`OP_STAGE_SOURCE`) — the
//! inline 8-KiB stage is gone, so these helpers name a container in
//! `/updates/` on the data volume that `nx image fixtures` shipped at
//! factory time (`/data` is the PARTITION name; the volume mounts at the
//! VFS root):
//!   * `updated_stage`          -- happy path, small VERIFY-only fixture.
//!   * `updated_stage_real`     -- the real os-B container (crown lane).
//!   * `updated_stage_deny`     -- deny lanes, asserting the reject code.
//!   * `updated_log_probe`      -- unsupported-op probe (0x7f).
//!
//! WHY TWO CONTAINERS: the small fixture keeps the headless ladder fast
//! (256 KiB instead of ~19 MB per stage) and is deliberately UNBOOTABLE
//! (invalid NXBD `load_addr`) because it lands in a real slot partition —
//! if a lane ever left it selected, the loader must refuse it loudly
//! rather than jump into pattern bytes. Only the `ota` crown lane stages
//! the real image, and that lane's whole point is booting it.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) — routing + ota phases.
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::vec::Vec;

use nexus_ipc::KernelClient;

use crate::markers::emit_line;

use super::reply_pump::{updated_expect_status, updated_send_with_reply};

/// Small verify-only container (fast, deliberately unbootable).
pub(crate) const FIXTURE_PATH: &str = "/updates/os-fixture-b.nxs";
/// The real os-B image the crown lane flips to.
pub(crate) const REAL_PATH: &str = "/updates/os-B.nxs";
/// TASK-0321 P3: os-B + the system volume + metricsd@1.0.1 as ONE set.
pub(crate) const BUNDLE_SET_PATH: &str = "/updates/bundle-set.nxs";
/// TASK-0035 P3: the same set with the changed bundle as a `bundle-delta`.
pub(crate) const BUNDLE_DELTA_PATH: &str = "/updates/bundle-delta.nxs";
/// Deny-lane containers (RFC-0089 §8 reject vocabulary).
pub(crate) const UNTRUSTED_PATH: &str = "/updates/os-fixture-untrusted.nxs";
pub(crate) const TAMPERED_PATH: &str = "/updates/os-fixture-tampered.nxs";
pub(crate) const DOWNGRADE_PATH: &str = "/updates/os-fixture-downgrade.nxs";
/// TASK-0034 (RFC-0090) delta lane: a real reconstruction from the ACTIVE
/// slot (one COPY window + a literal tail, small and unbootable) and the
/// base-binding deny (a stream made from bytes the device is NOT running).
pub(crate) const DELTA_PATH: &str = "/updates/os-fixture-delta.nxs";
pub(crate) const DELTABASE_PATH: &str = "/updates/os-fixture-deltabase.nxs";

/// Reject codes echoed in the FAILED reply payload (updated's
/// `reject_code`, mirroring `component_set::RejectReason`).
pub(crate) const REJECT_UNTRUSTED_PUBLISHER: u8 = 1;
pub(crate) const REJECT_DIGEST: u8 = 3;
pub(crate) const REJECT_DOWNGRADE: u8 = 7;
pub(crate) const REJECT_DELTA_BASE: u8 = 11;

pub(crate) fn updated_stage(
    client: &KernelClient,
    reply_send_slot: u32,
    reply_recv_slot: u32,
    pending: &mut VecDeque<Vec<u8>>,
) -> core::result::Result<(), ()> {
    let rsp = stage_source(client, reply_send_slot, reply_recv_slot, pending, FIXTURE_PATH)?;
    updated_expect_status(&rsp, nexus_abi::updated::OP_STAGE_SOURCE)?;
    Ok(())
}

/// TASK-0034 (RFC-0090): stage the delta fixture — `updated` reconstructs
/// the target from the ACTIVE slot's bytes through the full engine
/// (base binding, COPY reads, readback digest, NXBD-last).
pub(crate) fn updated_stage_delta(
    client: &KernelClient,
    reply_send_slot: u32,
    reply_recv_slot: u32,
    pending: &mut VecDeque<Vec<u8>>,
) -> core::result::Result<(), ()> {
    let rsp = stage_source(client, reply_send_slot, reply_recv_slot, pending, DELTA_PATH)?;
    updated_expect_status(&rsp, nexus_abi::updated::OP_STAGE_SOURCE)?;
    Ok(())
}

/// Crown lane (TASK-0179): stage the REAL os-B container — the bytes the
/// next boot actually runs.
pub(crate) fn updated_stage_real(
    client: &KernelClient,
    reply_send_slot: u32,
    reply_recv_slot: u32,
    pending: &mut VecDeque<Vec<u8>>,
) -> core::result::Result<(), ()> {
    updated_stage_path(client, reply_send_slot, reply_recv_slot, pending, REAL_PATH)
}

/// Happy-path stage of ANY container by path (the crown lanes pick theirs).
pub(crate) fn updated_stage_path(
    client: &KernelClient,
    reply_send_slot: u32,
    reply_recv_slot: u32,
    pending: &mut VecDeque<Vec<u8>>,
    path: &str,
) -> core::result::Result<(), ()> {
    let rsp = stage_source(client, reply_send_slot, reply_recv_slot, pending, path)?;
    updated_expect_status(&rsp, nexus_abi::updated::OP_STAGE_SOURCE)?;
    Ok(())
}

/// Deny lane: the stage MUST come back FAILED with the expected stable
/// reject code. An OK here means a verification gate is not enforced.
pub(crate) fn updated_stage_deny(
    client: &KernelClient,
    reply_send_slot: u32,
    reply_recv_slot: u32,
    pending: &mut VecDeque<Vec<u8>>,
    path: &str,
    expect_code: u8,
) -> core::result::Result<(), ()> {
    let rsp = stage_source(client, reply_send_slot, reply_recv_slot, pending, path)?;
    // Response framing: [M0, M1, VER, op|0x80, status, len:u16le, code...]
    if rsp.len() < 8 || rsp[4] != nexus_abi::updated::STATUS_FAILED || rsp[7] != expect_code {
        return Err(());
    }
    Ok(())
}

/// TASK-0198 Phase 1 deny lane (kept as a named helper: the untrusted
/// publisher reject is its own proof rung).
pub(crate) fn updated_stage_untrusted_deny(
    client: &KernelClient,
    reply_send_slot: u32,
    reply_recv_slot: u32,
    pending: &mut VecDeque<Vec<u8>>,
) -> core::result::Result<(), ()> {
    updated_stage_deny(
        client,
        reply_send_slot,
        reply_recv_slot,
        pending,
        UNTRUSTED_PATH,
        REJECT_UNTRUSTED_PUBLISHER,
    )
}

fn stage_source(
    client: &KernelClient,
    reply_send_slot: u32,
    reply_recv_slot: u32,
    pending: &mut VecDeque<Vec<u8>>,
    path: &str,
) -> core::result::Result<Vec<u8>, ()> {
    let bytes = path.as_bytes();
    let mut frame = Vec::with_capacity(8 + bytes.len());
    frame.resize(8 + bytes.len(), 0u8);
    let n = nexus_abi::updated::encode_stage_source_req(bytes, &mut frame).ok_or(())?;
    emit_line(crate::markers::M_SELFTEST_UPDATED_STAGE_SEND);
    updated_send_with_reply(
        client,
        reply_send_slot,
        reply_recv_slot,
        nexus_abi::updated::OP_STAGE_SOURCE,
        &frame[..n],
        pending,
    )
}

pub(crate) fn updated_log_probe(
    client: &KernelClient,
    reply_send_slot: u32,
    reply_recv_slot: u32,
    pending: &mut VecDeque<Vec<u8>>,
) -> core::result::Result<(), ()> {
    let mut frame = [0u8; 4];
    frame[0] = nexus_abi::updated::MAGIC0;
    frame[1] = nexus_abi::updated::MAGIC1;
    frame[2] = nexus_abi::updated::VERSION;
    frame[3] = 0x7f;
    let rsp =
        updated_send_with_reply(client, reply_send_slot, reply_recv_slot, 0x7f, &frame, pending)?;
    updated_expect_status(&rsp, 0x7f)?;
    Ok(())
}
