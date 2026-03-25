// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Internal OS-only module boundaries for netstackd refactor slices
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered by netstackd host tests + QEMU proofs
//! ADR: docs/adr/0005-dsoftbus-architecture.md

pub(crate) mod bootstrap;
pub(crate) mod config;
pub(crate) mod entry;
pub(crate) mod entry_pure;
pub(crate) mod facade;
pub(crate) mod ipc;
pub(crate) mod loopback;
pub(crate) mod observability;
