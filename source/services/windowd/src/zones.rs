// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: window TILING geometry (TASK-0066, the desktop default model): the zones a window
//! can be tiled into — halves, quarters, Fill — plus the Return to its pre-tile frame and the
//! arrangements that tile several windows at once (Left&Right, Top&Bottom, Quarters). Pure
//! functions of the display mode and the work area: the frame of a zone, the zone a drag
//! release at a pointer position means (edges → halves, corners → quarters, top → Fill), the
//! wire codes of the one windowd verb (`CONTROL_WIN_ZONE`) and the feed bits (RFC-0086). Pointer
//! and keyboard both land here; applying a frame is the runtime's job (`runtime/tiling.rs`).
//! Replaces `snap.rs` (TASK-0070's halves + top-edge fullscreen; the top edge now FILLS).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal (the wire codes are append-only)
//! TEST_COVERAGE: inline + `tests/tile_zones.rs`

/// How close (px) the POINTER must be to a display edge at drag release for an edge tile.
/// Small on purpose: an intentional shove, not a hair trigger on ordinary drags.
pub const TILE_EDGE_PX: i32 = 4;
/// A release within this many px of BOTH edges of a corner is a CORNER (a quarter) — the
/// pointer need not touch an edge there (board cycle 2026-10-07: the 4-px edge was too strict
/// a target for a corner).
pub const TILE_CORNER_PX: i32 = 64;
/// The margin steps the settings offer (`ui.tile.margin`).
pub const TILE_MARGINS: [u32; 3] = [0, 8, 16];

/// A zone a single window can occupy. Top/bottom halves exist for the Top&Bottom arrangement;
/// they are not offered as single tiles (the default model does not either).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Zone {
    #[default]
    None,
    LeftHalf,
    RightHalf,
    TopHalf,
    BottomHalf,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    /// The whole work area (chrome kept) — NOT fullscreen.
    Fill,
}

/// What a `CONTROL_WIN_ZONE` value (the low 4 bits) asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoneRequest {
    Tile(Zone),
    /// Back to the frame the window had before its first tile.
    Return,
    /// Tile the sender's window and the next windows in z-order together.
    Arrange(Arrangement),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arrangement {
    LeftRight,
    TopBottom,
    Quarters,
}

/// Why a tile request was refused — a stable marker token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileDeny {
    NotResizable,
    NoWindow,
    EdgesOff,
    ChordsOff,
    NoFocus,
}

impl TileDeny {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotResizable => "not-resizable",
            Self::NoWindow => "no-window",
            Self::EdgesOff => "edges-off",
            Self::ChordsOff => "chords-off",
            Self::NoFocus => "no-focus",
        }
    }
}

// Wire codes of the verb's low 4 bits — APPEND-ONLY (RFC-0086 feed bits reuse 1..=9).
pub const CODE_NONE: u8 = 0;
pub const CODE_LEFT_HALF: u8 = 1;
pub const CODE_RIGHT_HALF: u8 = 2;
pub const CODE_TOP_HALF: u8 = 3;
pub const CODE_BOTTOM_HALF: u8 = 4;
pub const CODE_TOP_LEFT: u8 = 5;
pub const CODE_TOP_RIGHT: u8 = 6;
pub const CODE_BOTTOM_LEFT: u8 = 7;
pub const CODE_BOTTOM_RIGHT: u8 = 8;
pub const CODE_FILL: u8 = 9;
pub const CODE_RETURN: u8 = 10;
pub const CODE_ARRANGE_LEFT_RIGHT: u8 = 11;
pub const CODE_ARRANGE_TOP_BOTTOM: u8 = 12;
pub const CODE_ARRANGE_QUARTERS: u8 = 13;

