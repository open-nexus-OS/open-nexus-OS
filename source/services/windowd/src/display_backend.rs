// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Narrow display handoff objects used by `fbdevd` so scanout stays service-owned.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered via `fbdevd` host tests and visible-bootstrap QEMU proofs.
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

use input_live_protocol::VisibleState;

use crate::error::Result;
use crate::frame::Frame;
use crate::smoke::{run_visible_systemui_smoke, VisibleBootstrapMode, VisibleSystemUiEvidence};
use crate::visible_state::{compose_live_visible_frame, copy_live_visible_row};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisplayFrameSource {
    Materialized(Frame),
    Bootstrap(VisibleSystemUiEvidence),
    Live(VisibleState),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayPresentHandoff {
    pub mode: VisibleBootstrapMode,
    pub source: DisplayFrameSource,
    pub damage_rects: u16,
    pub backend_visible: bool,
    pub systemui_first_frame_visible: bool,
    pub scanout_ready: bool,
}

pub fn bootstrap_display_handoff() -> Result<DisplayPresentHandoff> {
    let evidence = run_visible_systemui_smoke()?;
    let damage_rects = evidence.first_present.damage_rects;
    let backend_visible = evidence.backend_visible;
    let systemui_first_frame_visible = evidence.systemui_first_frame;
    let scanout_ready = evidence.ready && evidence.backend_visible;
    Ok(DisplayPresentHandoff {
        mode: evidence.mode,
        source: DisplayFrameSource::Bootstrap(evidence),
        damage_rects,
        backend_visible,
        systemui_first_frame_visible,
        scanout_ready,
    })
}

pub fn live_visible_state_handoff(state: VisibleState) -> Result<DisplayPresentHandoff> {
    let mode = VisibleBootstrapMode::fixed()?.validate()?;
    Ok(DisplayPresentHandoff {
        mode,
        source: DisplayFrameSource::Live(state),
        damage_rects: 1,
        backend_visible: state.backend_visible,
        systemui_first_frame_visible: state.systemui_first_frame_visible,
        scanout_ready: state.display_scanout_ready,
    })
}

impl DisplayPresentHandoff {
    pub fn byte_len(&self) -> Result<usize> {
        match &self.source {
            DisplayFrameSource::Materialized(frame) => Ok(frame.pixels.len()),
            DisplayFrameSource::Bootstrap(_) | DisplayFrameSource::Live(_) => self.mode.byte_len(),
        }
    }

    pub fn copy_row(&self, y: u32, row: &mut [u8]) -> Result<()> {
        match &self.source {
            DisplayFrameSource::Materialized(frame) => {
                let row_len = self.mode.stride as usize;
                if row.len() < row_len {
                    return Err(crate::WindowdError::BufferLengthMismatch);
                }
                if frame.width != self.mode.width
                    || frame.height != self.mode.height
                    || frame.stride != self.mode.stride
                {
                    return Err(crate::WindowdError::InvalidDimensions);
                }
                let src = y as usize * row_len;
                row[..row_len].copy_from_slice(
                    frame
                        .pixels
                        .get(src..src + row_len)
                        .ok_or(crate::WindowdError::BufferLengthMismatch)?,
                );
                Ok(())
            }
            DisplayFrameSource::Bootstrap(evidence) => evidence.copy_composed_row(y, row),
            DisplayFrameSource::Live(state) => copy_live_visible_row(*state, self.mode, y, row),
        }
    }

    pub fn materialize_frame(&self) -> Result<Frame> {
        match &self.source {
            DisplayFrameSource::Materialized(frame) => Ok(frame.clone()),
            DisplayFrameSource::Bootstrap(evidence) => materialize_bootstrap_frame(evidence),
            DisplayFrameSource::Live(state) => compose_live_visible_frame(*state, self.mode),
        }
    }
}

fn materialize_bootstrap_frame(evidence: &VisibleSystemUiEvidence) -> Result<Frame> {
    let mut frame = Frame {
        width: evidence.mode.width,
        height: evidence.mode.height,
        stride: evidence.mode.stride,
        pixels: alloc::vec![0u8; evidence.mode.byte_len()?],
    };
    let row_len = evidence.mode.stride as usize;
    let mut row = alloc::vec![0u8; row_len];
    for y in 0..evidence.mode.height {
        evidence.copy_composed_row(y, &mut row)?;
        let offset = y as usize * row_len;
        frame.pixels[offset..offset + row_len].copy_from_slice(&row);
    }
    Ok(frame)
}
