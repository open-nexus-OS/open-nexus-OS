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
mod display_bootstrap;
mod dsoftbus;
mod ipc;
mod mmio;
mod net;
mod phases;
mod probes;
mod profile;
mod services;
mod timed;
mod updated;
mod vfs;

pub fn run() -> core::result::Result<(), ()> {
    use profile::{PhaseId, Profile};
    let mut ctx = context::PhaseCtx::bootstrap()?;

    // P4-08: runtime profile dispatch. `Full` (the default) is byte-identical
    // to the pre-P4-08 ladder; non-Full profiles emit a single
    // `dbg: phase X skipped` breadcrumb in place of the phase body.
    let active = Profile::from_kernel_cmdline_or_default(Profile::Full);

    macro_rules! run_or_skip {
        ($phase:ident, $id:ident) => {
            if active.includes(PhaseId::$id) {
                phases::$phase::run(&mut ctx)?;
            } else {
                crate::markers::emit_line(Profile::skip_marker(PhaseId::$id));
            }
        };
    }

    // Phase order intentionally matches the original ladder (NOT the
    // numeric `[phase.X].order` field) so that under `Profile::Full` the
    // emitted UART transcript is byte-identical to the pre-P4-08 baseline.
    run_or_skip!(bringup, Bringup);
    run_or_skip!(routing, Routing);
    run_or_skip!(ota, Ota);
    run_or_skip!(policy, Policy);
    run_or_skip!(exec, Exec);
    run_or_skip!(logd, Logd);
    run_or_skip!(ipc_kernel, IpcKernel);
    run_or_skip!(mmio, Mmio);
    run_or_skip!(vfs, Vfs);
    run_or_skip!(net, Net);
    run_or_skip!(remote, Remote);
    if active.includes(PhaseId::End) {
        phases::end::run(&mut ctx)
    } else {
        crate::markers::emit_line(Profile::skip_marker(PhaseId::End));
        Ok(())
    }
}

// NOTE: Keep this file's marker surface centralized in `crate::markers`.
