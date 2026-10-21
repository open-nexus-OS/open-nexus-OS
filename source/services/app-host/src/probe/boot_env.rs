// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: app-host probe — the process-environment helpers of the boot path: the
//! shared-atlas map (RFC-0080), the nonce address salt and the monotonic clock. Split out
//! of `probe/mod.rs` (structure ratchet, TASK-0066); pure path plumbing, no behavior change.
//! OWNERS: @ui @runtime
//! STATUS: Experimental

use super::raw_marker;

/// A per-process address salt for the nonce (ASLR-independent uniqueness
/// helper; the time component does the heavy lifting).
pub(super) fn payload_addr() -> usize {
    (&super::PAYLOAD_BUDGET_NS) as *const u64 as usize
}

/// RFC-0080: slot execd grants the shared atlas VMO into (=execd `CHILD_ATLAS_VMO_SLOT`; clear of sdk-routes child_slots 11..=18).
const ATLAS_VMO_SLOT: u32 = nexus_service_topology::slots::app_child::ATLAS_VMO;

/// Maps the shared atlas VMO READ-only and installs it as the text atlas
/// base, so this app-host renders from ONE shared copy instead of its own
/// embedded 4.25 MB (the blob is not in this image). Best-effort: any
/// failure falls back to blank text (never a crash) with a loud marker.
#[allow(unsafe_code)]
pub(super) fn map_atlas_base() {
    use nexus_abi::page_flags;
    let len = nexus_text_baked::atlas_len();
    // RFC-0085: one whole-range map at a KERNEL-CHOSEN va (was a ~1100-call
    // per-page loop at fixed 0x3000_0000 — the slot-15 scar class). The RO
    // alias force-maps read-only kernel-side regardless of flags.
    let mapped_len = len.div_ceil(4096) * 4096;
    let flags = page_flags::VALID | page_flags::USER | page_flags::READ;
    let atlas_va = match nexus_abi::vm_map(ATLAS_VMO_SLOT, 0, mapped_len, flags) {
        Ok(va) => va,
        Err(_) => {
            raw_marker("APPHOST: FAIL atlas map");
            return;
        }
    };
    // SAFETY: the range [atlas_va, atlas_va + len) is now mapped read-only
    // from the shared VMO and stays valid for the process lifetime; `len`
    // is the baked atlas size.
    unsafe {
        nexus_text_baked::set_atlas_base(atlas_va as *const u8, len);
    }
    raw_marker("APPHOST: atlas mapped");
}

/// The launch clock starts: the process's first instruction, named on one clock with
/// windowd's `launch request` and `surface presented` (TASK-0333 6a's stages).
pub(super) fn mark_start() -> u64 {
    let start_ns = nsec_now();
    raw_marker(&alloc::format!("apphost: start t_us={}", start_ns / 1000));
    start_ns
}

/// The launch's stages (TASK-0336 P0), each the time since the start in ms: the program
/// mounted (the DSL loaded, its initial effects run), the first frame rendered (laid out,
/// text shaped, rastered), the surface created at windowd. Named by the `ux launch` line
/// so a slow launch says which stage took it.
static STAGES: [core::sync::atomic::AtomicU64; 4] = [
    core::sync::atomic::AtomicU64::new(0),
    core::sync::atomic::AtomicU64::new(0),
    core::sync::atomic::AtomicU64::new(0),
    core::sync::atomic::AtomicU64::new(0),
];

/// The surface's VMO, its creation a launch stage (`3`: the band settled, the region applied,
/// the bytes committed — `vmo_ms`, so `render_ms − vmo_ms` is the first frame's paint alone).
pub(super) fn surface_vmo(bytes: usize, start_ns: u64) -> nexus_abi::Result<nexus_abi::Handle> {
    let vmo = nexus_abi::vmo_create(bytes)?;
    mark_stage(3, start_ns);
    Ok(vmo)
}

/// A launch stage reached: `0` mounted, `1` rendered, `2` created, `3` the surface VMO.
pub(super) fn mark_stage(stage: usize, start_ns: u64) {
    if let Some(s) = STAGES.get(stage) {
        let ms = nsec_now().saturating_sub(start_ns) / 1_000_000;
        s.store(ms, core::sync::atomic::Ordering::Relaxed);
    }
}

/// The first frame is acked: the launch as the user feels it, judged against the UX
/// contract (RFC-0108: warm 300 ms, red at 1 s) — `apphost: ux launch <verdict> (…)`, with
/// its stages.
pub(super) fn mark_first_frame(start_ns: u64) {
    raw_marker("APPHOST: probe surface presented");
    let first_frame_ms = nsec_now().saturating_sub(start_ns) / 1_000_000;
    let stage = |i: usize| STAGES[i].load(core::sync::atomic::Ordering::Relaxed);
    raw_marker(&alloc::format!(
        "apphost: ux launch {} (first_frame_ms={first_frame_ms} mount_ms={} vmo_ms={} render_ms={} create_ms={} budget_ms={})",
        nexus_ux_budgets::judge_launch(first_frame_ms).as_str(),
        stage(0),
        stage(3),
        stage(1),
        stage(2),
        nexus_ux_budgets::LAUNCH_WARN_MS
    ));
}

/// Monotonic now (ns) for physics dt; 0 on ABI failure (tick clamps dt).
pub(super) fn nsec_now() -> u64 {
    #[cfg(nexus_env = "os")]
    {
        nexus_abi::nsec().unwrap_or(0)
    }
    #[cfg(not(nexus_env = "os"))]
    {
        0
    }
}
