// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Bounded `touchd` service seam for normalized touch ingest and proof fixtures.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 4 host contract tests in the `touchd` crate.
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

extern crate alloc;

use crate::ingest::{normalize_sample, synthetic_fixture};
use crate::{SyntheticTouchMode, TouchDeviceId, TouchdError};
use alloc::vec::Vec;
use touch::{RawTouchSample, TouchBounds, TouchEvent, TouchNormalizer};

const SYNTHETIC_X0: u32 = 20;
const SYNTHETIC_Y0: u32 = 20;
const SYNTHETIC_X1: u32 = 28;
const SYNTHETIC_Y1: u32 = 22;
const MAX_LOGGED_EVENTS: usize = 32;

#[derive(Debug, Clone)]
pub struct TouchdService {
    device: Option<TouchDeviceId>,
    normalizer: TouchNormalizer,
    synthetic_mode: SyntheticTouchMode,
    recent_events: Vec<TouchEvent>,
}

impl TouchdService {
    #[must_use]
    pub fn new(bounds: TouchBounds, synthetic_mode: SyntheticTouchMode) -> Self {
        Self {
            device: None,
            normalizer: TouchNormalizer::new(bounds),
            synthetic_mode,
            recent_events: Vec::new(),
        }
    }

    pub fn register_device(&mut self, device_id: TouchDeviceId) {
        self.device = Some(device_id);
    }

    #[must_use]
    pub const fn ready(&self) -> bool {
        self.device.is_some()
    }

    pub fn ingest(&mut self, sample: RawTouchSample) -> Result<TouchEvent, TouchdError> {
        if self.device.is_none() {
            return Err(TouchdError::DeviceUnavailable);
        }
        let event = normalize_sample(&mut self.normalizer, sample)?;
        self.push_event(event);
        Ok(event)
    }

    pub fn synthetic_sequence(&mut self, start_ns: u64) -> Result<Vec<TouchEvent>, TouchdError> {
        if self.synthetic_mode != SyntheticTouchMode::ProofFixture {
            return Err(TouchdError::SyntheticModeDisabled);
        }
        let mut events = Vec::new();
        for sample in synthetic_fixture(start_ns, SYNTHETIC_X0, SYNTHETIC_Y0, SYNTHETIC_X1, SYNTHETIC_Y1) {
            events.push(self.ingest(sample)?);
        }
        Ok(events)
    }

    #[must_use]
    pub fn recent_events(&self) -> &[TouchEvent] {
        self.recent_events.as_slice()
    }

    fn push_event(&mut self, event: TouchEvent) {
        if self.recent_events.len() == MAX_LOGGED_EVENTS {
            self.recent_events.remove(0);
        }
        self.recent_events.push(event);
    }
}
