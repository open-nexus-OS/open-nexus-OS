// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the declarative per-image sched recipe (B4, TASK-0042) — split
//! out of `os_lite.rs` (structure ratchet). Rust-data SSOT, applied
//! cross-task right after spawn via execd's QoS-admin standing.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: boot markers (`execd: sched recipe applied`).

use nexus_abi::sched::Placement;

use crate::os_lite::{IMG_APPHOST, IMG_HELLO};

/// B4 (TASK-0042): declarative per-image sched recipe — the Rust-data SSOT
/// (same doctrine as the init service topology; no TOML at runtime). Applied
/// cross-task right after spawn via execd's QoS-admin standing. The placement is a
/// role (RFC-0118): its CPU set is derived from the CPUs the kernel reports online,
/// never written as a mask; shares clamp to [1,1000].
const SCHED_RECIPES: &[(u8, Placement, usize)] = &[
    // (image_id, placement, shares)
    (IMG_HELLO, Placement::Any, 100),
    (IMG_APPHOST, Placement::Any, 100),
];

pub(crate) fn apply_sched_recipe(pid: u32, image_id: u8) {
    for (img, placement, shares) in SCHED_RECIPES {
        if *img != image_id {
            continue;
        }
        let set = nexus_abi::sched::online().map(|online| placement.set(online));
        let aff_ok = set.is_ok_and(|set| nexus_abi::sched::set_affinity_for(pid, set).is_ok());
        let mask = set.map_or(0, |set| set.bits());
        let shares_ok = nexus_abi::sched::set_shares_for(pid, *shares).is_ok();
        // RFC-0068: one line per image id per boot (repeat spawns are silent);
        // failures always print.
        static RECIPE_LOGGED: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
        let bit = 1u32 << (image_id % 32);
        let seen = RECIPE_LOGGED.fetch_or(bit, core::sync::atomic::Ordering::Relaxed) & bit != 0;
        if aff_ok && shares_ok {
            if !seen {
                let msg = alloc::format!(
                    "execd: sched recipe applied (img={} mask={:#x} shares={})\n",
                    image_id,
                    mask,
                    shares
                );
                let _ = nexus_abi::debug_write(msg.as_bytes());
            }
        } else {
            let _ = nexus_abi::debug_write(b"execd: sched recipe FAIL\n");
        }
        return;
    }
}
