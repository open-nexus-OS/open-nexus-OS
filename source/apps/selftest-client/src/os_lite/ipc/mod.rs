// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: IPC selftest seam (os_lite). Hosts the kernel-client cache,
//! routing helpers, and bounded reply-buffer helpers extracted verbatim from
//! the previous monolithic `os_lite` block in `main.rs` (TASK-0023B /
//! RFC-0038 phase 1, cut 3). No behavior, marker, or reject-path change.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal (binary crate)
//! TEST_COVERAGE: QEMU marker ladder via `just test-os` (full ladder).
//! ADR: docs/adr/0017-service-architecture.md, docs/rfcs/RFC-0038-*.md

pub(crate) mod clients;
pub(crate) mod reply;
pub(crate) mod routing;