impl Zone {
    /// The feed/verb code of this zone (`CODE_*`).
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::None => CODE_NONE,
            Self::LeftHalf => CODE_LEFT_HALF,
            Self::RightHalf => CODE_RIGHT_HALF,
            Self::TopHalf => CODE_TOP_HALF,
            Self::BottomHalf => CODE_BOTTOM_HALF,
            Self::TopLeft => CODE_TOP_LEFT,
            Self::TopRight => CODE_TOP_RIGHT,
            Self::BottomLeft => CODE_BOTTOM_LEFT,
            Self::BottomRight => CODE_BOTTOM_RIGHT,
            Self::Fill => CODE_FILL,
        }
    }

    /// The marker token (`zone=…`).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::LeftHalf => "left-half",
            Self::RightHalf => "right-half",
            Self::TopHalf => "top-half",
            Self::BottomHalf => "bottom-half",
            Self::TopLeft => "top-left",
            Self::TopRight => "top-right",
            Self::BottomLeft => "bottom-left",
            Self::BottomRight => "bottom-right",
            Self::Fill => "fill",
        }
    }
}

/// Decodes the verb's low 4 bits; `None` for a code this build does not know (fail-closed).
#[must_use]
pub const fn request_from_code(code: u8) -> Option<ZoneRequest> {
    Some(match code {
        CODE_LEFT_HALF => ZoneRequest::Tile(Zone::LeftHalf),
        CODE_RIGHT_HALF => ZoneRequest::Tile(Zone::RightHalf),
        CODE_TOP_HALF => ZoneRequest::Tile(Zone::TopHalf),
        CODE_BOTTOM_HALF => ZoneRequest::Tile(Zone::BottomHalf),
        CODE_TOP_LEFT => ZoneRequest::Tile(Zone::TopLeft),
        CODE_TOP_RIGHT => ZoneRequest::Tile(Zone::TopRight),
        CODE_BOTTOM_LEFT => ZoneRequest::Tile(Zone::BottomLeft),
        CODE_BOTTOM_RIGHT => ZoneRequest::Tile(Zone::BottomRight),
        CODE_FILL => ZoneRequest::Tile(Zone::Fill),
        CODE_RETURN => ZoneRequest::Return,
        CODE_ARRANGE_LEFT_RIGHT => ZoneRequest::Arrange(Arrangement::LeftRight),
        CODE_ARRANGE_TOP_BOTTOM => ZoneRequest::Arrange(Arrangement::TopBottom),
        CODE_ARRANGE_QUARTERS => ZoneRequest::Arrange(Arrangement::Quarters),
        _ => return None,
    })
}

impl Arrangement {
    /// The zones an arrangement hands out, in z-order (the focused window takes the first).
    #[must_use]
    pub const fn zones(self) -> &'static [Zone] {
        match self {
            Self::LeftRight => &[Zone::LeftHalf, Zone::RightHalf],
            Self::TopBottom => &[Zone::TopHalf, Zone::BottomHalf],
            Self::Quarters => &[Zone::TopLeft, Zone::TopRight, Zone::BottomLeft, Zone::BottomRight],
        }
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::LeftRight => "left-right",
            Self::TopBottom => "top-bottom",
            Self::Quarters => "quarters",
        }
    }
}

/// A display-space frame `(x, y, w, h)`.
pub type TileFrame = (i32, i32, u32, u32);

/// The frame of `zone` inside the work area: `mode_w` wide, from row `top` (the status-bar
/// floor a floating window starts below) to `work_h` (the taskbar's ceiling), each tile inset
/// by `margin` on every side (two tiles share `margin` between them, so halves meet with one
/// gap). Odd sizes give the extra pixel to the right/bottom tile — the union of halves is the
/// whole area, never a column less. `Zone::None` is the whole work area (what Return falls back
/// to when no pre-tile frame exists).
#[must_use]
pub fn zone_frame(zone: Zone, mode_w: u32, work_h: u32, top: i32, margin: u32) -> TileFrame {
    let top = top.max(0) as u32;
    let area_h = work_h.saturating_sub(top);
    let m = margin.min(mode_w / 4).min(area_h / 4);
    let (ax, ay, aw, ah) =
        (m as i32, (top + m) as i32, mode_w.saturating_sub(2 * m), area_h.saturating_sub(2 * m));
    // The two columns / rows and the gap `m` between them.
    let col_w = aw.saturating_sub(m) / 2;
    let col_w_right = aw.saturating_sub(m).saturating_sub(col_w);
    let right_x = ax + (col_w + m) as i32;
    let row_h = ah.saturating_sub(m) / 2;
    let row_h_bottom = ah.saturating_sub(m).saturating_sub(row_h);
    let bottom_y = ay + (row_h + m) as i32;
    match zone {
        Zone::None | Zone::Fill => (ax, ay, aw, ah),
        Zone::LeftHalf => (ax, ay, col_w, ah),
        Zone::RightHalf => (right_x, ay, col_w_right, ah),
        Zone::TopHalf => (ax, ay, aw, row_h),
        Zone::BottomHalf => (ax, bottom_y, aw, row_h_bottom),
        Zone::TopLeft => (ax, ay, col_w, row_h),
        Zone::TopRight => (right_x, ay, col_w_right, row_h),
        Zone::BottomLeft => (ax, bottom_y, col_w, row_h_bottom),
        Zone::BottomRight => (right_x, bottom_y, col_w_right, row_h_bottom),
    }
}

