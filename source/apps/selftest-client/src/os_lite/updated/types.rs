// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Shared types for the `updated` submodule — the A/B `SlotId`
//! enum used across stage/switch/status/health helpers. (TASK-0179: the
//! embedded `.nxs` blobs are gone; containers live on the data partition
//! and staging names a path — see `stage.rs`.)
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) — ota phase.
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SlotId {
    A,
    B,
}
