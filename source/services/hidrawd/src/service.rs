// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Bounded `hidrawd` service seam for keyboard/mouse report ingest.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `cargo test -p hidrawd -- --nocapture`
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

extern crate alloc;

use crate::ingest::{parse_keyboard, parse_mouse};
use crate::{DeviceId, HidBatch, HidDeviceKind, HidrawdError, PointerSource};
use alloc::vec::Vec;
use hid::{BootKeyboardParser, BootMouseParser, HidEvent, TimestampNs};

const MAX_LOGGED_BATCHES: usize = 32;

#[derive(Debug, Default, Clone)]
pub struct HidrawdService {
    keyboard: Option<KeyboardSource>,
    mouse: Option<MouseSource>,
    recent_batches: Vec<HidBatch>,
}

#[derive(Debug, Clone)]
struct KeyboardSource {
    device_id: DeviceId,
    parser: BootKeyboardParser,
}

#[derive(Debug, Clone)]
struct MouseSource {
    device_id: DeviceId,
    parser: BootMouseParser,
}

impl HidrawdService {
    #[must_use]
    pub fn new() -> Self {
        Self {
            keyboard: None,
            mouse: None,
            recent_batches: Vec::new(),
        }
    }

    pub fn register_keyboard(&mut self, device_id: DeviceId) {
        self.keyboard = Some(KeyboardSource {
            device_id,
            parser: BootKeyboardParser::new(),
        });
    }

    pub fn register_mouse(&mut self, device_id: DeviceId) {
        self.mouse = Some(MouseSource {
            device_id,
            parser: BootMouseParser::new(),
        });
    }

    #[must_use]
    pub const fn keyboard_ready(&self) -> bool {
        self.keyboard.is_some()
    }

    #[must_use]
    pub const fn mouse_ready(&self) -> bool {
        self.mouse.is_some()
    }

    pub fn ingest_keyboard_report(
        &mut self,
        device_id: DeviceId,
        timestamp: TimestampNs,
        report: &[u8],
    ) -> Result<HidBatch, HidrawdError> {
        let keyboard = self
            .keyboard
            .as_mut()
            .ok_or(HidrawdError::KeyboardUnavailable)?;
        if keyboard.device_id != device_id {
            return Err(HidrawdError::UnexpectedDevice {
                expected: HidDeviceKind::Keyboard,
                actual: HidDeviceKind::Mouse,
            });
        }
        let batch = HidBatch::new(
            device_id,
            HidDeviceKind::Keyboard,
            parse_keyboard(&mut keyboard.parser, timestamp, report)?,
        );
        self.push_batch(batch.clone());
        Ok(batch)
    }

    pub fn ingest_mouse_report(
        &mut self,
        device_id: DeviceId,
        timestamp: TimestampNs,
        report: &[u8],
    ) -> Result<HidBatch, HidrawdError> {
        let mouse = self.mouse.as_mut().ok_or(HidrawdError::MouseUnavailable)?;
        if mouse.device_id != device_id {
            return Err(HidrawdError::UnexpectedDevice {
                expected: HidDeviceKind::Mouse,
                actual: HidDeviceKind::Keyboard,
            });
        }
        let batch = HidBatch::new_pointer(
            device_id,
            PointerSource::MouseRelative,
            parse_mouse(&mut mouse.parser, timestamp, report)?,
        );
        self.push_batch(batch.clone());
        Ok(batch)
    }

    pub fn ingest_device_events(
        &mut self,
        device_id: DeviceId,
        kind: HidDeviceKind,
        events: Vec<HidEvent>,
    ) -> Result<HidBatch, HidrawdError> {
        match kind {
            HidDeviceKind::Keyboard => {
                let keyboard = self
                    .keyboard
                    .as_ref()
                    .ok_or(HidrawdError::KeyboardUnavailable)?;
                if keyboard.device_id != device_id {
                    return Err(HidrawdError::UnexpectedDevice {
                        expected: HidDeviceKind::Keyboard,
                        actual: HidDeviceKind::Mouse,
                    });
                }
            }
            HidDeviceKind::Mouse => {
                let mouse = self.mouse.as_ref().ok_or(HidrawdError::MouseUnavailable)?;
                if mouse.device_id != device_id {
                    return Err(HidrawdError::UnexpectedDevice {
                        expected: HidDeviceKind::Mouse,
                        actual: HidDeviceKind::Keyboard,
                    });
                }
            }
        }
        let batch = HidBatch::new(device_id, kind, events);
        self.push_batch(batch.clone());
        Ok(batch)
    }

    #[must_use]
    pub fn recent_batches(&self) -> &[HidBatch] {
        self.recent_batches.as_slice()
    }

    fn push_batch(&mut self, batch: HidBatch) {
        if self.recent_batches.len() == MAX_LOGGED_BATCHES {
            self.recent_batches.remove(0);
        }
        self.recent_batches.push(batch);
    }
}
