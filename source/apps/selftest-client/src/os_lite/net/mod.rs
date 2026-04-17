// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Networking selftest seam (os_lite). Hosts shared netstack/UDP
//! probe helpers extracted verbatim from the previous monolithic `os_lite`
//! block in `main.rs` (TASK-0023B / RFC-0038 phase 1, cut 2). No behavior,
//! marker, or reject-path change.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal (binary crate)
//! TEST_COVERAGE: QEMU marker ladder via `just test-os`.
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md, docs/rfcs/RFC-0038-*.md

pub(crate) mod icmp_ping;
pub(crate) mod local_addr;
#[cfg(feature = "smoltcp-probe")]
pub(crate) mod smoltcp_probe;
