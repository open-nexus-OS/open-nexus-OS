// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: DSoftBus remote-node probe family (os_lite). Hosts the resolve,
//! pkgfs, and statefs request paths that exercise the remote dsoftbusd peer
//! over the established QUIC session. Extracted verbatim from the previous
//! monolithic `os_lite/mod.rs` block (TASK-0023B / RFC-0038 phase 1, cut 7).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal (binary crate)
//! TEST_COVERAGE: QEMU marker ladder via `just test-os` (REQUIRE_DSOFTBUS=1)
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md, docs/rfcs/RFC-0038-*.md

pub(crate) mod pkgfs;
pub(crate) mod resolve;
pub(crate) mod statefs;

pub(super) const REMOTE_DSOFTBUS_WAIT_MS: u64 = 3000;
