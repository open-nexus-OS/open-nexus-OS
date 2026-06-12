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

// NOTE: inputd no longer hit-tests. Hover/click/scroll/focus are resolved by
// windowd against its own rendered geometry (the compositor model — see
// `windowd::interaction`). The legacy proof-space target rects and their
// `*_target_contains` helpers were removed; a 64×48 route-space pointer could
// never match the real 1280×800 control rects. The route transform below is
// kept only for the initial cursor placement and inputd's own proof surface.

pub const LIVE_POINTER_THRESHOLD: i32 = 1;
pub const LIVE_POINTER_NUMERATOR: i32 = 1;
pub const LIVE_POINTER_DENOMINATOR: i32 = 1;
pub const LIVE_POINTER_MAX_OUTPUT: i32 = 256;

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

