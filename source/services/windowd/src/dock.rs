// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! ⚠ CLEANUP-MAP (docs/dev/ui/windowd-cleanup-map.md): MOVE → DSL-Shell-App (Dock = Shell-UI).
//! DO NOT EXTEND — new capability belongs at the target, not here.
//

//! CONTEXT: pure dock geometry (TASK-0070 Phase 2) — the bottom-center glass
//! bar holding one icon per MINIMIZED window. Layout + hit-testing only, fully
//! host-testable; rendering/composition stay in the runtime. The dock exists
//! ONLY while ≥1 window is minimized (no permanent taskbar — user decision),
//! and its slot order is the stack's stable `minimized_list()` order.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 5 tests

/// Icon cell edge (the clickable square around each icon).
pub const DOCK_CELL: u32 = 48;
/// Gap between icon cells.
pub const DOCK_GAP: u32 = 10;
/// Horizontal padding inside the bar (left + right of the cell run).
pub const DOCK_PAD_X: u32 = 14;
/// Bar height (icon cell + vertical padding).
pub const DOCK_H: u32 = DOCK_CELL + 16;
/// Gap between the bar's bottom edge and the display's bottom edge.
pub const DOCK_MARGIN_BOTTOM: u32 = 14;
/// Corner radius of the glass bar.
pub const DOCK_RADIUS: u32 = 14;

/// A dock rectangle in display space.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DockRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Bar width for `n` icons (`n ≥ 1`).
pub fn dock_width(n: usize) -> u32 {
    let n = n.max(1) as u32;
    2 * DOCK_PAD_X + n * DOCK_CELL + (n - 1) * DOCK_GAP
}

/// The bar's display rect for `n` icons, bottom-center.
pub fn dock_rect(mode_w: u32, mode_h: u32, n: usize) -> DockRect {
    let width = dock_width(n).min(mode_w);
    DockRect {
        x: mode_w.saturating_sub(width) / 2,
        y: mode_h.saturating_sub(DOCK_H + DOCK_MARGIN_BOTTOM),
        width,
        height: DOCK_H,
    }
}

/// Slot `i`'s icon-cell rect inside `bar` (display space).
pub fn dock_slot_rect(bar: DockRect, i: usize) -> DockRect {
    DockRect {
        x: bar.x + DOCK_PAD_X + i as u32 * (DOCK_CELL + DOCK_GAP),
        y: bar.y + (DOCK_H - DOCK_CELL) / 2,
        width: DOCK_CELL,
        height: DOCK_CELL,
    }
}

/// Which slot (if any) a display-space point hits. Points inside the bar but
/// between cells resolve to `None` (a press there just consumes — the bar is
/// above the desktop, clicks must not fall through).
pub fn dock_slot_at(bar: DockRect, n: usize, cx: i32, cy: i32) -> Option<usize> {
    if !dock_contains(bar, cx, cy) {
        return None;
    }
    for i in 0..n {
        let cell = dock_slot_rect(bar, i);
        if cx >= cell.x as i32
            && cx < (cell.x + cell.width) as i32
            && cy >= cell.y as i32
            && cy < (cell.y + cell.height) as i32
        {
            return Some(i);
        }
    }
    None
}

/// Whether a display-space point lies inside the bar at all.
pub fn dock_contains(bar: DockRect, cx: i32, cy: i32) -> bool {
    cx >= bar.x as i32
        && cx < (bar.x + bar.width) as i32
        && cy >= bar.y as i32
        && cy < (bar.y + bar.height) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: u32 = 1280;
    const H: u32 = 800;

    #[test]
    fn bar_is_bottom_centered_and_grows_with_icons() {
        let one = dock_rect(W, H, 1);
        let two = dock_rect(W, H, 2);
        assert!(two.width > one.width);
        // Centered: symmetric margins (±1 for odd widths).
        assert!((one.x as i32 - (W - one.x - one.width) as i32).abs() <= 1);
        // Bottom-anchored with the margin.
        assert_eq!(one.y + one.height + DOCK_MARGIN_BOTTOM, H);
        assert_eq!(one.width, dock_width(1));
    }

    #[test]
    fn slots_lie_inside_the_bar_without_overlap() {
        let bar = dock_rect(W, H, 3);
        for i in 0..3 {
            let cell = dock_slot_rect(bar, i);
            assert!(cell.x >= bar.x && cell.x + cell.width <= bar.x + bar.width);
            assert!(cell.y >= bar.y && cell.y + cell.height <= bar.y + bar.height);
            if i > 0 {
                let prev = dock_slot_rect(bar, i - 1);
                assert!(cell.x >= prev.x + prev.width, "cells must not overlap");
            }
        }
    }

    #[test]
    fn hit_resolves_cell_centers_and_rejects_gaps() {
        let bar = dock_rect(W, H, 2);
        for i in 0..2 {
            let cell = dock_slot_rect(bar, i);
            let cx = (cell.x + cell.width / 2) as i32;
            let cy = (cell.y + cell.height / 2) as i32;
            assert_eq!(dock_slot_at(bar, 2, cx, cy), Some(i));
        }
        // The gap between cell 0 and 1 consumes but resolves no slot.
        let c0 = dock_slot_rect(bar, 0);
        let gap_x = (c0.x + c0.width + DOCK_GAP / 2) as i32;
        let cy = (bar.y + bar.height / 2) as i32;
        assert!(dock_contains(bar, gap_x, cy));
        assert_eq!(dock_slot_at(bar, 2, gap_x, cy), None);
    }

    #[test]
    fn outside_the_bar_is_no_hit() {
        let bar = dock_rect(W, H, 1);
        assert!(!dock_contains(bar, bar.x as i32 - 1, (bar.y + 5) as i32));
        assert_eq!(dock_slot_at(bar, 1, 5, 5), None);
        // Below the bar (display bottom margin) is not the bar.
        assert!(!dock_contains(bar, (bar.x + 5) as i32, (bar.y + bar.height) as i32));
    }

    #[test]
    fn width_never_exceeds_display() {
        let bar = dock_rect(200, H, 4);
        assert!(bar.width <= 200);
    }
}
