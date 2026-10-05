// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The USB protocol vocabulary of the host stack (TASK-0328 U1, RFC-0099): the
//! descriptors a device hands over, parsed with every bound checked before a byte is used
//! ([`descriptor`]); the setup packets of the standard, HID and hub requests ([`request`]);
//! the hub class — its descriptor, its port status and its change bitmap ([`hub`]); speeds
//! and the route strings that address a device through hubs ([`route`]). Controller-agnostic:
//! the xHCI encodings live with the controller's driver (`xhcid`). Descriptors and statuses
//! are untrusted input: nothing is unwrapped, every refusal has a stable code ([`UsbError`]).
//! OWNERS: @runtime @drivers
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `tests/usb` — the desk's devices' descriptors as goldens
//!   (`docs/board/measurements/2026-10-04-usb-boot-protocol/stock-descriptors.txt`), the hub's
//!   class descriptor and port statuses, the setup packets, route strings to five tiers, every
//!   refusal by its code (`test_reject_*`)
//! INVARIANTS: no read past a descriptor's own length or past the buffer; a configuration is
//!   at most `CONFIG_MAX` bytes with at most `MAX_INTERFACES` interfaces and
//!   `MAX_ENDPOINTS` endpoints per interface; a hub has 1..=15 ports; a route is at most five
//!   hubs deep.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod descriptor;
pub mod hub;
pub mod request;
pub mod route;

pub use descriptor::{Configuration, DeviceDescriptor, Endpoint, HidRole, Interface, Transfer};
pub use hub::{HubDescriptor, PortStatus};
pub use request::{HidProtocol, PortFeature, SetupPacket};
pub use route::{RouteString, Speed};

/// Why a descriptor, a status or a route was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UsbError {
    /// Fewer bytes than the structure needs.
    Truncated,
    /// A descriptor's `bLength` is below 2, below its type's minimum, or runs past the buffer.
    Length,
    /// Not the descriptor type asked for.
    Type,
    /// A configuration's `wTotalLength` is below its header or past `CONFIG_MAX`.
    TotalLength,
    /// More interfaces than `MAX_INTERFACES`.
    Interfaces,
    /// More endpoints in one interface than `MAX_ENDPOINTS`.
    Endpoints,
    /// An endpoint descriptor outside an interface.
    Order,
    /// `bMaxPacketSize0` is not 8, 16, 32 or 64 (or 9 — 512 bytes — on a USB 3 device).
    MaxPacket0,
    /// An endpoint's max packet size is 0 or past 1024.
    MaxPacket,
    /// A hub with no port or more than 15.
    HubPorts,
    /// A route through a port that is not 1..=15.
    RoutePort,
    /// A route deeper than five hubs.
    RouteDepth,
}

impl UsbError {
    /// The refusal's stable code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Truncated => "usb.desc.truncated",
            Self::Length => "usb.desc.length",
            Self::Type => "usb.desc.type",
            Self::TotalLength => "usb.desc.total_length",
            Self::Interfaces => "usb.desc.interfaces",
            Self::Endpoints => "usb.desc.endpoints",
            Self::Order => "usb.desc.order",
            Self::MaxPacket0 => "usb.desc.max_packet0",
            Self::MaxPacket => "usb.desc.max_packet",
            Self::HubPorts => "usb.hub.ports",
            Self::RoutePort => "usb.route.port",
            Self::RouteDepth => "usb.route.depth",
        }
    }
}

/// A little-endian `u16` at `at`, `None` past the end.
pub(crate) fn le16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*bytes.get(at)?, *bytes.get(at + 1)?]))
}
