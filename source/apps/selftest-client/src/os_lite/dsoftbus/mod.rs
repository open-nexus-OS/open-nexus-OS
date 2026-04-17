// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: DSoftBus selftest seam (os_lite). Hosts the local OS QUIC session
//! probe and (in later phase-1 cuts) remote pkgfs/statefs probes. Behavior
//! identical to the previous monolithic `os_lite` block; structural extraction
//! only.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal (binary crate)
//! TEST_COVERAGE: QEMU marker ladder via `just test-os` (REQUIRE_DSOFTBUS=1)
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md, docs/rfcs/RFC-0038-*.md

pub(crate) mod quic_os;
pub(crate) mod remote;
