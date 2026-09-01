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

/// fw_cfg file the launcher sets the runtime profile in (parity:
/// selftest-client `boot_cfg.rs`).
const PROFILE_FILE: &[u8] = b"opt/org.open-nexus/selftest-profile";
const FAULT_PROFILE: &[u8] = b"ota-fallback";

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
    // The knob: the harness-set runtime profile. init creates its own
    // fw_cfg window cap (same host-config channel the selftest reads;
    // not a policy-gated device).
    let Ok(cap) = nexus_abi::device_mmio_cap_create(
        nexus_abi::fwcfg::FW_CFG_MMIO_BASE,
        nexus_abi::fwcfg::FW_CFG_MMIO_LEN,
        usize::MAX,
    ) else {
        return;
    };
    let mut buf = [0u8; 16];
    let Some(n) = nexus_abi::fwcfg::read_named_file(cap, PROFILE_FILE, &mut buf) else {
        return;
    };
    if &buf[..n] != FAULT_PROFILE {
        return;
    }
    debug_write_bytes(b"init: health withheld (fault fixture)\n");
    loop {
        let _ = nexus_abi::yield_();
    }
}
