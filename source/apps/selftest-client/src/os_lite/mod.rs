// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `os_lite` entry point — twelve-phase dispatch for the OS selftest harness.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os); 119 `SELFTEST:` markers.
//!
//! After TASK-0023B Phase 2 this file is aggregator-only: the `mod`
//! declarations plus a thin `pub fn run()` that forwards to `dispatch::run()`
//! (QoS lane selection, profile gating, verdict aggregation, phase order). Marker emission, retry budgets, and reject paths live in the
//! per-phase modules; capability primitives live under the noun subtrees
//! (`services/`, `ipc/`, `probes/`, `dsoftbus/`, `net/`, `mmio/`, `vfs/`,
//! `timed/`, `updated/`).
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

extern crate alloc;

mod boot_cfg;
mod context;
mod dispatch;
#[path = "display_bootstrap_observer.rs"]
mod display_bootstrap;
mod display_observer;
mod dsoftbus;
mod imed;
mod imed_osk;
mod ipc;
mod mmio;
mod net;
mod observer;
mod phases;
mod probes;
mod profile;
mod services;
mod settings_watch;
mod timed;
mod updated;
mod vfs;

pub fn run() -> core::result::Result<(), ()> {
    dispatch::run()
}
