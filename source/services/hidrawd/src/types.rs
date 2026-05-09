// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Typed `hidrawd` service-level IDs and event batches for TASK-0253.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `cargo test -p hidrawd -- --nocapture`
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

extern crate alloc;

use alloc::vec::Vec;
use hid::HidEvent;
use crate::PointerSource;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceId(u16);

impl DeviceId {
    #[must_use]
    pub const fn new(raw: u16) -> Self {
        Self(raw)
    }

    #[must_use]
    pub const fn raw(self) -> u16 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HidDeviceKind {
    Keyboard,
    Mouse,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HidBatch {
    device: DeviceId,
    kind: HidDeviceKind,
    pointer_source: Option<PointerSource>,
    events: Vec<HidEvent>,
}

impl HidBatch {
    #[must_use]
    pub fn new(device: DeviceId, kind: HidDeviceKind, events: Vec<HidEvent>) -> Self {
        Self {
            device,
            kind,
            pointer_source: None,
            events,
        }
    }

    #[must_use]
    pub fn new_pointer(device: DeviceId, source: PointerSource, events: Vec<HidEvent>) -> Self {
        Self {
            device,
            kind: HidDeviceKind::Mouse,
            pointer_source: Some(source),
            events,
        }
    }

    #[must_use]
    pub const fn device(&self) -> DeviceId {
        self.device
    }

    #[must_use]
    pub const fn kind(&self) -> HidDeviceKind {
        self.kind
    }

    #[must_use]
    pub const fn pointer_source(&self) -> Option<PointerSource> {
        self.pointer_source
    }

    #[must_use]
    pub fn events(&self) -> &[HidEvent] {
        self.events.as_slice()
    }
}
