// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: hidrawd's device table — the ONE owner of the boot-protocol parsers (TASK-0253B).
//! Every boot keyboard and mouse a source registers (a USB HID interface xhcid attached; a host
//! test's device) gets its parser here; its reports become events here (allocation-free with
//! [`HidrawdService::ingest_report_into`]); and when it goes away its release — key-ups and
//! button-ups for whatever it held — comes from here, so nothing stays pressed. Bounded:
//! [`MAX_DEVICES`] at once, a refused registration says so.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `cargo test -p hidrawd -- --nocapture`
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

extern crate alloc;

use crate::{DeviceId, HidBatch, HidDeviceKind, HidrawdError, PointerSource};
use alloc::vec::Vec;
use hid::{BootKeyboardParser, BootMouseParser, HidEvent, TimestampNs};

const MAX_LOGGED_BATCHES: usize = 32;
/// Devices registered at once at most: every USB HID interface xhcid can attach (16) and room.
pub const MAX_DEVICES: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveRouteSendErrorClass {
    Backpressure,
    Disconnected,
    Fatal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveRouteSendAction {
    DropBatch,
    ResetRoute,
}

pub const fn classify_live_route_send_error(class: LiveRouteSendErrorClass) -> LiveRouteSendAction {
    match class {
        LiveRouteSendErrorClass::Backpressure => LiveRouteSendAction::DropBatch,
        LiveRouteSendErrorClass::Disconnected | LiveRouteSendErrorClass::Fatal => {
            LiveRouteSendAction::ResetRoute
        }
    }
}

/// A registered device's parser — its kind with it.
#[derive(Debug, Clone)]
enum Parser {
    Keyboard(BootKeyboardParser),
    Mouse(BootMouseParser),
}

impl Parser {
    const fn kind(&self) -> HidDeviceKind {
        match self {
            Self::Keyboard(_) => HidDeviceKind::Keyboard,
            Self::Mouse(_) => HidDeviceKind::Mouse,
        }
    }
}

#[derive(Debug, Clone)]
struct Registered {
    device: DeviceId,
    parser: Parser,
}

/// The device table and, for host diagnostics, the batches its convenience paths made.
#[derive(Debug, Default, Clone)]
pub struct HidrawdService {
    devices: [Option<Registered>; MAX_DEVICES],
    recent_batches: Vec<HidBatch>,
}

impl HidrawdService {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a boot keyboard as `device` (a second registration starts it over). `false`:
    /// the table is full and the device is refused.
    pub fn register_keyboard(&mut self, device: DeviceId) -> bool {
        self.register(device, Parser::Keyboard(BootKeyboardParser::new()))
    }

    /// Registers a boot mouse as `device` (a second registration starts it over). `false`: the
    /// table is full and the device is refused.
    pub fn register_mouse(&mut self, device: DeviceId) -> bool {
        self.register(device, Parser::Mouse(BootMouseParser::new()))
    }

    fn register(&mut self, device: DeviceId, parser: Parser) -> bool {
        if let Some(known) = self.devices.iter_mut().flatten().find(|r| r.device == device) {
            known.parser = parser;
            return true;
        }
        match self.devices.iter_mut().find(|r| r.is_none()) {
            Some(free) => {
                *free = Some(Registered { device, parser });
                true
            }
            None => false,
        }
    }

    /// What `device` is, if registered.
    #[must_use]
    pub fn kind_of(&self, device: DeviceId) -> Option<HidDeviceKind> {
        self.find(device).map(|r| r.parser.kind())
    }

    #[must_use]
    pub fn keyboard_ready(&self) -> bool {
        self.devices.iter().flatten().any(|r| r.parser.kind() == HidDeviceKind::Keyboard)
    }

    #[must_use]
    pub fn mouse_ready(&self) -> bool {
        self.devices.iter().flatten().any(|r| r.parser.kind() == HidDeviceKind::Mouse)
    }

    /// Forgets `device` and releases what it held: the key-ups and button-ups are appended to
    /// `out`. `None`: it was not registered.
    pub fn release_into(
        &mut self,
        device: DeviceId,
        timestamp: TimestampNs,
        out: &mut Vec<HidEvent>,
    ) -> Option<HidDeviceKind> {
        let slot =
            self.devices.iter().position(|r| r.as_ref().is_some_and(|r| r.device == device))?;
        let mut gone = self.devices[slot].take()?;
        match &mut gone.parser {
            Parser::Keyboard(parser) => parser.release_all_into(timestamp, out),
            Parser::Mouse(parser) => parser.release_all_into(timestamp, out),
        }
        Some(gone.parser.kind())
    }

    /// `device`'s boot report through its parser: the events are appended to `out` — nothing is
    /// allocated once `out` holds a report's worth (the live ingress path). A refused report
    /// leaves `out` and the device's state untouched.
    pub fn ingest_report_into(
        &mut self,
        device: DeviceId,
        timestamp: TimestampNs,
        report: &[u8],
        out: &mut Vec<HidEvent>,
    ) -> Result<HidDeviceKind, HidrawdError> {
        let registered = self
            .devices
            .iter_mut()
            .flatten()
            .find(|r| r.device == device)
            .ok_or(HidrawdError::UnknownDevice)?;
        let parsed = match &mut registered.parser {
            Parser::Keyboard(parser) => parser.parse_report_into(timestamp, report, out),
            Parser::Mouse(parser) => parser.parse_report_into(timestamp, report, out),
        };
        parsed.map(|()| registered.parser.kind()).map_err(HidrawdError::from)
    }

    /// A keyboard report as a batch of its own (host convenience: allocates, records).
    pub fn ingest_keyboard_report(
        &mut self,
        device: DeviceId,
        timestamp: TimestampNs,
        report: &[u8],
    ) -> Result<HidBatch, HidrawdError> {
        self.expect(device, HidDeviceKind::Keyboard)?;
        let mut events = Vec::new();
        self.ingest_report_into(device, timestamp, report, &mut events)?;
        let batch = HidBatch::new(device, HidDeviceKind::Keyboard, events);
        self.record_recent(&batch);
        Ok(batch)
    }

    /// A mouse report as a batch of its own (host convenience: allocates, records).
    pub fn ingest_mouse_report(
        &mut self,
        device: DeviceId,
        timestamp: TimestampNs,
        report: &[u8],
    ) -> Result<HidBatch, HidrawdError> {
        self.expect(device, HidDeviceKind::Mouse)?;
        let mut events = Vec::new();
        self.ingest_report_into(device, timestamp, report, &mut events)?;
        let batch = HidBatch::new_pointer(device, PointerSource::MouseRelative, events);
        self.record_recent(&batch);
        Ok(batch)
    }

    /// Events a transport already normalized (virtio-input), checked against the device's
    /// registration (host convenience: records).
    pub fn ingest_device_events(
        &mut self,
        device: DeviceId,
        kind: HidDeviceKind,
        events: Vec<HidEvent>,
    ) -> Result<HidBatch, HidrawdError> {
        self.expect(device, kind)?;
        let batch = HidBatch::new(device, kind, events);
        self.record_recent(&batch);
        Ok(batch)
    }

    #[must_use]
    pub fn recent_batches(&self) -> &[HidBatch] {
        self.recent_batches.as_slice()
    }

    fn find(&self, device: DeviceId) -> Option<&Registered> {
        self.devices.iter().flatten().find(|r| r.device == device)
    }

    /// `device` is registered as `kind`.
    fn expect(&self, device: DeviceId, kind: HidDeviceKind) -> Result<(), HidrawdError> {
        match (kind, self.kind_of(device)) {
            (expected, Some(actual)) if expected == actual => Ok(()),
            (expected, Some(actual)) => Err(HidrawdError::UnexpectedDevice { expected, actual }),
            (HidDeviceKind::Keyboard, None) => Err(HidrawdError::KeyboardUnavailable),
            (HidDeviceKind::Mouse, None) => Err(HidrawdError::MouseUnavailable),
        }
    }

    fn record_recent(&mut self, batch: &HidBatch) {
        if self.recent_batches.len() == MAX_LOGGED_BATCHES {
            self.recent_batches.remove(0);
        }
        self.recent_batches.push(batch.clone());
    }
}