/// The zone a title-bar drag RELEASED with the pointer at `(cx, cy)` means: a corner (within
/// `TILE_CORNER_PX` of both of its edges) → that quarter; the left/right edge → that half; the
/// top edge → Fill; anywhere else `None` (an ordinary drag). The bottom edge alone tiles nothing
/// (the taskbar lives there).
#[must_use]
pub fn release_zone_at(cx: i32, cy: i32, mode_w: u32, work_h: u32) -> Option<Zone> {
    let right = mode_w as i32 - 1;
    let at_left = cx <= TILE_EDGE_PX;
    let at_right = cx >= right - TILE_EDGE_PX;
    let at_top = cy <= TILE_EDGE_PX;
    let near_top = cy <= TILE_CORNER_PX;
    let near_bottom = cy >= work_h as i32 - 1 - TILE_CORNER_PX;
    let near_left = cx <= TILE_CORNER_PX;
    let near_right = cx >= right - TILE_CORNER_PX;
    if near_left && near_top {
        return Some(Zone::TopLeft);
    }
    if near_right && near_top {
        return Some(Zone::TopRight);
    }
    if near_left && near_bottom {
        return Some(Zone::BottomLeft);
    }
    if near_right && near_bottom {
        return Some(Zone::BottomRight);
    }
    if at_top {
        return Some(Zone::Fill);
    }
    if at_left {
        return Some(Zone::LeftHalf);
    }
    if at_right {
        return Some(Zone::RightHalf);
    }
    None
}

