// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the app child's fixed capability slots and payload budgets, as the probe reads
//! them — every number re-sourced from `nexus-service-topology::slots::app_child` (the SSOT
//! init and execd pin against). Split out of `probe/mod.rs` by the structure gate (TASK-0333
//! 6e); the names and values are unchanged.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: the topology's slot tests (host); QEMU markers (`APPHOST: …`)

/// Fixed child capability slots — execd transfers these AFTER spawn
/// (`cap_transfer_to_slot`): SEND on windowd's server endpoint into 5,
/// RECV on windowd's shared response endpoint into 6 (the inputd slot
/// convention). The child may run before the transfer lands, so every
/// first use retries bounded (the #123 empty-slot lesson).
pub(super) const WINDOWD_SEND_SLOT: u32 = nexus_service_topology::slots::app_child::WINDOWD.send;
pub(super) const WINDOWD_RECV_SLOT: u32 = nexus_service_topology::slots::app_child::WINDOWD.recv;
/// The app's DEDICATED event channel (ADR-0042): windowd delivers input
/// events AND surface acks here — the shared response endpoint (slot 6)
/// raced with inputd's ack drain, so a tap sent there could be consumed
/// by any receiver. Slot 6 stays as the fallback for older wiring
/// (marked).
pub(super) const EVENTS_RECV_SLOT: u32 = nexus_service_topology::slots::app_child::EVENTS_RECV;
/// The app's timer-notify pair (TASK-0324 P7-d, minted by execd): the clock's one-shot
/// timer fires a frame on RECV, a waitset member next to the event channel.
pub(super) const TIMER: nexus_service_topology::SlotPair =
    nexus_service_topology::slots::app_child::TIMER;

// The embedded fallback payload is DELETED (separation of concerns):
// program bytes belong to bundlemgrd (the registry) ONLY. A missing/broken
// payload VMO is a LOUD failure in the log (FAIL markers in the probe) and shows
// NO window at all — never a silently different program, and no longer the
// teal probe fill either (a fail-open ghost window titled "App").

/// Fixed child slot holding the payload VMO (execd's
/// `CHILD_PAYLOAD_SLOT`); bundlemgrd fills it and writes the 16-byte
/// header LAST (`nexus_abi::payload_vmo::encode_header`).
pub(super) const PAYLOAD_VMO_SLOT: u32 = nexus_service_topology::slots::app_child::PAYLOAD_VMO;
/// SEND-side clone of OUR OWN event channel (execd grants it alongside the
/// RECV side): the app-host attaches it to windowd ITSELF, tagged with a
/// self-minted nonce that SURFACE_CREATE repeats — windowd binds
/// channel↔surface by nonce (deterministic under concurrent connects).
pub(super) const EVENTS_SEND_CLONE_SLOT: u32 =
    nexus_service_topology::slots::app_child::EVENTS_SEND;
/// Header-poll budget: the fetch is kicked BEFORE our ELF even loads, so
/// the header normally beats us; the budget only bounds failure. 3s→8s
/// (RFC-0075 Phase 8d): the CJK-atlas-grown image lengthened early-boot
/// loads enough that the grant lagged 3s on busy boots (same class as
/// the content-rect budget).
pub(super) const PAYLOAD_BUDGET_NS: u64 = 8_000_000_000;
/// Upper payload bound accepted from the header (matches execd's VMO
/// budget; anything larger is a malformed header by contract). Raised with
/// execd's `PAYLOAD_VMO_BYTES` 256K→512K — settings' NXLC container hit 93%
/// of the old bound.
pub(super) const PAYLOAD_MAX_LEN: usize = 512 * 1024;
