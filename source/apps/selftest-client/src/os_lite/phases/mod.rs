// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Phase orchestration verbs — twelve `phases::<name>::run(&mut PhaseCtx)`
//!   modules each owning a contiguous slice of the original `os_lite::run()` body.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os).
//!
//! Phase modules MUST NOT import other `phases::*` modules (mechanically
//! enforced from Phase 3 onward by `scripts/check-selftest-arch.sh`). Allowed
//! downstream imports: `services::*`, `ipc::*`, `probes::*`, `dsoftbus::*`,
//! `net::*`, `mmio::*`, `vfs::*`, `timed::*`, `updated::*`, `markers::*`,
//! `crate::os_lite::context::PhaseCtx`.
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

pub(crate) mod bringup;
pub(crate) mod end;
pub(crate) mod exec;
pub(crate) mod ipc_kernel;
pub(crate) mod logd;
pub(crate) mod mmio;
pub(crate) mod net;
pub(crate) mod ota;
pub(crate) mod policy;
pub(crate) mod remote;
pub(crate) mod routing;
pub(crate) mod vfs;
