// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Explicit raw-ingress -> normalized-HID adapter seam for `hidrawd`.
//! OWNERS: @runtime @ui
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `cargo test -p hidrawd -- --nocapture`
//! ADR: docs/rfcs/RFC-0054-input-v1_0c-os-qemu-virtio-input-driver-layer-contract.md

extern crate alloc;

use alloc::vec::Vec;
use hid::{HidEvent, HidEventKind, TimestampNs};
use input_live_protocol::{
    WireHidBatch, WireHidEvent, EVENT_KIND_ABS, EVENT_KIND_BTN, EVENT_KIND_KEY, EVENT_KIND_REL,
    HID_KIND_KEYBOARD, HID_KIND_MOUSE, POINTER_SOURCE_MOUSE_RELATIVE, POINTER_SOURCE_NONE,
    POINTER_SOURCE_TABLET_ABSOLUTE, POINTER_SOURCE_TOUCH_ABSOLUTE,
};

use crate::{DeviceId, HidBatch, HidDeviceKind, HidrawdError, HidrawdService};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngressRole {
    Keyboard,
    RelativePointer,
    AbsolutePointer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerSource {
    MouseRelative,
    TabletAbsolute,
    TouchAbsolute,
}

pub const QEMU_ABSOLUTE_AXIS_FALLBACK_MAX: i32 = 32_767;

impl PointerSource {
    #[must_use]
    pub const fn wire_value(self) -> u8 {
        match self {
            Self::MouseRelative => POINTER_SOURCE_MOUSE_RELATIVE,
            Self::TabletAbsolute => POINTER_SOURCE_TABLET_ABSOLUTE,
            Self::TouchAbsolute => POINTER_SOURCE_TOUCH_ABSOLUTE,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawIngressEventKind {
    Key,
    Relative,
    Absolute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawIngressEvent {
    kind: RawIngressEventKind,
    code: u16,
    value: i32,
}

impl RawIngressEvent {
    #[must_use]
    pub const fn new(kind: RawIngressEventKind, code: u16, value: i32) -> Self {
        Self { kind, code, value }
    }

    #[must_use]
    pub const fn kind(self) -> RawIngressEventKind {
        self.kind
    }

    #[must_use]
    pub const fn code(self) -> u16 {
        self.code
    }

    #[must_use]
    pub const fn value(self) -> i32 {
        self.value
    }
}

#[must_use]
pub fn resolve_absolute_axis_max(
    pointer_source: Option<PointerSource>,
    reported_max: i32,
    raw_events: &[RawIngressEvent],
    axis_code: u16,
) -> i32 {
    if reported_max > 0 {
        return reported_max;
    }
    let absolute_source = matches!(
        pointer_source,
        Some(PointerSource::TabletAbsolute) | Some(PointerSource::TouchAbsolute)
    );
    if absolute_source
        && raw_events
            .iter()
            .any(|event| event.kind() == RawIngressEventKind::Absolute && event.code() == axis_code)
    {
        return QEMU_ABSOLUTE_AXIS_FALLBACK_MAX;
    }
    0
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawIngressBatch {
    role: IngressRole,
    pointer_source: Option<PointerSource>,
    events: Vec<RawIngressEvent>,
}

impl RawIngressBatch {
    #[must_use]
    pub fn new(role: IngressRole, events: Vec<RawIngressEvent>) -> Self {
        Self {
            role,
            pointer_source: default_pointer_source(role),
            events,
        }
    }

    #[must_use]
    pub fn with_pointer_source(
        role: IngressRole,
        pointer_source: Option<PointerSource>,
        events: Vec<RawIngressEvent>,
    ) -> Self {
        Self {
            role,
            pointer_source,
            events,
        }
    }

    #[must_use]
    pub const fn role(&self) -> IngressRole {
        self.role
    }

    #[must_use]
    pub const fn pointer_source(&self) -> Option<PointerSource> {
        self.pointer_source
    }

    #[must_use]
    pub fn events(&self) -> &[RawIngressEvent] {
        self.events.as_slice()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IngressGateEvidence {
    raw_event_count: u16,
    normalized_event_count: u16,
}

impl IngressGateEvidence {
    #[must_use]
    pub const fn new(raw_event_count: u16, normalized_event_count: u16) -> Self {
        Self {
            raw_event_count,
            normalized_event_count,
        }
    }

    #[must_use]
    pub const fn raw_event_count(self) -> u16 {
        self.raw_event_count
    }

    #[must_use]
    pub const fn normalized_event_count(self) -> u16 {
        self.normalized_event_count
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngressNormalization {
    evidence: IngressGateEvidence,
    hid_batch: Option<HidBatch>,
    wire_batch: Option<WireHidBatch>,
}

impl IngressNormalization {
    #[must_use]
    pub const fn evidence(&self) -> IngressGateEvidence {
        self.evidence
    }

    #[must_use]
    pub fn hid_batch(&self) -> Option<&HidBatch> {
        self.hid_batch.as_ref()
    }

    #[must_use]
    pub fn wire_batch(&self) -> Option<&WireHidBatch> {
        self.wire_batch.as_ref()
    }

    #[must_use]
    pub fn into_wire_batch(self) -> Option<WireHidBatch> {
        self.wire_batch
    }
}

pub fn normalize_ingress_batch(
    service: &mut HidrawdService,
    device_id: DeviceId,
    raw_batch: &RawIngressBatch,
    timestamp: TimestampNs,
    abs_max_x: i32,
    abs_max_y: i32,
) -> Result<IngressNormalization, HidrawdError> {
    let hid_kind = match raw_batch.role() {
        IngressRole::Keyboard => HidDeviceKind::Keyboard,
        IngressRole::RelativePointer | IngressRole::AbsolutePointer => HidDeviceKind::Mouse,
    };
    let wire_kind = match hid_kind {
        HidDeviceKind::Keyboard => HID_KIND_KEYBOARD,
        HidDeviceKind::Mouse => HID_KIND_MOUSE,
    };

    let mut events = Vec::new();
    for raw_event in raw_batch.events() {
        if let Some(event) = translate_raw_event(raw_batch.role(), *raw_event, timestamp) {
            events.push(event);
        }
    }

    let evidence = IngressGateEvidence::new(
        raw_batch.events().len() as u16,
        events.len().min(u16::MAX as usize) as u16,
    );
    if events.is_empty() {
        return Ok(IngressNormalization {
            evidence,
            hid_batch: None,
            wire_batch: None,
        });
    }

    let hid_batch = service.ingest_device_events(device_id, hid_kind, events)?;
    let wire_batch = WireHidBatch {
        device_kind: wire_kind,
        device_id: device_id.raw(),
        pointer_source: raw_batch
            .pointer_source()
            .map_or(POINTER_SOURCE_NONE, PointerSource::wire_value),
        abs_max_x,
        abs_max_y,
        raw_event_count: evidence.raw_event_count(),
        normalized_event_count: evidence.normalized_event_count(),
        events: batch_to_wire_events(&hid_batch),
    };
    Ok(IngressNormalization {
        evidence,
        hid_batch: Some(hid_batch),
        wire_batch: Some(wire_batch),
    })
}

fn batch_to_wire_events(batch: &HidBatch) -> Vec<WireHidEvent> {
    let mut out = Vec::with_capacity(batch.events().len());
    for event in batch.events() {
        let kind = match event.kind() {
            HidEventKind::Key => EVENT_KIND_KEY,
            HidEventKind::Rel => EVENT_KIND_REL,
            HidEventKind::Abs => EVENT_KIND_ABS,
            HidEventKind::Btn => EVENT_KIND_BTN,
        };
        out.push(WireHidEvent {
            kind,
            code: event.code().raw(),
            value: event.value().raw(),
            timestamp_ns: event.timestamp().raw(),
        });
    }
    out
}

fn translate_raw_event(
    role: IngressRole,
    event: RawIngressEvent,
    timestamp: TimestampNs,
) -> Option<HidEvent> {
    match (role, event.kind()) {
        (IngressRole::Keyboard, RawIngressEventKind::Key) => {
            let code = linux_key_to_hid(event.code())?;
            let value = match event.value() {
                0 => 0,
                1 => 1,
                _ => return None,
            };
            Some(HidEvent::key(timestamp, code, value))
        }
        (IngressRole::RelativePointer, RawIngressEventKind::Relative)
            if event.code() == 0 || event.code() == 1 || event.code() == 8 =>
        {
            Some(HidEvent::rel(timestamp, event.code(), event.value()))
        }
        (IngressRole::AbsolutePointer, RawIngressEventKind::Relative) if event.code() == 8 => {
            Some(HidEvent::rel(timestamp, event.code(), event.value()))
        }
        (IngressRole::AbsolutePointer, RawIngressEventKind::Absolute)
            if event.code() == 0 || event.code() == 1 =>
        {
            Some(HidEvent::abs(timestamp, event.code(), event.value()))
        }
        (_, RawIngressEventKind::Key) if event.code() >= 0x110 => {
            let value = if event.value() > 0 { 1 } else { 0 };
            Some(HidEvent::btn(timestamp, event.code(), value))
        }
        _ => None,
    }
}

const fn default_pointer_source(role: IngressRole) -> Option<PointerSource> {
    match role {
        IngressRole::Keyboard => None,
        IngressRole::RelativePointer => Some(PointerSource::MouseRelative),
        IngressRole::AbsolutePointer => Some(PointerSource::TabletAbsolute),
    }
}

fn linux_key_to_hid(code: u16) -> Option<u16> {
    Some(match code {
        1 => 0x29,
        2 => 0x1e,
        3 => 0x1f,
        4 => 0x20,
        5 => 0x21,
        6 => 0x22,
        7 => 0x23,
        8 => 0x24,
        9 => 0x25,
        10 => 0x26,
        11 => 0x27,
        12 => 0x2d,
        13 => 0x2e,
        14 => 0x2a,
        15 => 0x2b,
        16 => 0x14,
        17 => 0x1a,
        18 => 0x08,
        19 => 0x15,
        20 => 0x17,
        21 => 0x1c,
        22 => 0x18,
        23 => 0x0c,
        24 => 0x12,
        25 => 0x13,
        26 => 0x2f,
        27 => 0x30,
        28 => 0x28,
        29 => 0xe0,
        30 => 0x04,
        31 => 0x16,
        32 => 0x07,
        33 => 0x09,
        34 => 0x0a,
        35 => 0x0b,
        36 => 0x0d,
        37 => 0x0e,
        38 => 0x0f,
        39 => 0x33,
        40 => 0x34,
        41 => 0x35,
        42 => 0xe1,
        43 => 0x31,
        44 => 0x1d,
        45 => 0x1b,
        46 => 0x06,
        47 => 0x19,
        48 => 0x05,
        49 => 0x11,
        50 => 0x10,
        51 => 0x36,
        52 => 0x37,
        53 => 0x38,
        54 => 0xe5,
        55 => 0x55,
        56 => 0xe2,
        57 => 0x2c,
        58 => 0x39,
        59 => 0x3a,
        97 => 0xe4,
        100 => 0xe6,
        125 => 0xe3,
        126 => 0xe7,
        _ => return None,
    })
}
