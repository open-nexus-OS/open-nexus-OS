// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0080 Phase 1 — execd owns ONE shared glyph-atlas VMO (filled
//! once from the embedded blob) and grants a READ-ONLY clone into every
//! app-host it spawns. The physical pages are shared, so N app windows add ~0
//! atlas bytes instead of the ~4.25 MB `exec` used to copy per launch. Split
//! out of `os_lite.rs` (module-size ratchet). No `os_lite` helper needed — the
//! grant is a self-contained clone → COPY-transfer → close.
//! OWNERS: @runtime / @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU markers (`execd: atlas vmo ready/granted`,
//!   `APPHOST: atlas mapped`) + arena-flat open/close storm

/// The child's fixed slot for the shared atlas VMO (read-only). The app-host
/// maps it and installs it as its text atlas base. MUST stay clear of the
/// `nexus-sdk-routes` `child_slot` range (11..=18) — slot 15 collided with the
/// `settings` route (`nexus.permission.SETTINGS`), so the settings app's
/// settingsd grant failed and no toggled setting applied. Kept at 19 (above the
/// route range); the matching constant is `app-host`'s `ATLAS_VMO_SLOT`.
const CHILD_ATLAS_VMO_SLOT: u32 = 19;

/// Creates the shared glyph-atlas VMO and fills it from the embedded blob (one
/// copy for ALL app-hosts). Returns execd's cap slot, or `None` on any failure
/// (logged; app-hosts then render blank text — fail-visible, never a crash).
pub(crate) fn create() -> Option<u32> {
    let bytes = nexus_text_baked::embedded_atlas();
    let len = bytes.len();
    let vmo = match nexus_abi::vmo_create(len) {
        Ok(v) => v,
        Err(_) => {
            let _ = nexus_abi::debug_println("execd: FAIL atlas vmo (create)");
            return None;
        }
    };
    // Fill in bounded chunks (the whole atlas is ~4.25 MB).
    const CHUNK: usize = 64 * 1024;
    let mut off = 0usize;
    while off < len {
        let end = core::cmp::min(off + CHUNK, len);
        if nexus_abi::vmo_write(vmo, off, &bytes[off..end]).is_err() {
            let _ = nexus_abi::debug_println("execd: FAIL atlas vmo (write)");
            let _ = nexus_abi::cap_close(vmo);
            return None;
        }
        off = end;
    }
    // RFC-0080 hardening: derive a READ-ONLY alias and DROP the writable cap —
    // execd keeps only the RO alias, so not even execd (nor any app-host) can
    // corrupt the shared atlas after it is filled. The physical pages stay
    // alive via the alias (the writable close is a local drop, not a free).
    let ro = match nexus_abi::vmo_share_readonly(vmo) {
        Ok(ro) => ro,
        Err(_) => {
            let _ = nexus_abi::debug_println("execd: FAIL atlas vmo (share-ro)");
            let _ = nexus_abi::cap_close(vmo);
            return None;
        }
    };
    let _ = nexus_abi::cap_close(vmo);
    let _ = nexus_abi::debug_println("execd: atlas vmo ready");
    Some(ro)
}

/// Grants a READ-ONLY clone of the shared atlas VMO into the child's fixed slot
/// (before resume). No-op when the VMO is missing (child renders blank text).
/// `cap_transfer_to_slot` COPIES, so the clone is closed after the transfer.
pub(crate) fn grant(child_pid: u32, atlas_vmo: Option<u32>) {
    let Some(vmo) = atlas_vmo else {
        return;
    };
    let Ok(clone) = nexus_abi::cap_clone(vmo) else {
        let _ = nexus_abi::debug_println("execd: FAIL atlas vmo grant (clone)");
        return;
    };
    // Rights::MAP lets the child `vmo_map_page` it; the child maps it READ-only
    // (the map FLAGS, not the cap, choose RO — a RO-only VMO right is a
    // hardening follow-up, RFC-0080).
    let ok = nexus_abi::cap_transfer_to_slot(
        child_pid as nexus_abi::Pid,
        clone,
        nexus_abi::Rights::MAP,
        CHILD_ATLAS_VMO_SLOT,
    )
    .is_ok();
    let _ = nexus_abi::cap_close(clone);
    let _ = nexus_abi::debug_println(if ok {
        "execd: atlas vmo granted"
    } else {
        "execd: FAIL atlas vmo grant"
    });
}
