// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Canonical pointer state and transform primitives for display/window routing.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Unit tests in this crate plus service contract coverage.
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

#![cfg_attr(all(nexus_env = "os", target_os = "none"), no_std)]
#![forbid(unsafe_code)]

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerStateError {
    InvalidSpace,
    InvalidCalibration,
    InitialPositionOutOfBounds { x: i32, y: i32 },
}

impl PointerStateError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidSpace => "pointer_state.space.invalid",
            Self::InvalidCalibration => "pointer_state.calibration.invalid",
            Self::InitialPositionOutOfBounds { .. } => {
                "pointer_state.position.initial_out_of_bounds"
            }
        }
    }
}

impl core::fmt::Display for PointerStateError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidSpace => f.write_str("pointer space must be non-zero"),
            Self::InvalidCalibration => {
                f.write_str("absolute pointer calibration must have max > min")
            }
            Self::InitialPositionOutOfBounds { x, y } => {
                write!(f, "initial pointer position out of bounds: ({x}, {y})")
            }
        }
    }
}

#[cfg(not(all(nexus_env = "os", target_os = "none")))]
impl std::error::Error for PointerStateError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointerSpace {
    width: u32,
    height: u32,
}

impl PointerSpace {
    pub fn new(width: u32, height: u32) -> Result<Self, PointerStateError> {
        if width == 0 || height == 0 {
            return Err(PointerStateError::InvalidSpace);
        }
        Ok(Self { width, height })
    }

    #[must_use]
    pub const fn width(self) -> u32 {
        self.width
    }

