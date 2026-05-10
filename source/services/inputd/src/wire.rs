// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Narrow live-wire decode seam for `inputd` with explicit reject classes.
//! OWNERS: @runtime @ui
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `cargo test -p inputd -- --nocapture`
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

extern crate alloc;

use alloc::vec::Vec;

use hid::{HidEvent, TimestampNs};
use hidrawd::{DeviceId, HidBatch, HidDeviceKind, PointerSource};
use input_live_protocol::{
    WireHidBatch, EVENT_KIND_ABS, EVENT_KIND_BTN, EVENT_KIND_KEY, EVENT_KIND_REL,
    HID_KIND_KEYBOARD, HID_KIND_MOUSE, POINTER_SOURCE_MOUSE_RELATIVE, POINTER_SOURCE_NONE,
    POINTER_SOURCE_TABLET_ABSOLUTE, POINTER_SOURCE_TOUCH_ABSOLUTE, STATUS_MALFORMED,
    STATUS_UNSUPPORTED,
};
use pointer_state::{AbsoluteAxisCalibration, PointerAxis, PointerTransform};

const REL_WHEEL_EVENT_CODE: u16 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireBatchReject {
    CountMismatch,
    RawCountUnderflow,
    UnknownDeviceKind(u8),
    KeyboardPointerSource(u8),
    MissingPointerSource,
    UnknownPointerSource(u8),
    KeyboardEventKind(u8),
    PointerKeyEvent,
    RelativeOnAbsoluteSource(PointerSource),
    AbsoluteOnRelativeSource(PointerSource),
    InvalidAbsoluteCalibration(PointerAxis),
    InvalidAbsoluteAxis(u16),
    UnknownEventKind(u8),
}

impl WireBatchReject {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::CountMismatch => "count-mismatch",
            Self::RawCountUnderflow => "raw-count-underflow",
            Self::UnknownDeviceKind(_) => "device-kind",
            Self::KeyboardPointerSource(_) => "keyboard-pointer-source",
            Self::MissingPointerSource => "pointer-source-missing",
            Self::UnknownPointerSource(_) => "pointer-source-unknown",
            Self::KeyboardEventKind(_) => "keyboard-event-kind",
            Self::PointerKeyEvent => "pointer-key-event",
            Self::RelativeOnAbsoluteSource(PointerSource::TabletAbsolute) => {
                "relative-on-tablet-absolute"
            }
            Self::RelativeOnAbsoluteSource(PointerSource::TouchAbsolute) => {
                "relative-on-touch-absolute"
            }
            Self::RelativeOnAbsoluteSource(PointerSource::MouseRelative) => {
                "relative-on-mouse-relative"
            }
            Self::AbsoluteOnRelativeSource(PointerSource::MouseRelative) => {
                "absolute-on-mouse-relative"
            }
            Self::AbsoluteOnRelativeSource(PointerSource::TabletAbsolute) => {
                "absolute-on-tablet-absolute"
            }
            Self::AbsoluteOnRelativeSource(PointerSource::TouchAbsolute) => {
                "absolute-on-touch-absolute"
            }
            Self::InvalidAbsoluteCalibration(PointerAxis::X) => "abs-calibration-x",
            Self::InvalidAbsoluteCalibration(PointerAxis::Y) => "abs-calibration-y",
            Self::InvalidAbsoluteAxis(_) => "abs-axis",
            Self::UnknownEventKind(_) => "event-kind",
        }
    }

    #[must_use]
    pub const fn status(self) -> u8 {
        match self {
            Self::UnknownDeviceKind(_) | Self::UnknownPointerSource(_) => STATUS_UNSUPPORTED,
            _ => STATUS_MALFORMED,
        }
    }
}

