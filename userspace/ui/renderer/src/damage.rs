// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Bounded damage tracking for deterministic host renderer snapshots.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 3 renderer integration tests, 24 ui_host_snap contract tests
//! ADR: docs/rfcs/RFC-0046-ui-v1a-host-cpu-renderer-snapshots-contract.md

use crate::error::RenderResult;
use crate::geometry::Rect;
use crate::units::{DamageRectCount, SurfaceHeight, SurfaceWidth};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Damage {
    frame_bounds: Rect,
    max_count: DamageRectCount,
    rects: Vec<Rect>,
}

impl Damage {
    pub fn for_frame(
        width: SurfaceWidth,
        height: SurfaceHeight,
        max_count: DamageRectCount,
    ) -> RenderResult<Self> {
        Ok(Self {
            frame_bounds: Rect::new(0, 0, width.get(), height.get())?,
            max_count,
            rects: Vec::new(),
        })
    }

    pub fn add(&mut self, rect: Rect) -> RenderResult<()> {
        let Some(clipped) = rect.clip_to(self.frame_bounds) else {
            return Ok(());
        };
        self.rects.push(clipped);
        self.coalesce()?;
        if self.rects.len() > usize::from(self.max_count.get()) {
            self.rects.clear();
            self.rects.push(self.frame_bounds);
        }
        Ok(())
    }

    #[must_use]
    pub fn rects(&self) -> &[Rect] {
        &self.rects
    }

    #[must_use]
    pub const fn frame_bounds(&self) -> Rect {
        self.frame_bounds
    }

    fn coalesce(&mut self) -> RenderResult<()> {
        let mut changed = true;
        while changed {
            changed = false;
            'outer: for left in 0..self.rects.len() {
                for right in (left + 1)..self.rects.len() {
                    if self.rects[left].intersects_or_touches(self.rects[right]) {
                        let merged = self.rects[left].union(self.rects[right])?;
                        self.rects[left] = merged;
                        self.rects.remove(right);
                        changed = true;
                        break 'outer;
                    }
                }
            }
        }
        Ok(())
    }
}
