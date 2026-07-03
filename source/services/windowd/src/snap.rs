// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: pure drag-to-edge snap geometry (TASK-0070 Phase 3). Releasing a
//! title-bar drag with the POINTER at a display edge snaps the window: left
//! edge → left half, right edge → right half, top edge → fullscreen (covers
//! the chrome). Pointer-driven only — there are NO snap keyboard shortcuts
//! (explicit user decision). Pure decisions, fully host-testable; applying
//! the frame (content re-render at size) is the runtime's job.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 4 tests

/// How close (px) the POINTER must be to a display edge at drag-release for
/// the snap to trigger. Small on purpose: an intentional shove, not a hair
/// trigger on ordinary drags near the border.
pub const SNAP_EDGE_PX: i32 = 4;

/// What releasing a drag at the current pointer position snaps to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SnapTarget {
    /// Pointer at the left display edge → the window fills the left half.
    LeftHalf,
    /// Pointer at the right display edge → the window fills the right half.
    RightHalf,
    /// Pointer at the top display edge → fullscreen (over the chrome).
    Fullscreen,
}

/// The snap target for a drag released with the pointer at `(cx, cy)`.
/// Top wins over the corners (the classic maximize gesture); `None` = an
/// ordinary drag release.
pub fn snap_target_at(cx: i32, cy: i32, mode_w: u32) -> Option<SnapTarget> {
    if cy <= SNAP_EDGE_PX {
        return Some(SnapTarget::Fullscreen);
    }
    if cx <= SNAP_EDGE_PX {
        return Some(SnapTarget::LeftHalf);
    }
    if cx >= mode_w as i32 - 1 - SNAP_EDGE_PX {
        return Some(SnapTarget::RightHalf);
    }
    None
}

/// The display-space frame `(x, y, w, h)` of a half-snap target. (Fullscreen
/// has no frame here — the runtime routes it through the fullscreen toggle so
/// the chrome-cover + restore semantics stay in ONE place.)
pub fn snap_frame(target: SnapTarget, mode_w: u32, mode_h: u32) -> (i32, i32, u32, u32) {
    let half = mode_w / 2;
    match target {
        SnapTarget::LeftHalf => (0, 0, half, mode_h),
        SnapTarget::RightHalf => (half as i32, 0, mode_w - half, mode_h),
        SnapTarget::Fullscreen => (0, 0, mode_w, mode_h),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: u32 = 1280;
    const H: u32 = 800;

    #[test]
    fn edges_map_to_targets_top_wins_corners() {
        assert_eq!(snap_target_at(600, 0, W), Some(SnapTarget::Fullscreen));
        assert_eq!(snap_target_at(0, 400, W), Some(SnapTarget::LeftHalf));
        assert_eq!(snap_target_at((W - 1) as i32, 400, W), Some(SnapTarget::RightHalf));
        // Top-left corner: fullscreen wins (top checked first).
        assert_eq!(snap_target_at(0, 0, W), Some(SnapTarget::Fullscreen));
    }

    #[test]
    fn interior_release_is_no_snap() {
        assert_eq!(snap_target_at(600, 400, W), None);
        // Just past the trigger band: no snap.
        assert_eq!(snap_target_at(SNAP_EDGE_PX + 1, 400, W), None);
        assert_eq!(snap_target_at(600, SNAP_EDGE_PX + 1, W), None);
    }

    #[test]
    fn half_frames_tile_the_display_exactly() {
        let (lx, ly, lw, lh) = snap_frame(SnapTarget::LeftHalf, W, H);
        let (rx, ry, rw, rh) = snap_frame(SnapTarget::RightHalf, W, H);
        assert_eq!((lx, ly, lh), (0, 0, H));
        assert_eq!((ry, rh), (0, H));
        // The halves cover the full width with no gap and no overlap —
        // including odd widths (the right half absorbs the odd pixel).
        assert_eq!(lx + lw as i32, rx);
        assert_eq!(lw + rw, W);
        let (ox, _, ow, _) = snap_frame(SnapTarget::LeftHalf, 1281, H);
        let (opx, _, opw, _) = snap_frame(SnapTarget::RightHalf, 1281, H);
        assert_eq!(ox + ow as i32, opx);
        assert_eq!(ow + opw, 1281);
    }

    #[test]
    fn fullscreen_frame_is_the_display() {
        assert_eq!(snap_frame(SnapTarget::Fullscreen, W, H), (0, 0, W, H));
    }
}
