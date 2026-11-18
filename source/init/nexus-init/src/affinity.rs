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

/// A service's reservation (RFC-0120): `share_ppm` of one CPU every `period_ns`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reservation {
    /// The share of one CPU, in parts per million.
    pub share_ppm: u32,
    /// The period the share is budgeted over, in nanoseconds.
    pub period_ns: u64,
}

/// The display chain's time (TASK-0363 P3; the start of TASK-0365 P1): what init binds, from
/// the right it derives for the chain, to a service before its wake. A reservation runs before
/// everything fair, so the chain's guarantee no longer rests on a class: the renderer a third
/// of a CPU every 10 ms, the compositor a fifth every frame at 120 Hz, the input router a
/// twentieth every 4 ms. Past its budget a service runs on in the fair class at its class's
/// weight. The rest of the chain (the device drivers, the input method) keeps the weight only,
/// until init hands rights down (TASK-0365 P1).
pub const DISPLAY_TIME: &[(&str, Reservation)] = &[
    ("gpud", Reservation { share_ppm: 300_000, period_ns: 10_000_000 }),
    ("windowd", Reservation { share_ppm: 200_000, period_ns: 8_333_333 }),
    ("inputd", Reservation { share_ppm: 50_000, period_ns: 4_000_000 }),
];

/// The reservation init binds to service `name`, if it holds one.
pub fn reservation_for(name: &str) -> Option<Reservation> {
    DISPLAY_TIME.iter().find(|(svc, _)| *svc == name).map(|&(_, reservation)| reservation)
}

/// What the display chain's right holds: the sum of its services' shares.
pub const fn display_time_ppm() -> u32 {
    let (mut sum, mut i) = (0, 0);
    while i < DISPLAY_TIME.len() {
        sum += DISPLAY_TIME[i].1.share_ppm;
        i += 1;
    }
    sum
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
