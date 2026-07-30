// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the declarative per-image sched recipe (B4, TASK-0042) — split
//! out of `os_lite.rs` (structure ratchet). Rust-data SSOT, applied
//! cross-task right after spawn via execd's QoS-admin standing.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: boot markers (`execd: sched recipe applied`).

use crate::os_lite::{IMG_APPHOST, IMG_HELLO};

/// B4 (TASK-0042): declarative per-image sched recipe — the Rust-data SSOT
/// (same doctrine as the init service topology; no TOML at runtime). Applied
/// cross-task right after spawn via execd's QoS-admin standing. `mask` is
/// clamped by the kernel to online CPUs; shares clamp to [1,1000].
const SCHED_RECIPES: &[(u8, usize, usize)] = &[
    // (image_id, affinity_mask, shares)
    (IMG_HELLO, 0xF, 100),
    (IMG_APPHOST, 0xF, 100),
];

pub(crate) fn apply_sched_recipe(pid: u32, image_id: u8) {
    for (img, mask, shares) in SCHED_RECIPES {
        if *img != image_id {
            continue;
        }
        let aff_ok = nexus_abi::sched::set_affinity_for(pid, *mask).is_ok();
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
