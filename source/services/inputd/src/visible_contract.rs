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
pub const VISIBLE_INPUT_CURSOR_END_X: i32 = 8;
pub const VISIBLE_INPUT_CURSOR_END_Y: i32 = 40;
pub const VISIBLE_INPUT_LEFT_SQUARE_X: u32 = 4;
pub const VISIBLE_INPUT_LEFT_SQUARE_Y: u32 = 36;
pub const VISIBLE_INPUT_RIGHT_SQUARE_X: u32 = 52;
pub const VISIBLE_INPUT_RIGHT_SQUARE_Y: u32 = 18;
pub const VISIBLE_INPUT_SQUARE_SIZE: u32 = 8;

pub const LIVE_POINTER_THRESHOLD: i32 = 1;
pub const LIVE_POINTER_NUMERATOR: i32 = 1;
pub const LIVE_POINTER_DENOMINATOR: i32 = 1;
pub const LIVE_POINTER_MAX_OUTPUT: i32 = 256;

#[must_use]
pub fn visible_hover_target_contains(x: i32, y: i32) -> bool {
    let (Ok(x), Ok(y)) = (u32::try_from(x), u32::try_from(y)) else {
        return false;
    };
    rect_contains(
        x,
        y,
        VISIBLE_INPUT_LEFT_SQUARE_X,
        VISIBLE_INPUT_LEFT_SQUARE_Y,
        VISIBLE_INPUT_SQUARE_SIZE,
        VISIBLE_INPUT_SQUARE_SIZE,
    )
}

pub fn visible_route_space() -> Result<PointerSpace, PointerStateError> {
    PointerSpace::new(VISIBLE_INPUT_PROOF_WIDTH, VISIBLE_INPUT_PROOF_HEIGHT)
}

pub fn visible_display_space() -> Result<PointerSpace, PointerStateError> {
    PointerSpace::new(
        windowd::VISIBLE_BOOTSTRAP_WIDTH,
        windowd::VISIBLE_BOOTSTRAP_HEIGHT,
    )
}

pub fn visible_pointer_transform() -> Result<PointerTransform, PointerStateError> {
    PointerTransform::new(visible_display_space()?, visible_route_space()?)
}

pub fn visible_display_start_position() -> Result<PointerPosition, PointerStateError> {
    Ok(
        visible_pointer_transform()?.route_to_display(PointerPosition::new(
            VISIBLE_INPUT_CURSOR_START_X,
            VISIBLE_INPUT_CURSOR_START_Y,
        )),
    )
}

const fn rect_contains(x: u32, y: u32, rx: u32, ry: u32, width: u32, height: u32) -> bool {
    x >= rx && y >= ry && x < rx + width && y < ry + height
}
