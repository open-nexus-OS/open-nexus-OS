// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Declarative CPU-placement SSOT (split from `service_topology.rs`
//! under the structure ratchet) — SMP soft-realtime plan P1: which ROLE a service is placed
//! by. The role says nothing about a machine: the CPU set it resolves to is derived from the
//! topology the kernel reports (`nexus_sched::Placement::set`, RFC-0118) — the display and
//! input chain on the boot hart's CPU, the background on every other online one, whatever
//! their number.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: service_topology placement tests; the roles' sets are host-tested in
//!   `nexus-sched` for one to 64 CPUs.
//! ADR: docs/adr/0049-bkl-lockclass-and-softrt-cpu-placement.md

use nexus_sched::Placement;

// const-fn string match via bytes (const_str_eq is not stable): compare against the canonical
// names.
const fn eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// The soft-realtime chain: the services a frame or a key passes through on its way from the
/// device to the glass. One list, two consumers — their placement (`placement_for`, ADR-0049)
/// and their scheduler class (ADR-0096: init raises them to `Interactive` before their wake,
/// so an interrupt or a message that wakes one of them preempts a `Normal` task at the trap
/// exit; everything else, apps included, runs `Normal`).
pub const fn soft_realtime(name: &str) -> bool {
    eq(name, "gpud")
        || eq(name, "windowd")
        || eq(name, "inputd")
        || eq(name, "hidrawd")
        || eq(name, "xhcid")
        || eq(name, "touchd")
        || eq(name, "imed")
}

/// Declarative CPU placement SSOT (SMP soft-realtime plan P1). The display +
/// input chain runs on the soft-RT CPU (its BKL competitors are only each
/// other), everything background on the other CPUs, so exec/vmo-heavy
/// bring-up work never steals the soft-RT CPU's time from the interactive chain.
/// A role, not a mask: on a machine of one CPU every role is that CPU.
pub const fn placement_for(name: &str) -> Placement {
    if soft_realtime(name) {
        return Placement::Display;
    }
    // init itself + the selftest run anywhere (the proof ladder tests
    // cross-cpu behaviour deliberately).
    if eq(name, "selftest-client") || eq(name, "init-lite") || eq(name, "nexus-init") {
        return Placement::Any;
    }
    Placement::Background
}
