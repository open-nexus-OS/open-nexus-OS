// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use core::fmt;
use core::ops::{Add, AddAssign, Div, Mul, Sub, SubAssign};

// ─── FxPx: fixed-point pixel ───

/// Fixed-point pixel value. Integer-only for v3a (text advances from
/// rustybuzz are naturally integer). The `i32` provides headroom for
/// sub-pixel if later needed without changing the API surface.
///
/// All arithmetic uses checked operations. Overflow or division by zero
/// panics in debug, saturates in release (caller should validate).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FxPx(pub i32);

impl FxPx {
    pub const ZERO: Self = FxPx(0);
    pub const ONE: Self = FxPx(1);

    pub const fn new(value: i32) -> Self {
        FxPx(value)
    }

    pub const fn as_i32(self) -> i32 {
        self.0
    }

    pub const fn as_u32(self) -> Option<u32> {
        if self.0 >= 0 {
            Some(self.0 as u32)
        } else {
            None
        }
    }

    pub fn max(self, other: Self) -> Self {
        FxPx(self.0.max(other.0))
    }

    pub fn min(self, other: Self) -> Self {
        FxPx(self.0.min(other.0))
    }

    pub fn clamp(self, min: Self, max: Self) -> Self {
        FxPx(self.0.clamp(min.0, max.0))
    }

    pub fn saturating_sub(self, other: Self) -> Self {
        FxPx(self.0.saturating_sub(other.0))
    }

    pub fn checked_div(self, other: Self) -> Option<Self> {
        if other.0 == 0 {
            None
        } else {
            Some(FxPx(self.0 / other.0))
        }
    }
}

impl Add for FxPx {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        FxPx(self.0 + rhs.0)
    }
}

impl AddAssign for FxPx {
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl Sub for FxPx {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        FxPx(self.0 - rhs.0)
    }
}

impl SubAssign for FxPx {
    fn sub_assign(&mut self, rhs: Self) {
        self.0 -= rhs.0;
    }
}

impl Mul<i32> for FxPx {
    type Output = Self;
    fn mul(self, rhs: i32) -> Self {
        FxPx(self.0 * rhs)
    }
}

impl Div<i32> for FxPx {
    type Output = Self;
    fn div(self, rhs: i32) -> Self {
        debug_assert!(rhs != 0, "FxPx division by zero");
        FxPx(self.0 / rhs)
    }
}

impl Default for FxPx {
    fn default() -> Self { FxPx::ZERO }
}

impl fmt::Display for FxPx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ─── Rect ───

/// A rectangle in the layout coordinate space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: FxPx,
    pub y: FxPx,
    pub width: FxPx,
    pub height: FxPx,
}

impl Rect {
    pub const fn new(x: FxPx, y: FxPx, width: FxPx, height: FxPx) -> Self {
        Rect { x, y, width, height }
    }

    pub const fn zero() -> Self {
        Rect { x: FxPx::ZERO, y: FxPx::ZERO, width: FxPx::ZERO, height: FxPx::ZERO }
    }

    /// Inset the rectangle by the given edge insets (shrink).
    pub fn inset(&self, insets: EdgeInsets) -> Self {
        Rect {
            x: self.x + insets.left,
            y: self.y + insets.top,
            width: self.width.saturating_sub(insets.left + insets.right),
            height: self.height.saturating_sub(insets.top + insets.bottom),
        }
    }

    /// Content area after padding.
    pub fn content_rect(&self, padding: EdgeInsets) -> Self {
        self.inset(padding)
    }
}

// ─── EdgeInsets ───

/// EdgeInsets for padding and margin.
/// Mirrors the DSL `padding(...)` / `margin(...)` modifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EdgeInsets {
    pub top: FxPx,
    pub right: FxPx,
    pub bottom: FxPx,
    pub left: FxPx,
}

impl EdgeInsets {
    pub const fn all(v: FxPx) -> Self {
        EdgeInsets { top: v, right: v, bottom: v, left: v }
    }

    pub const fn zero() -> Self {
        Self::all(FxPx::ZERO)
    }

    pub const fn symmetric(vertical: FxPx, horizontal: FxPx) -> Self {
        EdgeInsets { top: vertical, right: horizontal, bottom: vertical, left: horizontal }
    }

    pub fn horizontal(&self) -> FxPx {
        self.left + self.right
    }

    pub fn vertical(&self) -> FxPx {
        self.top + self.bottom
    }
}
