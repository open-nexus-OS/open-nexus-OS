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

/// A service's reservation (RFC-0120): `share_ppm` of one CPU every `period_ns`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reservation {
    /// The share of one CPU, in parts per million.
    pub share_ppm: u32,
    /// The period the share is budgeted over, in nanoseconds.
    pub period_ns: u64,
}

/// The soft-realtime chain: the services a frame or a key passes through on its way from the
/// device to the glass, each with the time it holds. One table, three consumers (TASK-0365 P1):
/// - **placement** (`placement_for`, ADR-0049);
/// - **class** (ADR-0096: init raises them to `Interactive` before their wake — a weight in the
///   fair class, ADR-0101; everything else, apps included, runs `Normal`);
/// - **time** (RFC-0120): init derives a right of the service's share from the chain's right,
///   hands it the capability (`TIME_RIGHT_SLOT`) and binds it as a reservation before its wake.
///   A reservation runs before everything fair, so the chain's guarantee does not rest on a
///   class: the renderer a third of a CPU every 10 ms, the compositor a fifth every 8.3 ms,
///   the input router a twentieth every 4 ms. Past its budget a service runs on in the fair
///   class. The device drivers and the input method hold no time: their weight only.
pub const CHAIN: &[(&str, Option<Reservation>)] = &[
    ("gpud", Some(Reservation { share_ppm: 300_000, period_ns: 10_000_000 })),
    ("windowd", Some(Reservation { share_ppm: 200_000, period_ns: 8_333_333 })),
    ("inputd", Some(Reservation { share_ppm: 50_000, period_ns: 4_000_000 })),
    ("hidrawd", None),
    ("xhcid", None),
    ("touchd", None),
    ("imed", None),
];

/// `name`'s row of the chain.
const fn chain_row(name: &str) -> Option<usize> {
    let mut i = 0;
    while i < CHAIN.len() {
        if eq(CHAIN[i].0, name) {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Whether `name` is a service of the soft-realtime chain.
pub const fn soft_realtime(name: &str) -> bool {
    chain_row(name).is_some()
}

/// The reservation init binds to service `name` — and the right it hands it — if it holds one,
/// with its row of the chain (what init keeps its right under).
pub const fn reservation_for(name: &str) -> Option<(usize, Reservation)> {
    match chain_row(name) {
        Some(row) => match CHAIN[row].1 {
            Some(reservation) => Some((row, reservation)),
            None => None,
        },
        None => None,
    }
}

/// What the display chain's right holds: the sum of its services' shares.
pub const fn display_time_ppm() -> u32 {
    let (mut sum, mut i) = (0, 0);
    while i < CHAIN.len() {
        if let Some(reservation) = CHAIN[i].1 {
            sum += reservation.share_ppm;
        }
        i += 1;
    }
    sum
}

/// The service that spawns the apps and hands each its time: it holds the apps' right.
pub const APP_SPAWNER: &str = "execd";

/// The smallest part ever taken from the apps' right or below it (RFC-0120's grain): an app is
/// code the system does not trust, so its right is cut in twentieths of a CPU — the apps'
/// right, however it is divided, fills at most `apps_time_ppm / APPS_GRAIN_PPM` of the kernel's
/// slots, never the table.
pub const APPS_GRAIN_PPM: u32 = 50_000;

/// What the apps' right holds on a machine whose root right is `root_ppm`: a quarter of what
/// the display chain leaves, in whole grains — it scales with the CPUs, never a fixed number.
/// Zero (no right: every app runs in the fair class) when that is less than one grain.
pub const fn apps_time_ppm(root_ppm: u32) -> u32 {
    let quarter = root_ppm.saturating_sub(display_time_ppm()) / 4;
    quarter - quarter % APPS_GRAIN_PPM
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
