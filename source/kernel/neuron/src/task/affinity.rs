// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Pure affinity-mask helpers (ABI-boundary validation + home-CPU
//! clamping) — split out of `task/mod.rs` (module-size ratchet); bodies are
//! verbatim.
//! OWNERS: @kernel-sched-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: kernel selftests (sched affinity reject/clamp markers)
//! ADR: docs/adr/0052-per-hart-earliest-deadline-timer-and-affinity-respecting-steal.md

use super::ALL_CPUS_MASK;

/// Validates an affinity mask at the ABI boundary: non-empty, within the
/// CPU ceiling, and intersecting the online set (a mask of only-offline CPUs
/// would strand the task forever).
pub fn validate_affinity_mask(mask: usize, online_mask: usize) -> Result<u8, ()> {
    if mask == 0 || mask > ALL_CPUS_MASK as usize {
        return Err(());
    }
    if mask & online_mask == 0 {
        return Err(());
    }
    Ok(mask as u8)
}

/// Clamps a home CPU into an affinity mask: keeps `home` when allowed,
/// otherwise picks the first ONLINE CPU in the mask (boot as last resort).
pub fn clamp_home_to_affinity(
    mask: u8,
    home: crate::types::CpuId,
    online_mask: usize,
) -> crate::types::CpuId {
    if mask & (1u8 << home.as_index()) != 0 && online_mask & (1usize << home.as_index()) != 0 {
        return home;
    }
    for idx in 0..crate::smp::MAX_CPUS {
        if mask & (1u8 << idx) != 0 && online_mask & (1usize << idx) != 0 {
            return crate::types::CpuId::from_raw(idx as u16);
        }
    }
    crate::types::CpuId::BOOT
}
