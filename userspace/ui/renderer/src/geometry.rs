// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Integer point/rectangle geometry for clipped renderer operations.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 3 renderer integration tests, 24 ui_host_snap contract tests
//! ADR: docs/rfcs/RFC-0046-ui-v1a-host-cpu-renderer-snapshots-contract.md

use crate::error::{RenderError, RenderResult};
use crate::math::checked_i32_extent;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    #[must_use]
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub fn new(x: i32, y: i32, width: u32, height: u32) -> RenderResult<Self> {
        if width == 0 || height == 0 {
            return Err(RenderError::InvalidRect);
        }
        let right = checked_i32_extent(x, width)?;
        let bottom = checked_i32_extent(y, height)?;
        if right <= i64::from(i32::MIN) || bottom <= i64::from(i32::MIN) {
            return Err(RenderError::InvalidRect);
        }
        Ok(Self {
            x,
            y,
            width,
            height,
        })
    }

    #[must_use]
    pub fn right(self) -> i64 {
        i64::from(self.x) + i64::from(self.width)
    }

    #[must_use]
    pub fn bottom(self) -> i64 {
        i64::from(self.y) + i64::from(self.height)
    }

    pub fn union(self, other: Self) -> RenderResult<Self> {
        let left = i64::from(self.x).min(i64::from(other.x));
        let top = i64::from(self.y).min(i64::from(other.y));
        let right = self.right().max(other.right());
        let bottom = self.bottom().max(other.bottom());
        if left < i64::from(i32::MIN)
            || top < i64::from(i32::MIN)
            || left > i64::from(i32::MAX)
            || top > i64::from(i32::MAX)
        {
            return Err(RenderError::InvalidRect);
        }
        let width = u32::try_from(right - left).map_err(|_| RenderError::InvalidRect)?;
        let height = u32::try_from(bottom - top).map_err(|_| RenderError::InvalidRect)?;
        Rect::new(left as i32, top as i32, width, height)
    }

    #[must_use]
    pub fn intersects_or_touches(self, other: Self) -> bool {
        i64::from(self.x) <= other.right()
            && i64::from(other.x) <= self.right()
            && i64::from(self.y) <= other.bottom()
            && i64::from(other.y) <= self.bottom()
    }

    #[must_use]
    pub fn clip_to(self, bounds: Self) -> Option<Self> {
        let left = i64::from(self.x).max(i64::from(bounds.x));
        let top = i64::from(self.y).max(i64::from(bounds.y));
        let right = self.right().min(bounds.right());
        let bottom = self.bottom().min(bounds.bottom());
        if right <= left || bottom <= top {
            return None;
        }
        let width = u32::try_from(right - left).ok()?;
        let height = u32::try_from(bottom - top).ok()?;
        Rect::new(left as i32, top as i32, width, height).ok()
    }
}
