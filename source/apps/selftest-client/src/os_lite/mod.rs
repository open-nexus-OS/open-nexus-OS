// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `os_lite` entry point — twelve-phase dispatch for the OS selftest harness.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os); 119 `SELFTEST:` markers.
//!
//! After TASK-0023B Phase 2 this file is aggregator-only: 12 `mod` declarations
//! plus a 14-line `pub fn run()` that bootstraps a minimal `PhaseCtx` and
//! dispatches to `phases::<name>::run(&mut ctx)?` in the order locked by the
//! marker ladder. Marker emission, retry budgets, and reject paths live in the
//! per-phase modules; capability primitives live under the noun subtrees
//! (`services/`, `ipc/`, `probes/`, `dsoftbus/`, `net/`, `mmio/`, `vfs/`,
//! `timed/`, `updated/`).
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

extern crate alloc;

mod context;
mod dsoftbus;
mod ipc;
mod mmio;
mod net;
mod phases;
mod probes;
mod services;
mod timed;
mod updated;
mod vfs;

pub fn run() -> core::result::Result<(), ()> {
    let mut ctx = context::PhaseCtx::bootstrap()?;
    phases::bringup::run(&mut ctx)?;
    phases::routing::run(&mut ctx)?;
    phases::ota::run(&mut ctx)?;
    phases::policy::run(&mut ctx)?;
    phases::exec::run(&mut ctx)?;
    phases::logd::run(&mut ctx)?;
    phases::ipc_kernel::run(&mut ctx)?;
    phases::mmio::run(&mut ctx)?;
    phases::vfs::run(&mut ctx)?;
    phases::net::run(&mut ctx)?;
    phases::remote::run(&mut ctx)?;
    phases::end::run(&mut ctx)
}

// NOTE: Keep this file's marker surface centralized in `crate::markers`.
