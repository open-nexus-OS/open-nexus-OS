// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Host-testable visible-input coordinate contract for the OS-lite proof scene.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `inputd` host contract tests.
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

use pointer_state::{PointerPosition, PointerSpace, PointerStateError, PointerTransform};

pub const VISIBLE_INPUT_PROOF_WIDTH: u32 = 64;
pub const VISIBLE_INPUT_PROOF_HEIGHT: u32 = 48;
pub const VISIBLE_INPUT_CURSOR_START_X: i32 = 24;
pub const VISIBLE_INPUT_CURSOR_START_Y: i32 = 12;
// Cursor end position: center of the glass button in route space (55..62, 1..4).
pub const VISIBLE_INPUT_CURSOR_END_X: i32 = 58;
pub const VISIBLE_INPUT_CURSOR_END_Y: i32 = 2;
// Glass button position in display space: x=1100..1256, y=24..80 (1280×800 display).
// Mapped to route space (64×48): x = floor(1100*64/1280)=55 .. y = floor(24*48/800)=1.
pub const VISIBLE_INPUT_LEFT_SQUARE_X: u32 = 55;
pub const VISIBLE_INPUT_LEFT_SQUARE_Y: u32 = 1;
pub const VISIBLE_INPUT_RIGHT_SQUARE_X: u32 = 52;
pub const VISIBLE_INPUT_RIGHT_SQUARE_Y: u32 = 18;
pub const VISIBLE_INPUT_SQUARE_SIZE: u32 = 8;
const VISIBLE_INPUT_HOVER_TARGET_WIDTH: u32 = 8; // covers route x=55..62
const VISIBLE_INPUT_HOVER_TARGET_HEIGHT: u32 = 4; // covers route y=1..4
const VISIBLE_INPUT_CLOSE_TARGET_WIDTH: u32 = 8;
const VISIBLE_INPUT_CLOSE_TARGET_HEIGHT: u32 = 8;

// Proof-panel hover test target: display (56..480, 440..700) → route (2..24, 26..42).
// Centered on the hover card in the panel; independent of the glass button.
pub const PANEL_HOVER_TARGET_X: u32 = 4;
pub const PANEL_HOVER_TARGET_Y: u32 = 36;
const PANEL_HOVER_TARGET_WIDTH: u32 = 8;
const PANEL_HOVER_TARGET_HEIGHT: u32 = 5;

pub const LIVE_POINTER_THRESHOLD: i32 = 1;
pub const LIVE_POINTER_NUMERATOR: i32 = 1;
pub const LIVE_POINTER_DENOMINATOR: i32 = 1;
pub const LIVE_POINTER_MAX_OUTPUT: i32 = 256;

#[must_use]
pub fn visible_hover_target_contains(x: i32, y: i32) -> bool {
    visible_sidebar_open_target_contains(x, y)
}

#[must_use]
pub fn visible_sidebar_open_target_contains(x: i32, y: i32) -> bool {
    let (Ok(x), Ok(y)) = (u32::try_from(x), u32::try_from(y)) else {
        return false;
    };
    rect_contains(
        x,
        y,
        VISIBLE_INPUT_LEFT_SQUARE_X,
        VISIBLE_INPUT_LEFT_SQUARE_Y,
        VISIBLE_INPUT_HOVER_TARGET_WIDTH,
        VISIBLE_INPUT_HOVER_TARGET_HEIGHT,
    )
}

#[must_use]
pub fn visible_panel_hover_target_contains(x: i32, y: i32) -> bool {
    let (Ok(x), Ok(y)) = (u32::try_from(x), u32::try_from(y)) else {
        return false;
    };
    rect_contains(
        x,
        y,
        PANEL_HOVER_TARGET_X,
        PANEL_HOVER_TARGET_Y,
        PANEL_HOVER_TARGET_WIDTH,
        PANEL_HOVER_TARGET_HEIGHT,
    )
}