/// Where a RETURNED window goes when the user drags it off its tile: its pre-tile size, placed
/// so the pointer stays over the title bar at the same relative position it grabbed —
/// clamped into the display so no part leaves the screen.
#[must_use]
pub fn return_frame_under_pointer(
    pre: TileFrame,
    cx: i32,
    grab_dx: i32,
    tiled_w: u32,
    mode_w: u32,
) -> TileFrame {
    let (_, y, w, h) = pre;
    // The grab's relative position inside the TILED title bar, carried over to the returned width.
    let rel =
        if tiled_w == 0 { 0 } else { (grab_dx.max(0) as i64 * w as i64 / tiled_w as i64) as i32 };
    let max_x = mode_w.saturating_sub(w) as i32;
    let x = (cx - rel).clamp(0, max_x.max(0));
    (x, y, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: u32 = 1280;
    const H: u32 = 800;

    #[test]
    fn halves_and_quarters_partition_the_work_area_exactly() {
        for (w, h, top) in [(W, H, 36), (1919u32, 1079u32, 0), (801, 601, 24)] {
            let (lx, ly, lw, lh) = zone_frame(Zone::LeftHalf, w, h, top, 0);
            let (rx, _, rw, _) = zone_frame(Zone::RightHalf, w, h, top, 0);
            assert_eq!((lx, ly), (0, top));
            assert_eq!(lw + rw, w, "halves cover the width ({w})");
            assert_eq!(rx as u32, lw, "right half starts where the left ends");
            assert_eq!(lh, h - top as u32);
            let (_, ty, _, th) = zone_frame(Zone::TopLeft, w, h, top, 0);
            let (_, by, _, bh) = zone_frame(Zone::BottomLeft, w, h, top, 0);
            assert_eq!(ty, top);
            assert_eq!(th + bh, h - top as u32, "quarters cover the height");
            assert_eq!(by as u32, top as u32 + th);
            assert_eq!(zone_frame(Zone::Fill, w, h, top, 0), (0, top, w, h - top as u32));
        }
    }

    #[test]
    fn margins_inset_every_tile_and_keep_one_gap_between_neighbours() {
        let (lx, ly, lw, lh) = zone_frame(Zone::LeftHalf, W, H, 36, 8);
        let (rx, _, rw, _) = zone_frame(Zone::RightHalf, W, H, 36, 8);
        assert_eq!((lx, ly), (8, 44));
        assert_eq!(rx as u32, 8 + lw + 8, "one margin between the halves");
        assert_eq!(8 + lw + 8 + rw + 8, W, "margins + halves = the width");
        assert_eq!(lh, H - 36 - 16);
        assert_eq!(zone_frame(Zone::Fill, W, H, 36, 16), (16, 52, W - 32, H - 36 - 32));
    }

    #[test]
    fn release_edges_and_corners_resolve_like_the_desktop() {
        assert_eq!(release_zone_at(0, 400, W, H), Some(Zone::LeftHalf));
        assert_eq!(release_zone_at(1279, 400, W, H), Some(Zone::RightHalf));
        assert_eq!(
            release_zone_at(600, 0, W, H),
            Some(Zone::Fill),
            "top edge FILLS, no fullscreen"
        );
        assert_eq!(release_zone_at(0, 10, W, H), Some(Zone::TopLeft));
        assert_eq!(
            release_zone_at(30, 2, W, H),
            Some(Zone::TopLeft),
            "top edge near the left corner"
        );
        assert_eq!(
            release_zone_at(40, 40, W, H),
            Some(Zone::TopLeft),
            "inside the corner band, touching no edge"
        );
        assert_eq!(release_zone_at(80, 80, W, H), None, "outside the corner band, no edge");
        assert_eq!(release_zone_at(1279, 20, W, H), Some(Zone::TopRight));
        assert_eq!(release_zone_at(2, 790, W, H), Some(Zone::BottomLeft));
        assert_eq!(release_zone_at(1278, 770, W, H), Some(Zone::BottomRight));
        assert_eq!(release_zone_at(600, 799, W, H), None, "the bottom edge is the taskbar's");
        assert_eq!(release_zone_at(600, 400, W, H), None);
        assert_eq!(
            release_zone_at(0, 100, W, H),
            Some(Zone::LeftHalf),
            "below the corner band = half"
        );
    }

    #[test]
    fn codes_round_trip_and_unknown_codes_are_refused() {
        for zone in [
            Zone::LeftHalf,
            Zone::RightHalf,
            Zone::TopHalf,
            Zone::BottomHalf,
            Zone::TopLeft,
            Zone::TopRight,
            Zone::BottomLeft,
            Zone::BottomRight,
            Zone::Fill,
        ] {
            assert_eq!(request_from_code(zone.code()), Some(ZoneRequest::Tile(zone)));
            assert!(zone.code() <= 0b1111, "fits the 4 feed bits");
        }
        assert_eq!(request_from_code(CODE_RETURN), Some(ZoneRequest::Return));
        assert_eq!(
            request_from_code(CODE_ARRANGE_QUARTERS),
            Some(ZoneRequest::Arrange(Arrangement::Quarters))
        );
        assert_eq!(request_from_code(0), None);
        assert_eq!(request_from_code(14), None);
        assert_eq!(request_from_code(15), None);
        assert_eq!(Arrangement::Quarters.zones().len(), 4);
    }

    #[test]
    fn a_returned_window_keeps_the_pointer_on_its_title_bar_and_stays_on_screen() {
        let pre = (300, 140, 800, 600);
        // Grabbed at 10% of a 640-wide tile → 10% of the returned 800 width.
        assert_eq!(return_frame_under_pointer(pre, 64, 64, 640, W), (0, 140, 800, 600));
        assert_eq!(return_frame_under_pointer(pre, 700, 320, 640, W), (300, 140, 800, 600));
        // Near the right edge the frame is clamped into the display.
        assert_eq!(return_frame_under_pointer(pre, 1270, 10, 640, W), (480, 140, 800, 600));
    }
}