pub fn decode_wire_batch(
    batch: WireHidBatch,
    pointer_transform: PointerTransform,
) -> Result<HidBatch, WireBatchReject> {
    if usize::from(batch.normalized_event_count) != batch.events.len() {
        return Err(WireBatchReject::CountMismatch);
    }
    if batch.raw_event_count < batch.normalized_event_count {
        return Err(WireBatchReject::RawCountUnderflow);
    }

    let kind = match batch.device_kind {
        HID_KIND_KEYBOARD => {
            if batch.pointer_source != POINTER_SOURCE_NONE {
                return Err(WireBatchReject::KeyboardPointerSource(batch.pointer_source));
            }
            HidDeviceKind::Keyboard
        }
        HID_KIND_MOUSE => HidDeviceKind::Mouse,
        other => return Err(WireBatchReject::UnknownDeviceKind(other)),
    };
    let pointer_source = match kind {
        HidDeviceKind::Keyboard => None,
        HidDeviceKind::Mouse => Some(pointer_source_from_wire(batch.pointer_source)?),
    };

    let mut events = Vec::with_capacity(batch.events.len());
    for event in batch.events {
        let timestamp = TimestampNs::new(event.timestamp_ns);
        let hid_event = match event.kind {
            EVENT_KIND_KEY if kind == HidDeviceKind::Keyboard => {
                HidEvent::key(timestamp, event.code, event.value)
            }
            EVENT_KIND_KEY => return Err(WireBatchReject::PointerKeyEvent),
            EVENT_KIND_REL => match pointer_source.expect("pointer source for pointer device") {
                PointerSource::MouseRelative => HidEvent::rel(timestamp, event.code, event.value),
                _source if event.code == REL_WHEEL_EVENT_CODE => {
                    HidEvent::rel(timestamp, event.code, event.value)
                }
                source => return Err(WireBatchReject::RelativeOnAbsoluteSource(source)),
            },
            EVENT_KIND_BTN if kind == HidDeviceKind::Mouse => {
                HidEvent::btn(timestamp, event.code, event.value)
            }
            EVENT_KIND_BTN => return Err(WireBatchReject::KeyboardEventKind(EVENT_KIND_BTN)),
            EVENT_KIND_ABS => {
                let source = pointer_source.expect("pointer source for pointer device");
                if source == PointerSource::MouseRelative {
                    return Err(WireBatchReject::AbsoluteOnRelativeSource(source));
                }
                let scaled = match event.code {
                    0 => pointer_transform.scale_absolute_axis(
                        event.value,
                        AbsoluteAxisCalibration::new(0, batch.abs_max_x).map_err(|_| {
                            WireBatchReject::InvalidAbsoluteCalibration(PointerAxis::X)
                        })?,
                        PointerAxis::X,
                    ),
                    1 => pointer_transform.scale_absolute_axis(
                        event.value,
                        AbsoluteAxisCalibration::new(0, batch.abs_max_y).map_err(|_| {
                            WireBatchReject::InvalidAbsoluteCalibration(PointerAxis::Y)
                        })?,
                        PointerAxis::Y,
                    ),
                    _ => return Err(WireBatchReject::InvalidAbsoluteAxis(event.code)),
                };
                HidEvent::abs(timestamp, event.code, scaled)
            }
            other if kind == HidDeviceKind::Keyboard => {
                return Err(WireBatchReject::KeyboardEventKind(other))
            }
            other => return Err(WireBatchReject::UnknownEventKind(other)),
        };
        events.push(hid_event);
    }

    Ok(match (kind, pointer_source) {
        (HidDeviceKind::Keyboard, _) => HidBatch::new(DeviceId::new(batch.device_id), kind, events),
        (HidDeviceKind::Mouse, Some(source)) => {
            HidBatch::new_pointer(DeviceId::new(batch.device_id), source, events)
        }
        (HidDeviceKind::Mouse, None) => HidBatch::new(DeviceId::new(batch.device_id), kind, events),
    })
}

fn pointer_source_from_wire(value: u8) -> Result<PointerSource, WireBatchReject> {
    match value {
        POINTER_SOURCE_NONE => Err(WireBatchReject::MissingPointerSource),
        POINTER_SOURCE_MOUSE_RELATIVE => Ok(PointerSource::MouseRelative),
        POINTER_SOURCE_TABLET_ABSOLUTE => Ok(PointerSource::TabletAbsolute),
        POINTER_SOURCE_TOUCH_ABSOLUTE => Ok(PointerSource::TouchAbsolute),
        other => Err(WireBatchReject::UnknownPointerSource(other)),
    }
}
