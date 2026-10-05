// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Setup packets (USB 2.0 §9.3): the standard requests enumeration needs, the HID class
//! requests of a boot interface (HID 1.11 §7.2) and the hub class requests (USB 2.0 §11.24).

use crate::descriptor::{CONFIGURATION, DEVICE};
use crate::hub::HUB_DESCRIPTOR;

const GET_STATUS: u8 = 0;
const CLEAR_FEATURE: u8 = 1;
const SET_FEATURE: u8 = 3;
const GET_DESCRIPTOR: u8 = 6;
const SET_CONFIGURATION: u8 = 9;
const SET_INTERFACE: u8 = 11;
const HID_SET_IDLE: u8 = 0x0a;
const HID_SET_PROTOCOL: u8 = 0x0b;

/// Direction, type and recipient (`bmRequestType`).
const STANDARD_DEVICE_IN: u8 = 0x80;
const STANDARD_DEVICE_OUT: u8 = 0x00;
const STANDARD_INTERFACE_OUT: u8 = 0x01;
const CLASS_INTERFACE_OUT: u8 = 0x21;
const CLASS_DEVICE_IN: u8 = 0xa0;
const CLASS_OTHER_IN: u8 = 0xa3;
const CLASS_OTHER_OUT: u8 = 0x23;

/// The eight bytes of a control transfer's setup stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SetupPacket {
    /// `bmRequestType`.
    pub request_type: u8,
    /// `bRequest`.
    pub request: u8,
    /// `wValue`.
    pub value: u16,
    /// `wIndex`.
    pub index: u16,
    /// `wLength`: the data stage's length (0 = none).
    pub length: u16,
}

/// The protocol a HID boot interface reports in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HidProtocol {
    /// The fixed boot report format.
    Boot,
    /// The format the report descriptor describes.
    Report,
}

/// A hub port feature (USB 2.0 Table 11-17).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortFeature {
    /// Reset the port.
    Reset,
    /// Power the port.
    Power,
    /// The connection changed.
    CConnection,
    /// The port was disabled by the hub.
    CEnable,
    /// The suspend state changed.
    CSuspend,
    /// The over-current state changed.
    COverCurrent,
    /// A reset completed.
    CReset,
}

impl PortFeature {
    /// The feature selector.
    #[must_use]
    pub const fn selector(self) -> u16 {
        match self {
            Self::Reset => 4,
            Self::Power => 8,
            Self::CConnection => 16,
            Self::CEnable => 17,
            Self::CSuspend => 18,
            Self::COverCurrent => 19,
            Self::CReset => 20,
        }
    }
}

impl SetupPacket {
    /// The packet's bytes, as the setup stage sends them.
    #[must_use]
    pub const fn to_bytes(self) -> [u8; 8] {
        let v = self.value.to_le_bytes();
        let i = self.index.to_le_bytes();
        let l = self.length.to_le_bytes();
        [self.request_type, self.request, v[0], v[1], i[0], i[1], l[0], l[1]]
    }

    /// A packet from its bytes (what a device sees).
    #[must_use]
    pub const fn from_bytes(b: [u8; 8]) -> Self {
        Self {
            request_type: b[0],
            request: b[1],
            value: u16::from_le_bytes([b[2], b[3]]),
            index: u16::from_le_bytes([b[4], b[5]]),
            length: u16::from_le_bytes([b[6], b[7]]),
        }
    }

    /// The data stage (if any) moves from the device to the host.
    #[must_use]
    pub const fn is_in(&self) -> bool {
        self.request_type & 0x80 != 0
    }

    /// GET_DESCRIPTOR(device), the first `length` bytes.
    #[must_use]
    pub const fn device_descriptor(length: u16) -> Self {
        Self::get_descriptor(STANDARD_DEVICE_IN, DEVICE, length)
    }

    /// GET_DESCRIPTOR(configuration 0), the first `length` bytes.
    #[must_use]
    pub const fn configuration_descriptor(length: u16) -> Self {
        Self::get_descriptor(STANDARD_DEVICE_IN, CONFIGURATION, length)
    }

    const fn get_descriptor(request_type: u8, kind: u8, length: u16) -> Self {
        Self { request_type, request: GET_DESCRIPTOR, value: (kind as u16) << 8, index: 0, length }
    }

    /// SET_CONFIGURATION(`value`).
    #[must_use]
    pub const fn set_configuration(value: u8) -> Self {
        Self {
            request_type: STANDARD_DEVICE_OUT,
            request: SET_CONFIGURATION,
            value: value as u16,
            index: 0,
            length: 0,
        }
    }

    /// SET_INTERFACE(`interface`, `alternate`).
    #[must_use]
    pub const fn set_interface(interface: u8, alternate: u8) -> Self {
        Self {
            request_type: STANDARD_INTERFACE_OUT,
            request: SET_INTERFACE,
            value: alternate as u16,
            index: interface as u16,
            length: 0,
        }
    }

    /// HID SET_PROTOCOL on `interface`.
    #[must_use]
    pub const fn hid_set_protocol(interface: u8, protocol: HidProtocol) -> Self {
        let value = match protocol {
            HidProtocol::Boot => 0,
            HidProtocol::Report => 1,
        };
        Self {
            request_type: CLASS_INTERFACE_OUT,
            request: HID_SET_PROTOCOL,
            value,
            index: interface as u16,
            length: 0,
        }
    }

    /// HID SET_IDLE on `interface`: a report only when the data changes (duration 0), every
    /// report id.
    #[must_use]
    pub const fn hid_set_idle_infinite(interface: u8) -> Self {
        Self {
            request_type: CLASS_INTERFACE_OUT,
            request: HID_SET_IDLE,
            value: 0,
            index: interface as u16,
            length: 0,
        }
    }

    /// GET_DESCRIPTOR(hub), the first `length` bytes.
    #[must_use]
    pub const fn hub_descriptor(length: u16) -> Self {
        Self::get_descriptor(CLASS_DEVICE_IN, HUB_DESCRIPTOR, length)
    }

    /// GET_STATUS(port): four bytes, status then change.
    #[must_use]
    pub const fn hub_port_status(port: u8) -> Self {
        Self {
            request_type: CLASS_OTHER_IN,
            request: GET_STATUS,
            value: 0,
            index: port as u16,
            length: 4,
        }
    }

    /// SET_FEATURE(port, `feature`).
    #[must_use]
    pub const fn hub_set_port_feature(port: u8, feature: PortFeature) -> Self {
        Self {
            request_type: CLASS_OTHER_OUT,
            request: SET_FEATURE,
            value: feature.selector(),
            index: port as u16,
            length: 0,
        }
    }

    /// CLEAR_FEATURE(port, `feature`).
    #[must_use]
    pub const fn hub_clear_port_feature(port: u8, feature: PortFeature) -> Self {
        Self {
            request_type: CLASS_OTHER_OUT,
            request: CLEAR_FEATURE,
            value: feature.selector(),
            index: port as u16,
            length: 0,
        }
    }
}
