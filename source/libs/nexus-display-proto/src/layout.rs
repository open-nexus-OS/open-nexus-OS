// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The shared framebuffer resource's LAYOUT — one maximum and everything derived from
//! it (RFC-0074 / ADR-0050 "one maximum", RFC-0067 retained-plane contract; M-L of the
//! hardware fast track, 2026-09-30). windowd sizes the VMO it hands gpud to this and addresses
//! its planes and atlas by these rows; gpud aliases the same rows into its textures and scans
//! the display plane out of them. Before this module the rows and byte offsets lived as
//! literals in six files of two crates with "MUST match" comments between them, and they
//! drifted once (an atlas band sampled outside the GL alias — garbage rows after maximizing).
//! Now a row is a formula of `LAYOUT_MAX` here and nowhere else.
//!
//! The maximum is the SoC's HDMI maximum, 1920x1080 (measured 2026-09-29,
//! docs/board/measurements/2026-09-29-display-regs); QEMU lanes keep requesting 1280x800 as
//! their VISIBLE mode, which `resolve_display_mode` clamps against this maximum — the
//! resource budget is the maximum, the mode is what the display accepts.
//! OWNERS: @ui @runtime
//! STATUS: Production
//! API_STABILITY: Stable (the rows are a windowd↔gpud contract)
//! TEST_COVERAGE: the invariants below (host); every visible QEMU lane and the board lane

/// The fixed shared-VMO layout maximum — the RESOURCE BUDGET every display consumer sizes
/// against, not a "default mode". The visible mode is resolved separately
/// (`crate::resolve_display_mode`) and never exceeds it.
pub const LAYOUT_MAX: (u32, u32) = (1920, 1080);

/// Bytes per pixel of every plane and atlas surface (BGRA/XRGB 8888).
pub const BYTES_PER_PIXEL: u32 = 4;

/// Bytes per resource row: the framebuffer stride every plane and atlas surface shares.
pub const STRIDE_BYTES: u32 = LAYOUT_MAX.0 * BYTES_PER_PIXEL;

/// Rows of one display-sized plane.
pub const PLANE_ROWS: u32 = LAYOUT_MAX.1;

/// Bytes of one display-sized plane.
pub const PLANE_BYTES: usize = PLANE_ROWS as usize * STRIDE_BYTES as usize;

/// Plane 0: the wallpaper source (written once).
pub const WALLPAPER_ROW: u32 = 0;
/// Plane 1: the retained scene the CPU compositor renders (cursor-free); gpud blits damage
/// from here to the display plane and overlays the cursor.
pub const RETAINED_ROW: u32 = PLANE_ROWS;
/// Plane 2: the display plane gpud scans out (frame ring slot A).
pub const DISPLAY_ROW: u32 = 2 * PLANE_ROWS;
/// Plane 3: frame ring slot B, used as the blur cache.
pub const SLOT_B_ROW: u32 = 3 * PLANE_ROWS;
/// Rows of the four display planes together — where the surface atlas begins.
pub const ATLAS_ROW: u32 = 4 * PLANE_ROWS;
/// Rows of the surface atlas: a desktop band, one resident scroll band, floating windows
/// with their blur bands, the dock and fullscreen round-trips — eight display heights
/// (measured: five starved on the fullscreen re-create with four windows open).
pub const ATLAS_ROWS: u32 = 8 * PLANE_ROWS;
/// Total rows of the shared resource: the four planes plus the atlas.
pub const RESOURCE_HEIGHT: u32 = ATLAS_ROW + ATLAS_ROWS;
/// Total bytes of the shared resource windowd allocates and gpud attaches.
pub const RESOURCE_BYTES: usize = RESOURCE_HEIGHT as usize * STRIDE_BYTES as usize;

/// Byte offset of a plane or atlas row within the resource.
pub const fn row_offset_bytes(row: u32) -> usize {
    row as usize * STRIDE_BYTES as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The maximum is the SoC's HDMI maximum: 16:9, the mode the board's EDID and the stock
    /// system agree on (VIC 16).
    #[test]
    fn the_maximum_is_full_hd() {
        assert_eq!(LAYOUT_MAX, (1920, 1080));
        assert_eq!(LAYOUT_MAX.0 * 9, LAYOUT_MAX.1 * 16);
        assert_eq!(STRIDE_BYTES, 7680);
    }

    /// Planes are contiguous, in order, each one display high; the atlas starts right after
    /// the fourth plane and ends the resource — no hole and no overlap anywhere.
    #[test]
    fn test_reject_overlapping_or_gapped_planes() {
        assert_eq!(WALLPAPER_ROW, 0);
        assert_eq!(RETAINED_ROW, WALLPAPER_ROW + PLANE_ROWS);
        assert_eq!(DISPLAY_ROW, RETAINED_ROW + PLANE_ROWS);
        assert_eq!(SLOT_B_ROW, DISPLAY_ROW + PLANE_ROWS);
        assert_eq!(ATLAS_ROW, SLOT_B_ROW + PLANE_ROWS);
        assert_eq!(RESOURCE_HEIGHT, ATLAS_ROW + ATLAS_ROWS);
        assert!(ATLAS_ROWS >= 5 * PLANE_ROWS, "the atlas starved below five display heights");
    }

    /// Byte offsets are rows times the stride — the two forms can never disagree.
    #[test]
    fn byte_offsets_are_rows_times_stride() {
        assert_eq!(row_offset_bytes(RETAINED_ROW), PLANE_BYTES);
        assert_eq!(row_offset_bytes(DISPLAY_ROW), 2 * PLANE_BYTES);
        assert_eq!(row_offset_bytes(SLOT_B_ROW), 3 * PLANE_BYTES);
        assert_eq!(row_offset_bytes(ATLAS_ROW), 4 * PLANE_BYTES);
        assert_eq!(RESOURCE_BYTES, row_offset_bytes(RESOURCE_HEIGHT));
        assert_eq!(RESOURCE_BYTES, 12 * PLANE_BYTES);
    }

    /// The resource is page-granular at both ends (it is a VMO) and every plane row offset is
    /// 64-byte aligned (the GL alias and the DMA paths address rows).
    #[test]
    fn test_reject_unaligned_layout() {
        assert_eq!(RESOURCE_BYTES % 4096, 0, "the resource must be whole pages");
        for row in [RETAINED_ROW, DISPLAY_ROW, SLOT_B_ROW, ATLAS_ROW] {
            assert_eq!(row_offset_bytes(row) % 64, 0, "row {row} is not 64-byte aligned");
        }
    }
}
