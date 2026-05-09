// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Service-facing visible-state renderer for the service-owned display path.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Exercised through `fbdevd` host tests and visible-bootstrap QEMU proofs.
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

use alloc::vec;

use input_live_protocol::VisibleState;

use crate::error::{Result, WindowdError};
use crate::frame::Frame;
use crate::server::VISIBLE_CURSOR_BGRA;
use crate::smoke::{VisibleBootstrapMode, VISIBLE_INPUT_CLICK_BGRA, VISIBLE_INPUT_KEYBOARD_BGRA};

const VISIBLE_INPUT_PROOF_WIDTH: u32 = 64;
const VISIBLE_INPUT_PROOF_HEIGHT: u32 = 48;
const VISIBLE_INPUT_LEFT_SQUARE_X: u32 = 4;
const VISIBLE_INPUT_LEFT_SQUARE_Y: u32 = 36;
const VISIBLE_INPUT_RIGHT_SQUARE_X: u32 = 52;
const VISIBLE_INPUT_RIGHT_SQUARE_Y: u32 = 18;
const VISIBLE_INPUT_SQUARE_SIZE: u32 = 8;
const VISIBLE_INPUT_BGRA: [u8; 4] = [0x18, 0x30, 0x88, 0xff];
const VISIBLE_INPUT_LEFT_IDLE_BGRA: [u8; 4] = [0x30, 0x70, 0xd8, 0xff];
const VISIBLE_INPUT_LEFT_HOVER_BGRA: [u8; 4] = [0x20, 0xd0, 0xf8, 0xff];
const VISIBLE_INPUT_RIGHT_IDLE_BGRA: [u8; 4] = [0x90, 0x40, 0x40, 0xff];
pub const VISIBLE_INPUT_WHEEL_IDLE_BGRA: [u8; 4] = [0x60, 0x60, 0x90, 0xff];
pub const VISIBLE_INPUT_WHEEL_ACTIVE_BGRA: [u8; 4] = [0x30, 0xf0, 0xf0, 0xff];
const VISIBLE_INPUT_WHEEL_X: u32 = 15;
const VISIBLE_INPUT_WHEEL_UP_Y: u32 = 36;
const VISIBLE_INPUT_WHEEL_DOWN_Y: u32 = 40;
const VISIBLE_INPUT_WHEEL_WIDTH: u32 = 5;
const VISIBLE_INPUT_WHEEL_HEIGHT: u32 = 4;

pub fn compose_live_visible_frame(
    state: VisibleState,
    mode: VisibleBootstrapMode,
) -> Result<Frame> {
    let mode = mode.validate()?;
    let mut frame = Frame {
        width: mode.width,
        height: mode.height,
        stride: mode.stride,
        pixels: vec![0u8; mode.byte_len()?],
    };
    for y in 0..mode.height {
        let idx = (y as usize)
            .checked_mul(mode.stride as usize)
            .ok_or(WindowdError::ArithmeticOverflow)?;
        copy_live_visible_row(
            state,
            mode,
            y,
            &mut frame.pixels[idx..idx + mode.stride as usize],
        )?;
    }
    Ok(frame)
}

pub fn copy_live_visible_row(
    state: VisibleState,
    mode: VisibleBootstrapMode,
    y: u32,
    row: &mut [u8],
) -> Result<()> {
    let mode = mode.validate()?;
    let row_len = mode.stride as usize;
    if row.len() < row_len {
        return Err(WindowdError::BufferLengthMismatch);
    }
    let left_square = if state.launcher_click_visible {
        VISIBLE_INPUT_CLICK_BGRA
    } else if state.hover_visible {
        VISIBLE_INPUT_LEFT_HOVER_BGRA
    } else {
        VISIBLE_INPUT_LEFT_IDLE_BGRA
    };
    let right_square = if state.keyboard_visible {
        VISIBLE_INPUT_KEYBOARD_BGRA
    } else {
        VISIBLE_INPUT_RIGHT_IDLE_BGRA
    };
    let wheel_up = if state.wheel_up_visible {
        VISIBLE_INPUT_WHEEL_ACTIVE_BGRA
    } else {
        VISIBLE_INPUT_WHEEL_IDLE_BGRA
    };
    let wheel_down = if state.wheel_down_visible {
        VISIBLE_INPUT_WHEEL_ACTIVE_BGRA
    } else {
        VISIBLE_INPUT_WHEEL_IDLE_BGRA
    };
    let left_rect = route_rect_to_display(
        VISIBLE_INPUT_LEFT_SQUARE_X,
        VISIBLE_INPUT_LEFT_SQUARE_Y,
        VISIBLE_INPUT_SQUARE_SIZE,
        VISIBLE_INPUT_SQUARE_SIZE,
        mode,
    );
    let right_rect = route_rect_to_display(
        VISIBLE_INPUT_RIGHT_SQUARE_X,
        VISIBLE_INPUT_RIGHT_SQUARE_Y,
        VISIBLE_INPUT_SQUARE_SIZE,
        VISIBLE_INPUT_SQUARE_SIZE,
        mode,
    );
    let cursor_rect = cursor_display_rect(state, mode);
    for x in 0..mode.width {
        let route_x = scale_display_to_route_axis(x, mode.width, VISIBLE_INPUT_PROOF_WIDTH);
        let route_y = scale_display_to_route_axis(y, mode.height, VISIBLE_INPUT_PROOF_HEIGHT);
        let mut bgra = visible_input_background_bgra(route_x, route_y);
        if left_rect.contains(x, y) {
            bgra = left_square;
        } else if right_rect.contains(x, y) {
            bgra = right_square;
        } else if wheel_triangle_contains(route_x, route_y, VISIBLE_INPUT_WHEEL_UP_Y, true) {
            bgra = wheel_up;
        } else if wheel_triangle_contains(route_x, route_y, VISIBLE_INPUT_WHEEL_DOWN_Y, false) {
            bgra = wheel_down;
        }
        if state.scene_ready && cursor_rect.is_some_and(|rect| rect.contains(x, y)) {
            bgra = VISIBLE_CURSOR_BGRA;
        }
        let idx = (x as usize)
            .checked_mul(4)
            .ok_or(WindowdError::ArithmeticOverflow)?;
        row[idx..idx + 4].copy_from_slice(&bgra);
    }
    Ok(())
}

