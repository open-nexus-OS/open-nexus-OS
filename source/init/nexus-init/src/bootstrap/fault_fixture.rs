// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Fault-fixture park for the `ota-fallback` loader-backstop lane
//! (TASK-0289-B). The lane must prove the LOADER's tries-exhaustion flip —
//! the recovery that still works when a staged image is too broken to run
//! its own userspace. A live userspace would shadow that proof: init's
//! boot-attempt tick exhausts the RECORD first and bootctld rolls back in
//! software, so the loader flip never fires. This module makes the trial
//! boots honestly dead: on a TRIAL boot of slot b (per the loader's own
//! measured record) with the harness-armed `ota-fallback` profile, init
//! emits the withheld marker and parks BEFORE any service spawns — no
//! statefsd, no bootctld, no boot-attempt, exactly like an image that
//! bricks in early userspace. The harness plays the power-cycle role.
//! The park is deliberately eternal (wait-loop doctrine exception, marker
//! first): the guest cannot reset itself here — SBI reset is identity-
//! bound to bootctld, which this boot never starts.
//! OWNERS: @runtime @security
//! STATUS: Experimental (TASK-0289-B)
//! TEST_COVERAGE: QEMU `ota-fallback` lane (four boots, one uart)
//! ADR: docs/adr/0059-first-stage-boot-chain-nxboot-handoff.md

use crate::bootstrap::helpers::debug_write_bytes;

/// The lane's runtime profile as nxboot wrote it into `/chosen/nexus,boot-profile`
/// (RFC-0098 C2; parity: selftest-client `boot_cfg.rs`).
const FAULT_PROFILE: &str = "ota-fallback";

/// Parks this boot if it is an armed fault-fixture trial. Returns
/// (normally) in every other case; never returns when parked.
pub(crate) fn park_if_armed() {
    // Trial detection comes from the LOADER's measured record (ADR-0059
    // raw layout: [10] = boot slot 0/1, [11] = tries_decremented) — the
    // one source that cannot confuse a fallback boot of slot a with the
    // trial itself. Absent record (direct-kernel dev boot) ⇒ no trial.
    let Some(record) = nexus_abi::boot_measured_read() else { return };
    if record[10] != 1 || record[11] != 1 {
        return;
    }
    // The knob: the harness-set runtime profile, read from the tree the kernel
    // exposes to init (the same channel the selftest reads).
    if crate::bootstrap::device_tree::chosen_str("boot-profile") != Some(FAULT_PROFILE) {
        return;
    }
    debug_write_bytes(b"init: health withheld (fault fixture)\n");
    loop {
        let _ = nexus_abi::yield_();
    }
}