    #[must_use]
    pub const fn height(self) -> u32 {
        self.height
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointerPosition {
    pub x: i32,
    pub y: i32,
}

impl PointerPosition {
    #[must_use]
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AbsoluteAxisCalibration {
    min: i32,
    max: i32,
}

impl AbsoluteAxisCalibration {
    pub fn new(min: i32, max: i32) -> Result<Self, PointerStateError> {
        if max <= min {
            return Err(PointerStateError::InvalidCalibration);
        }
        Ok(Self { min, max })
    }

    #[must_use]
    pub const fn min(self) -> i32 {
        self.min
    }

    #[must_use]
    pub const fn max(self) -> i32 {
        self.max
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointerTransform {
    display: PointerSpace,
    route: PointerSpace,
}

impl PointerTransform {
    pub fn new(display: PointerSpace, route: PointerSpace) -> Result<Self, PointerStateError> {
        if display.width == 0 || display.height == 0 || route.width == 0 || route.height == 0 {
            return Err(PointerStateError::InvalidSpace);
        }
        Ok(Self { display, route })
    }

    #[must_use]
    pub const fn display_space(self) -> PointerSpace {
        self.display
    }

    #[must_use]
    pub const fn route_space(self) -> PointerSpace {
        self.route
    }

    #[must_use]
    pub fn display_to_route(self, position: PointerPosition) -> PointerPosition {
        PointerPosition {
            x: scale_point_axis(position.x, self.display.width, self.route.width),
            y: scale_point_axis(position.y, self.display.height, self.route.height),
        }
    }

    #[must_use]
    pub fn route_to_display(self, position: PointerPosition) -> PointerPosition {
        let x_rect =
            self.route_rect_to_display(u32::try_from(position.x.max(0)).unwrap_or(0), 0, 1, 1);
        let y_rect =
            self.route_rect_to_display(0, u32::try_from(position.y.max(0)).unwrap_or(0), 1, 1);
        PointerPosition {
            x: midpoint(x_rect.left, x_rect.right),
            y: midpoint(y_rect.top, y_rect.bottom),
        }
    }

    #[must_use]
    pub fn route_rect_to_display(
        self,
        left: u32,
        top: u32,
        width: u32,
        height: u32,
    ) -> DisplayRect {
        let start_x = scale_rect_start(left, self.route.width, self.display.width);
        let start_y = scale_rect_start(top, self.route.height, self.display.height);
        let end_x =
            scale_rect_end(left.saturating_add(width), self.route.width, self.display.width);
        let end_y =
            scale_rect_end(top.saturating_add(height), self.route.height, self.display.height);
        DisplayRect {
            left: start_x.min(self.display.width),
            top: start_y.min(self.display.height),
            right: end_x.min(self.display.width).max(start_x),
            bottom: end_y.min(self.display.height).max(start_y),
        }
    }

    #[must_use]
    pub fn display_extent_from_route(self) -> PointerExtent {
        PointerExtent {
            width: ceil_div(self.display.width, self.route.width),
            height: ceil_div(self.display.height, self.route.height),
        }
    }

    pub fn scale_absolute_axis(
        self,
        value: i32,
        calibration: AbsoluteAxisCalibration,
        axis: PointerAxis,
    ) -> i32 {
        let span = i64::from(calibration.max() - calibration.min());
        let clamped = value.clamp(calibration.min(), calibration.max()) - calibration.min();
        let target = match axis {
            PointerAxis::X => self.display.width,
            PointerAxis::Y => self.display.height,
        };
        if target == 0 {
            return 0;
        }
        let top = i64::from(target.saturating_sub(1));
        ((i64::from(clamped) * top) / span) as i32
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerAxis {
    X,
    Y,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointerExtent {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayRect {
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
}

impl DisplayRect {
    #[must_use]
    pub fn contains(self, x: u32, y: u32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointerState {
    space: PointerSpace,
    display: PointerPosition,
}

impl PointerState {
    pub fn new(space: PointerSpace, initial: PointerPosition) -> Result<Self, PointerStateError> {
        if !contains(space, initial) {
            return Err(PointerStateError::InitialPositionOutOfBounds {
                x: initial.x,
                y: initial.y,
            });
        }
        Ok(Self { space, display: initial })
    }

    #[must_use]
    pub const fn display_position(self) -> PointerPosition {
        self.display
    }

    #[must_use]
    pub const fn display_space(self) -> PointerSpace {
        self.space
    }

    #[must_use]
    pub fn route_position(self, transform: PointerTransform) -> PointerPosition {
        transform.display_to_route(self.display)
    }

    pub fn apply_relative(&mut self, dx: i32, dy: i32) -> PointerPosition {
        self.display = PointerPosition {
            x: clamp_axis(self.display.x.saturating_add(dx), self.space.width),
            y: clamp_axis(self.display.y.saturating_add(dy), self.space.height),
        };
        self.display
    }

    pub fn apply_absolute(&mut self, x: Option<i32>, y: Option<i32>) -> PointerPosition {
        self.display = PointerPosition {
            x: clamp_axis(x.unwrap_or(self.display.x), self.space.width),
            y: clamp_axis(y.unwrap_or(self.display.y), self.space.height),
        };
        self.display
    }
}

fn contains(space: PointerSpace, position: PointerPosition) -> bool {
    position.x >= 0
        && position.y >= 0
        && u32::try_from(position.x).ok().is_some_and(|x| x < space.width)
        && u32::try_from(position.y).ok().is_some_and(|y| y < space.height)
}

fn clamp_axis(value: i32, bound: u32) -> i32 {
    let max = i32::try_from(bound.saturating_sub(1)).unwrap_or(i32::MAX);
    value.clamp(0, max)
}

fn scale_point_axis(value: i32, source_bound: u32, target_bound: u32) -> i32 {
    let clamped = clamp_axis(value, source_bound);
    scale_point_axis_u32(u32::try_from(clamped).unwrap_or(0), source_bound, target_bound) as i32
}

fn scale_point_axis_u32(value: u32, source_bound: u32, target_bound: u32) -> u32 {
    if source_bound == 0 || target_bound == 0 {
        return 0;
    }
    let clamped = value.min(source_bound.saturating_sub(1));
    ((u64::from(clamped) * u64::from(target_bound)) / u64::from(source_bound))
        .min(u64::from(target_bound.saturating_sub(1))) as u32
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

fn midpoint(start: u32, end: u32) -> i32 {
    start.saturating_add(end.saturating_sub(start) / 2) as i32
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reject_invalid_pointer_space() {
        assert_eq!(PointerSpace::new(0, 48), Err(PointerStateError::InvalidSpace));
        assert_eq!(
            AbsoluteAxisCalibration::new(10, 10),
            Err(PointerStateError::InvalidCalibration)
        );
    }

    #[test]
    fn relative_pointer_state_clamps_inside_display_space() {
        let space = PointerSpace::new(1280, 800).expect("space");
        let mut state = PointerState::new(space, PointerPosition::new(10, 10)).expect("state");

        assert_eq!(state.apply_relative(-100, 30), PointerPosition::new(0, 40));
        assert_eq!(state.apply_relative(5000, 5000), PointerPosition::new(1279, 799));
    }

    #[test]
    fn transform_maps_display_and_route_positions() {
        let display = PointerSpace::new(1280, 800).expect("display");
        let route = PointerSpace::new(64, 48).expect("route");
        let transform = PointerTransform::new(display, route).expect("transform");

        assert_eq!(
            transform.display_to_route(PointerPosition::new(490, 208)),
            PointerPosition::new(24, 12)
        );
        assert_eq!(
            transform.route_to_display(PointerPosition::new(8, 40)),
            PointerPosition::new(170, 675)
        );
        assert_eq!(
            transform.route_rect_to_display(4, 36, 8, 8),
            DisplayRect { left: 80, top: 600, right: 240, bottom: 734 }
        );
        assert_eq!(transform.display_extent_from_route(), PointerExtent { width: 20, height: 17 });
    }

    #[test]
    fn absolute_axis_scales_across_display_space() {
        let display = PointerSpace::new(1280, 800).expect("display");
        let route = PointerSpace::new(64, 48).expect("route");
        let transform = PointerTransform::new(display, route).expect("transform");
        let calibration = AbsoluteAxisCalibration::new(0, 32_767).expect("calibration");

        assert_eq!(transform.scale_absolute_axis(0, calibration, PointerAxis::X), 0);
        assert_eq!(transform.scale_absolute_axis(32_767, calibration, PointerAxis::X), 1279);
        assert_eq!(transform.scale_absolute_axis(16_384, calibration, PointerAxis::Y), 399);
    }
}
