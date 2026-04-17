// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Shared types and constants for the `updated` submodule —
//!   `SYSTEM_TEST_NXS` (test-key-signed system bundle bytes) and the A/B
//!   `SlotId` enum used across stage/switch/status/health helpers.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) — ota phase.
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

// SECURITY: bring-up test system-set signed with a test key (NOT production custody).
pub(crate) const SYSTEM_TEST_NXS: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/system-test.nxs"));

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SlotId {
    A,
    B,
}