fn cursor_display_rect(state: VisibleState, mode: VisibleBootstrapMode) -> Option<DisplayRect> {
    let (Ok(x), Ok(y)) = (u32::try_from(state.cursor_x), u32::try_from(state.cursor_y)) else {
        return None;
    };
    if x >= mode.width || y >= mode.height {
        return None;
    }
    let extent = cursor_display_extent(mode);
    Some(DisplayRect {
        left: x,
        top: y,
        right: x.saturating_add(extent.width).min(mode.width),
        bottom: y.saturating_add(extent.height).min(mode.height),
    })
}

fn route_rect_to_display(
    left: u32,
    top: u32,
    width: u32,
    height: u32,
    mode: VisibleBootstrapMode,
) -> DisplayRect {
    DisplayRect {
        left: scale_rect_start(left, VISIBLE_INPUT_PROOF_WIDTH, mode.width),
        top: scale_rect_start(top, VISIBLE_INPUT_PROOF_HEIGHT, mode.height),
        right: scale_rect_end(
            left.saturating_add(width),
            VISIBLE_INPUT_PROOF_WIDTH,
            mode.width,
        ),
        bottom: scale_rect_end(
            top.saturating_add(height),
            VISIBLE_INPUT_PROOF_HEIGHT,
            mode.height,
        ),
    }
}

fn wheel_triangle_contains(route_x: u32, route_y: u32, top_y: u32, points_up: bool) -> bool {
    if route_x < VISIBLE_INPUT_WHEEL_X
        || route_x >= VISIBLE_INPUT_WHEEL_X + VISIBLE_INPUT_WHEEL_WIDTH
        || route_y < top_y
        || route_y >= top_y + VISIBLE_INPUT_WHEEL_HEIGHT
    {
        return false;
    }
    let local_x = route_x - VISIBLE_INPUT_WHEEL_X;
    let local_y = route_y - top_y;
    let row = if points_up {
        local_y
    } else {
        VISIBLE_INPUT_WHEEL_HEIGHT - 1 - local_y
    };
    let center = VISIBLE_INPUT_WHEEL_WIDTH / 2;
    let left = center.saturating_sub(row);
    let right = (center + row).min(VISIBLE_INPUT_WHEEL_WIDTH - 1);
    local_x >= left && local_x <= right
}

fn cursor_display_extent(mode: VisibleBootstrapMode) -> DisplayExtent {
    DisplayExtent {
        width: ceil_div(mode.width, VISIBLE_INPUT_PROOF_WIDTH),
        height: ceil_div(mode.height, VISIBLE_INPUT_PROOF_HEIGHT),
    }
}

fn scale_display_to_route_axis(value: u32, display_bound: u32, route_bound: u32) -> u32 {
    if display_bound == 0 || route_bound == 0 {
        return 0;
    }
    ((u64::from(value.min(display_bound.saturating_sub(1))) * u64::from(route_bound))
        / u64::from(display_bound))
    .min(u64::from(route_bound.saturating_sub(1))) as u32
}

fn scale_rect_start(value: u32, source_bound: u32, target_bound: u32) -> u32 {
    if source_bound == 0 || target_bound == 0 {
        return 0;
    }
    ((u64::from(value.min(source_bound)) * u64::from(target_bound)) / u64::from(source_bound))
        .min(u64::from(target_bound)) as u32
}

fn scale_rect_end(value: u32, source_bound: u32, target_bound: u32) -> u32 {
    if source_bound == 0 || target_bound == 0 {
        return 0;
    }
    ((u64::from(value.min(source_bound)) * u64::from(target_bound)
        + u64::from(source_bound.saturating_sub(1)))
        / u64::from(source_bound))
    .min(u64::from(target_bound)) as u32
}

const fn ceil_div(value: u32, divisor: u32) -> u32 {
    if divisor == 0 {
        return 1;
    }
    let rounded = value.div_ceil(divisor);
    if rounded == 0 {
        1
    } else {
        rounded
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DisplayExtent {
    width: u32,
    height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DisplayRect {
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
}

impl DisplayRect {
    fn contains(self, x: u32, y: u32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
}

fn visible_input_background_bgra(x: u32, y: u32) -> [u8; 4] {
    let stripe = ((x / 8) + (y / 8)) & 1;
    if stripe == 0 {
        VISIBLE_INPUT_BGRA
    } else {
        [0x24, 0x38, 0xa0, 0xff]
    }
}
