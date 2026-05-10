// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Deterministic touch lifecycle normalizer for bounded host samples.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: No direct tests (covered by 4 integration tests in `tests/input_v1_0_host/tests/touch_contract.rs`).
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

use crate::{RawTouchSample, TouchBounds, TouchError, TouchEvent, TouchPhase, TouchX, TouchY};

#[derive(Debug, Clone)]
pub struct TouchNormalizer {
    bounds: TouchBounds,
    active: bool,
}

impl TouchNormalizer {
    #[must_use]
    pub const fn new(bounds: TouchBounds) -> Self {
        Self { bounds, active: false }
    }

    pub fn normalize(&mut self, sample: RawTouchSample) -> Result<TouchEvent, TouchError> {
        if sample.x() >= self.bounds.width() || sample.y() >= self.bounds.height() {
            return Err(TouchError::OutOfBounds { x: sample.x(), y: sample.y() });
        }

        match sample.phase() {
            TouchPhase::Down if self.active => return Err(TouchError::DuplicateDown),
            TouchPhase::Move | TouchPhase::Up if !self.active => {
                return Err(TouchError::MissingDown)
            }
            TouchPhase::Down => self.active = true,
            TouchPhase::Up => self.active = false,
            TouchPhase::Move => {}
        }

        Ok(TouchEvent::new(
            sample.timestamp(),
            TouchX::new(sample.x()),
            TouchY::new(sample.y()),
            sample.phase(),
        ))
    }
}
