// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

/// Layout direction for Stack containers.
/// DSL uses `Stack(direction: column)` — not separate VStack/HStack types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    Column,
    Row,
}

impl Direction {
    /// Returns true if the main axis is vertical.
    pub fn is_vertical(self) -> bool {
        matches!(self, Direction::Column)
    }

    /// Returns true if the main axis is horizontal.
    pub fn is_horizontal(self) -> bool {
        matches!(self, Direction::Row)
    }

    /// Cross-axis: the perpendicular axis.
    pub fn cross_axis(self) -> Self {
        match self {
            Direction::Column => Direction::Row,
            Direction::Row => Direction::Column,
        }
    }
}

// ─── Alignment ───

/// Cross-axis alignment (Tailwind: `items-*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Align {
    Start,
    Center,
    End,
    Stretch,
}

/// Main-axis justification (Tailwind: `justify-*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Justify {
    Start,
    Center,
    End,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

// ─── Overflow ───

/// Overflow behavior for content exceeding container bounds.
/// v3a defaults to `Visible`; v3b uses `Hidden` for scissor clipping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Overflow {
    Visible,
    /// Scissor-clip only: the container keeps its content-derived size
    /// (rounded cards etc.) — nothing scrolls.
    Hidden,
    /// A SCROLL VIEWPORT: clipped like `Hidden`, but the container measures
    /// collapsed on its main axis (flex `grow` gives it its size — the CSS
    /// `min-height: 0` rule), children lay out UNBOUNDED (content overflows
    /// the clip; that overflow is the scrollable extent) and are never
    /// flex-shrunk to fit. The HOST owns the paint-time scroll offset along
    /// the DECLARED axis (`.scroll(vertical|horizontal)` — never guessed
    /// from content shape).
    Scroll(ScrollAxis),
}

/// The declared axis of a `.scroll(...)` viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScrollAxis {
    Vertical,
    Horizontal,
}

// ─── Position ───

/// Child positioning inside a Stack.
/// `Relative` = normal flow. `Absolute` = removed from flow, positioned
/// relative to the nearest `Relative` ancestor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Position {
    Relative,
    Absolute,
}

// ─── ZIndex ───

/// Stacking order for overlapping elements.
/// Higher values paint on top. Tie-breaking follows tree order.
pub type ZIndex = i16;
