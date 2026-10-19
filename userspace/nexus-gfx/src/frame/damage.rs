// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! What a frame changed on the screen (TASK-0215 5c-3 P5, RFC-0101 §8: damage per plane).
//!
//! WHY: a present's scene is the whole layer set, but the app-host's tree and rasterizer already
//! say what changed — each surface present names its damage, windowd turns it into screen
//! rectangles (`Command::Damage`). A display that keeps its planes redraws into a plane only what
//! changed since that plane was last drawn (buffer age), and re-blurs a glass only when what lies
//! within its reach changed.
//!
//! MODEL: an [`Area`] is nothing, one rectangle (the bounds of what changed: a pass is limited to
//! one rectangle) or the whole screen; [`History`] keeps the last frames' areas, so a plane drawn
//! `n` frames ago is brought up to date by their union — the whole screen when the plane is older
//! than the history, or of unknown age. Allocation-free.

/// A screen area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Area {
    None,
    /// `x0..x1` × `y0..y1` (end exclusive), never empty.
    Rect {
        x0: u32,
        y0: u32,
        x1: u32,
        y1: u32,
    },
    Full,
}

impl Area {
    /// `w` × `h` at (`x`, `y`): nothing when empty; the end saturates at the coordinate space's.
    pub fn rect(x: u32, y: u32, w: u32, h: u32) -> Self {
        let (x1, y1) = (x.saturating_add(w), y.saturating_add(h));
        if x1 <= x || y1 <= y {
            Self::None
        } else {
            Self::Rect { x0: x, y0: y, x1, y1 }
        }
    }

    /// `rect` grown by `by` on every side (clamped at 0 and at the coordinate space's end).
    pub fn grown(self, by: u32) -> Self {
        match self {
            Self::Rect { x0, y0, x1, y1 } => Self::Rect {
                x0: x0.saturating_sub(by),
                y0: y0.saturating_sub(by),
                x1: x1.saturating_add(by),
                y1: y1.saturating_add(by),
            },
            other => other,
        }
    }

    /// Both areas' bounds.
    pub fn union(self, other: Self) -> Self {
        match (self, other) {
            (Self::Full, _) | (_, Self::Full) => Self::Full,
            (Self::None, a) | (a, Self::None) => a,
            (
                Self::Rect { x0: a0, y0: b0, x1: a1, y1: b1 },
                Self::Rect { x0: c0, y0: d0, x1: c1, y1: d1 },
            ) => Self::Rect { x0: a0.min(c0), y0: b0.min(d0), x1: a1.max(c1), y1: b1.max(d1) },
        }
    }

    /// Whether every pixel of the area lies within `other`.
    pub fn inside(self, other: Self) -> bool {
        match (self, other) {
            (Self::None, _) | (_, Self::Full) => true,
            (Self::Full, _) | (_, Self::None) => false,
            (
                Self::Rect { x0: a0, y0: b0, x1: a1, y1: b1 },
                Self::Rect { x0: c0, y0: d0, x1: c1, y1: d1 },
            ) => c0 <= a0 && a1 <= c1 && d0 <= b0 && b1 <= d1,
        }
    }

    /// Whether the area shares a pixel with `other`.
    pub fn touches(self, other: Self) -> bool {
        match (self, other) {
            (Self::None, _) | (_, Self::None) => false,
            (Self::Full, _) | (_, Self::Full) => true,
            (
                Self::Rect { x0: a0, y0: b0, x1: a1, y1: b1 },
                Self::Rect { x0: c0, y0: d0, x1: c1, y1: d1 },
            ) => a0 < c1 && c0 < a1 && b0 < d1 && d0 < b1,
        }
    }

    /// The area on a display of `display` pixels, as a pass names it (inclusive corners);
    /// `None` when nothing of it is on the display.
    pub fn on(self, (w, h): (u32, u32)) -> Option<(u32, u32, u32, u32)> {
        let (x0, y0, x1, y1) = match self {
            Self::None => return None,
            Self::Full => (0, 0, w, h),
            Self::Rect { x0, y0, x1, y1 } => (x0, y0, x1.min(w), y1.min(h)),
        };
        (x0 < x1 && y0 < y1).then(|| (x0, y0, x1 - 1, y1 - 1))
    }

    /// The pixels of the area on a display of `display` pixels.
    pub fn pixels(self, display: (u32, u32)) -> u64 {
        self.on(display)
            .map_or(0, |(x0, y0, x1, y1)| u64::from(x1 - x0 + 1) * u64::from(y1 - y0 + 1))
    }
}

/// Frames a [`History`] remembers: a ring of planes shows a plane again after a few frames.
pub const HISTORY: usize = 8;

/// The last frames' damage, for a plane shown frames ago (buffer age).
pub struct History {
    areas: [Area; HISTORY],
    /// Frames recorded so far: the newest is frame `frames` (the first is 1).
    frames: u64,
}

impl History {
    pub const fn new() -> Self {
        Self { areas: [Area::None; HISTORY], frames: 0 }
    }

    /// Records the next frame's damage; its number.
    pub fn record(&mut self, area: Area) -> u64 {
        self.frames += 1;
        self.areas[(self.frames % HISTORY as u64) as usize] = area;
        self.frames
    }

    /// What a plane last drawn at frame `drawn` lacks now: every later frame's damage — the
    /// whole screen when `drawn` is 0 (never drawn), newer than the newest frame, or older than
    /// the history holds.
    pub fn since(&self, drawn: u64) -> Area {
        if drawn == 0 || drawn > self.frames || self.frames - drawn > HISTORY as u64 {
            return Area::Full;
        }
        ((drawn + 1)..=self.frames)
            .map(|f| self.areas[(f % HISTORY as u64) as usize])
            .fold(Area::None, Area::union)
    }

    /// The newest frame's number (0 before the first).
    pub const fn newest(&self) -> u64 {
        self.frames
    }
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}
