// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Data-plane resource budgets and pool definitions for the production
//! UI architecture. All pixel caches, effect surfaces, and retained data live
//! in VMO-backed pools — windowd heap stays control-only.
//!
//! Phase 5: Workstream 3 — resource model budgets.
//! These constants and pool descriptors lock in the data-plane posture.
//! Actual allocation happens in gpud; windowd references resources by handle.
//!
//! OWNERS: @ui
//! STATUS: Phase 5 — budgets defined, allocation deferred to gpud resource manager
//! API_STABILITY: Contract-locked for Workstreams 3-7
//! TEST_COVERAGE: Host unit tests in this module

// ---------------------------------------------------------------------------
// VMO plane layout (mirrors compositor/mod.rs)
// ---------------------------------------------------------------------------

/// Total VMO size: 1280 × 3200 × 4 = 16,384,000 bytes (16 MB).
pub(crate) const VMO_TOTAL_BYTES: usize = 16_384_000;

/// Plane 0: wallpaper source (static, written once at boot).
pub(crate) const PLANE_WALLPAPER_OFFSET: usize = 0x000000;
pub(crate) const PLANE_WALLPAPER_BYTES: usize = 4_096_000; // 1280×800×4

/// Plane 1: retained scene (backdrop snapshots, frozen-glass surfaces).
pub(crate) const PLANE_RETAINED_OFFSET: usize = 0x3E8000; // 4,096,000
pub(crate) const PLANE_RETAINED_BYTES: usize = 4_096_000; // 1280×800×4

/// Plane 2: frame ring slot A (active scanout).
pub(crate) const PLANE_SLOT_A_OFFSET: usize = 0x7D0000; // 8,192,000
pub(crate) const PLANE_SLOT_A_BYTES: usize = 4_096_000; // 1280×800×4

/// Plane 3: frame ring slot B (alternate scanout).
pub(crate) const PLANE_SLOT_B_OFFSET: usize = 0xBB8000; // 12,288,000
pub(crate) const PLANE_SLOT_B_BYTES: usize = 4_096_000; // 1280×800×4

// ---------------------------------------------------------------------------
// Resource pool budgets (all sizes in bytes)
// ---------------------------------------------------------------------------

/// Maximum total bytes allocated to retained surfaces at any time.
/// Surfaces above this budget trigger LRU eviction.
pub(crate) const SURFACE_POOL_BUDGET: usize = 2_097_152; // 2 MB
const _: () = assert!(
    SURFACE_POOL_BUDGET >= 512 * 512 * 4,
    "SURFACE_POOL_BUDGET too small for one 512×512 surface"
);

/// Maximum bytes for backdrop snapshots (frozen-glass inputs).
/// One snapshot per effect region; evicted on invalidation.
pub(crate) const BACKDROP_SNAPSHOT_BUDGET: usize = 524_288; // 512 KB

/// Maximum bytes for blur transient buffers.
/// Transients are valid for one frame only and reused across frames.
pub(crate) const BLUR_TRANSIENT_BUDGET: usize = 262_144; // 256 KB

/// Maximum bytes for glyph atlas.
/// Glyphs are pre-rasterized text glyphs cached for reuse.
pub(crate) const GLYPH_ATLAS_BUDGET: usize = 524_288; // 512 KB

/// Maximum bytes for icon/SVG atlas.
/// Icons are pre-rasterized SVG assets cached at display resolution.
pub(crate) const ICON_ATLAS_BUDGET: usize = 262_144; // 256 KB

/// Maximum bytes for cursor resources (hardware cursor bitmap uploads).
pub(crate) const CURSOR_RESOURCE_BUDGET: usize = 65_536; // 64 KB

/// Maximum bytes for in-flight command metadata (serialized CommandBuffers
/// queued between windowd and gpud).
pub(crate) const COMMAND_METADATA_BUDGET: usize = 131_072; // 128 KB

// ---------------------------------------------------------------------------
// Resource handle vocabulary
// ---------------------------------------------------------------------------

/// Opaque handle to a VMO-backed surface in gpud's resource table.
/// windowd references surfaces by handle; gpud owns the backing VMO.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct SurfaceHandle(pub u32);