#[must_use]
pub fn visible_sidebar_close_target_contains(x: i32, y: i32) -> bool {
    let (Ok(x), Ok(y)) = (u32::try_from(x), u32::try_from(y)) else {
        return false;
    };
    rect_contains(
        x,
        y,
        VISIBLE_INPUT_RIGHT_SQUARE_X,
        VISIBLE_INPUT_RIGHT_SQUARE_Y,
        VISIBLE_INPUT_CLOSE_TARGET_WIDTH,
        VISIBLE_INPUT_CLOSE_TARGET_HEIGHT,
    )
}

pub fn visible_route_space() -> Result<PointerSpace, PointerStateError> {
    PointerSpace::new(VISIBLE_INPUT_PROOF_WIDTH, VISIBLE_INPUT_PROOF_HEIGHT)
}

pub fn visible_display_space() -> Result<PointerSpace, PointerStateError> {
    PointerSpace::new(windowd::VISIBLE_BOOTSTRAP_WIDTH, windowd::VISIBLE_BOOTSTRAP_HEIGHT)
}

pub fn visible_pointer_transform() -> Result<PointerTransform, PointerStateError> {
    PointerTransform::new(visible_display_space()?, visible_route_space()?)
}

pub fn visible_display_start_position() -> Result<PointerPosition, PointerStateError> {
    Ok(visible_pointer_transform()?.route_to_display(PointerPosition::new(
        VISIBLE_INPUT_CURSOR_START_X,
        VISIBLE_INPUT_CURSOR_START_Y,
    )))
}

const fn rect_contains(x: u32, y: u32, rx: u32, ry: u32, width: u32, height: u32) -> bool {
    x >= rx && y >= ry && x < rx + width && y < ry + height
}

#[cfg(test)]
mod tests {
    use super::{
        visible_panel_hover_target_contains, visible_sidebar_close_target_contains,
        visible_sidebar_open_target_contains, PANEL_HOVER_TARGET_X, PANEL_HOVER_TARGET_Y,
        VISIBLE_INPUT_LEFT_SQUARE_X, VISIBLE_INPUT_LEFT_SQUARE_Y, VISIBLE_INPUT_RIGHT_SQUARE_X,
        VISIBLE_INPUT_RIGHT_SQUARE_Y,
    };

    #[test]
    fn sidebar_open_target_hits_glass_button_region() {
        // Glass button is at route (55..62, 1..4) — clicking anywhere there triggers hover.
        assert!(visible_sidebar_open_target_contains(
            VISIBLE_INPUT_LEFT_SQUARE_X as i32,
            VISIBLE_INPUT_LEFT_SQUARE_Y as i32
        ));
        // Left side of old proof panel should NOT trigger hover.
        assert!(!visible_sidebar_open_target_contains(4, 36));
        assert!(!visible_sidebar_open_target_contains(0, 0));
    }

    #[test]
    fn sidebar_close_target_hits_right_square() {
        assert!(visible_sidebar_close_target_contains(
            VISIBLE_INPUT_RIGHT_SQUARE_X as i32,
            VISIBLE_INPUT_RIGHT_SQUARE_Y as i32
        ));
        assert!(!visible_sidebar_close_target_contains(0, 0));
    }

    #[test]
    fn panel_hover_target_is_independent_of_glass_button() {
        // Proof-panel hover target (route 4..11, 36..40) triggers independent of the glass button.
        assert!(visible_panel_hover_target_contains(
            PANEL_HOVER_TARGET_X as i32,
            PANEL_HOVER_TARGET_Y as i32
        ));
        // Glass button should NOT trigger the panel hover target.
        assert!(!visible_panel_hover_target_contains(55, 1));
        assert!(!visible_panel_hover_target_contains(0, 0));
        // Panel hover should NOT trigger the sidebar open target.
        assert!(!visible_sidebar_open_target_contains(
            PANEL_HOVER_TARGET_X as i32,
            PANEL_HOVER_TARGET_Y as i32
        ));
    }
}
