// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Declarative CPU-placement SSOT (split from `service_topology.rs`
//! under the structure ratchet; content unchanged — SMP soft-realtime plan
//! P1: display/input chain on cpu0, background on cpu1-3).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: service_topology affinity tests (unchanged).
//! ADR: docs/adr/0049-bkl-lockclass-and-softrt-cpu-placement.md

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
/// device to the glass. One list, two consumers — their placement (`affinity_for`, ADR-0049)
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
/// input chain is pinned to cpu0 (the soft-RT hart: its BKL competitors are
/// only each other), everything background runs on cpu1-3, so exec/vmo-heavy
/// bring-up work never steals cpu0 time from the interactive chain. Masks are
/// clamped by the kernel to ONLINE cpus, so SMP=1 degrades to cpu0 for all.
pub const fn affinity_for(name: &str) -> u8 {
    // Soft-realtime chain -> cpu0.
    if soft_realtime(name) {
        return 0b0001;
    }
    // init itself + the selftest keep the full mask (the proof ladder tests
    // cross-cpu behaviour deliberately).
    if eq(name, "selftest-client") || eq(name, "init-lite") || eq(name, "nexus-init") {
        return 0b1111;
    }
    // Everything else is background -> cpu1-3 (kernel clamps to online).
    0b1110
}