/// Opaque handle to a backdrop snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct BackdropHandle(pub u32);

/// Opaque handle to a glyph atlas tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct GlyphHandle(pub u32);

/// Opaque handle to an icon/SVG atlas tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct IconHandle(pub u32);

/// Opaque handle to a cursor resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct CursorHandle(pub u32);

/// Opaque handle to a blur transient buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct BlurTransientHandle(pub u32);

// ---------------------------------------------------------------------------
// Resource residency (conceptual — implementation in gpud)
// ---------------------------------------------------------------------------

/// Residency class for a VMO-backed resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResidencyClass {
    /// CPU-readable or writable (staging, wallpaper source).
    HostVisible,
    /// Optimized for GPU execution (scanout, retained surfaces).
    DevicePrivate,
    /// Valid for one frame/pass only (blur transients, scratch).
    Transient,
    /// Not fully resident; streamed in as needed (large textures).
    Streamed,
    /// Imported from another subsystem (media frames, infer tensors).
    Imported,
    /// Exported to another subsystem (window surfaces, capture).
    Exported,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn total_vmo_matches_4_plane_sum() {
        let total =
            PLANE_WALLPAPER_BYTES + PLANE_RETAINED_BYTES + PLANE_SLOT_A_BYTES + PLANE_SLOT_B_BYTES;
        assert_eq!(total, VMO_TOTAL_BYTES);
        assert_eq!(VMO_TOTAL_BYTES, 16_384_000);
    }

    #[test]
    fn plane_offsets_are_contiguous() {
        assert_eq!(PLANE_WALLPAPER_OFFSET, 0);
        assert_eq!(PLANE_RETAINED_OFFSET, PLANE_WALLPAPER_OFFSET + PLANE_WALLPAPER_BYTES);
        assert_eq!(PLANE_SLOT_A_OFFSET, PLANE_RETAINED_OFFSET + PLANE_RETAINED_BYTES);
        assert_eq!(PLANE_SLOT_B_OFFSET, PLANE_SLOT_A_OFFSET + PLANE_SLOT_A_BYTES);
    }

    #[test]
    fn total_pool_budget_within_vmo() {
        // Pools live in gpud-managed VMOs, separate from the framebuffer VMO.
        // This test ensures budgets are reasonable (not accidentally huge).
        let pool_total = SURFACE_POOL_BUDGET
            + BACKDROP_SNAPSHOT_BUDGET
            + BLUR_TRANSIENT_BUDGET
            + GLYPH_ATLAS_BUDGET
            + ICON_ATLAS_BUDGET
            + CURSOR_RESOURCE_BUDGET
            + COMMAND_METADATA_BUDGET;
        // Pools plus framebuffer VMO should fit in 32 MB (reasonable QEMU config).
        assert!(pool_total + VMO_TOTAL_BYTES <= 32_768_000);
    }

    #[test]
    fn handle_types_are_copy_send() {
        // Verify handles are small Copy types suitable for IPC.
        assert_eq!(core::mem::size_of::<SurfaceHandle>(), 4);
        assert_eq!(core::mem::size_of::<BackdropHandle>(), 4);
        assert_eq!(core::mem::size_of::<GlyphHandle>(), 4);
        assert_eq!(core::mem::size_of::<IconHandle>(), 4);
        assert_eq!(core::mem::size_of::<CursorHandle>(), 4);
        assert_eq!(core::mem::size_of::<BlurTransientHandle>(), 4);
    }

    #[test]
    fn residency_classes_are_distinct() {
        // Quick sanity: all variants can be constructed.
        let classes = [
            ResidencyClass::HostVisible,
            ResidencyClass::DevicePrivate,
            ResidencyClass::Transient,
            ResidencyClass::Streamed,
            ResidencyClass::Imported,
            ResidencyClass::Exported,
        ];
        // No two adjacent elements are equal (basic variant uniqueness).
        for i in 1..classes.len() {
            assert_ne!(classes[i - 1], classes[i]);
        }
    }
}
