// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Shared data types for the windowd compositor runtime (RenderClip, ProofBoxRect,
//! ProofCard/PaintRole system, SourceFrame, FixedDebugLine).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered via compositor integration tests

pub(crate) struct FixedDebugLine {
    pub(crate) buf: [u8; 256],
    pub(crate) len: usize,
}

impl FixedDebugLine {
    pub(crate) const fn new() -> Self {
        Self { buf: [0; 256], len: 0 }
    }

    pub(crate) fn as_str(&self) -> Option<&str> {
        core::str::from_utf8(&self.buf[..self.len]).ok()
    }
}

impl core::fmt::Write for FixedDebugLine {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let end = self.len.saturating_add(s.len());
        if end > self.buf.len() {
            return Err(core::fmt::Error);
        }
        self.buf[self.len..end].copy_from_slice(s.as_bytes());
        self.len = end;
        Ok(())
    }
}

#[derive(Clone, Copy)]
pub(crate) struct RenderClip {
    pub(crate) start_x: u32,
    pub(crate) end_x: u32,
}

impl RenderClip {
    pub(crate) const fn full(width: u32) -> Self {
        Self { start_x: 0, end_x: width }
    }

    pub(crate) fn new(start_x: u32, end_x: u32, width: u32) -> Self {
        Self { start_x: start_x.min(width), end_x: end_x.min(width) }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct SourceFrame {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) stride: u32,
    /// Raw BGRA rows — or ROW-RLE data when `rows` is `Some` (per-row runs of
    /// `[len:u16 LE][b g r a]`, bounded by `rows[y]..rows[y+1]`). RLE keeps
    /// BOTH theme wallpapers full-resolution inside the image budget (raw
    /// 2×4MB overflowed the RAM region); rows decode into a stack buffer at
    /// copy time — no heap, no quality loss.
    pub(crate) pixels: &'static [u8],
    pub(crate) rows: Option<&'static [u32]>,
}
