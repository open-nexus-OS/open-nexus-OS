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

mod boot_cfg;
mod context;
#[path = "display_bootstrap_observer.rs"]
mod display_bootstrap;
mod display_observer;
mod dsoftbus;
mod ipc;
mod mmio;
mod net;
mod observer;
mod phases;
mod probes;
mod profile;
mod services;
mod timed;
mod updated;
mod vfs;

pub fn run() -> core::result::Result<(), ()> {
    use profile::{PhaseId, Profile};

    // QoS by boot lane — this is the SSOT for selftest-client's scheduling and it
    // must respect BOTH lanes (RFC-0023: a task may set its own QoS):
    //
    //  * INTERACTIVE boot (`just start`): the human wants a FAST first frame +
    //    the animated boot logo. DEFER the whole selftest suite to Idle so the
    //    display/input critical path (Normal) is never preempted — the ladder
    //    runs after the frame settles (and if it never fully drains here, that's
    //    fine: interactive boots don't gate on the proof ladder). Raising to
    //    Interactive here (the old unconditional behaviour) strictly preempted
    //    gpud/windowd → slow boot + a frozen splash. This is the regression fix.
    //
    //  * PROOF boot (`just test-os`): the selftest ladder IS the deliverable —
    //    run it at Interactive so it isn't starved by Normal services (incl.
    //    under SMP=2), and the full marker stream flushes deterministically.
    //
    // The kernel spawns selftest-client at Normal (see `initial_qos_for`) so it
    // is schedulable enough to REACH this point; from here it owns its QoS.
    let interactive = profile::runtime_is_interactive();
    let _ = nexus_abi::task_qos_set_self(if interactive {
        nexus_abi::QosClass::Idle
    } else {
        nexus_abi::QosClass::Interactive
    });

    let mut ctx = context::PhaseCtx::bootstrap()?;

    // P4-08: runtime profile dispatch. `Full` (the default) is byte-identical
    // to the pre-P4-08 ladder; non-Full profiles emit a single
    // `dbg: phase X skipped` breadcrumb in place of the phase body.
    let active = Profile::from_kernel_cmdline_or_default(Profile::Full);

    // Verdict aggregation: in an interactive boot the per-marker ladder is folded into one
    // `selftest:<phase> N/N OK <ms>` line per group (+ a final `SELFTEST` total) — slow groups
    // flagged, failures expanded. The proof harness keeps the full marker stream (verdict mode
    // off) so `verify-uart` stays deterministic against the proof-manifest SSOT.
    crate::markers::set_console_verdict_mode(interactive);
    let boot_span = nexus_abi::Span::begin();

    macro_rules! run_or_skip {
        ($phase:ident, $id:ident, $group:literal) => {
            if active.includes(PhaseId::$id) {
                let (t0, f0) = crate::markers::marker_counts();
                let span = nexus_abi::Span::begin();
                let result = phases::$phase::run(&mut ctx);
                if interactive {
                    let (t1, f1) = crate::markers::marker_counts();
                    let emitted = t1 - t0;
                    let fails = (f1 - f0) + if result.is_err() { 1 } else { 0 };
                    let (total, passed) = if emitted == 0 && fails > 0 {
                        (1, 0)
                    } else if fails >= emitted {
                        (emitted, 0)
                    } else {
                        (emitted, emitted - fails)
                    };
                    crate::markers::emit_verdict($group, passed, total, span.elapsed_ms());
                }
                result?;
            } else {
                crate::markers::emit_line(Profile::skip_marker(PhaseId::$id));
            }
        };
    }

    // Phase order intentionally matches the original ladder (NOT the
    // numeric `[phase.X].order` field) so that under `Profile::Full` the
    // emitted UART transcript is byte-identical to the pre-P4-08 baseline.
    run_or_skip!(bringup, Bringup, "selftest:bringup");
    run_or_skip!(routing, Routing, "selftest:routing");
    run_or_skip!(ota, Ota, "selftest:ota");
    run_or_skip!(policy, Policy, "selftest:policy");
    run_or_skip!(exec, Exec, "selftest:exec");
    run_or_skip!(logd, Logd, "selftest:logd");
    run_or_skip!(ipc_kernel, IpcKernel, "selftest:ipc");
    run_or_skip!(mmio, Mmio, "selftest:mmio");
    run_or_skip!(vfs, Vfs, "selftest:vfs");
    run_or_skip!(net, Net, "selftest:net");
    run_or_skip!(remote, Remote, "selftest:remote");
    let end = if active.includes(PhaseId::End) {
        phases::end::run(&mut ctx)
    } else {
        crate::markers::emit_line(Profile::skip_marker(PhaseId::End));
        Ok(())
    };
    // Final aggregated verdict over the whole run.
    if interactive {
        let (total, fails) = crate::markers::marker_counts();
        let passed = total.saturating_sub(fails);
        crate::markers::emit_verdict("SELFTEST", passed, total, boot_span.elapsed_ms());
    }
    end
}

// NOTE: Keep this file's marker surface centralized in `crate::markers`.
