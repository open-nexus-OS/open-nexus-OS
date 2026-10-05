// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Standard descriptors (USB 2.0 §9.6) and the HID class descriptor (HID 1.11 §6.2.1).
//!
//! A device's descriptor is read twice: its first eight bytes name EP0's max packet
//! ([`max_packet0`]), then the whole 18 bytes ([`DeviceDescriptor::parse`]). A configuration
//! is read twice too: its nine-byte header names the total length ([`Configuration::total_length`]),
//! then [`Configuration::parse`] walks every descriptor in it ONCE, checking each length,
//! count and size — the iterators that follow re-walk bytes that are already proven, so they
//! cannot fail.

use crate::{le16, UsbError};

/// The device descriptor's type.
pub const DEVICE: u8 = 1;
/// The configuration descriptor's type.
pub const CONFIGURATION: u8 = 2;
/// The interface descriptor's type.
pub const INTERFACE: u8 = 4;
/// The endpoint descriptor's type.
pub const ENDPOINT: u8 = 5;
/// The HID class descriptor's type.
pub const HID: u8 = 0x21;

/// A device descriptor's length.
pub const DEVICE_LEN: usize = 18;
/// A configuration header's length.
pub const CONFIG_HEADER_LEN: usize = 9;
/// The largest configuration (header, interfaces, endpoints, class descriptors) the stack reads.
pub const CONFIG_MAX: usize = 1024;
/// The most interfaces a configuration may declare.
pub const MAX_INTERFACES: u8 = 8;
/// The most interface descriptors (alternate settings included) a configuration may carry.
pub const MAX_INTERFACE_DESCRIPTORS: usize = 16;
/// The most endpoints one interface may have.
pub const MAX_ENDPOINTS: u8 = 4;
/// The largest endpoint max packet (high-speed bulk/interrupt).
pub const MAX_PACKET: u16 = 1024;

const INTERFACE_LEN: usize = 9;
const ENDPOINT_LEN: usize = 7;
const HID_LEN: usize = 9;

/// The device descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceDescriptor {
    /// `bcdUSB`.
    pub usb: u16,
    /// `bDeviceClass` (9 = hub; 0 = per interface).
    pub class: u8,
    /// `bDeviceSubClass`.
    pub subclass: u8,
    /// `bDeviceProtocol` (a high-speed hub: 1 single TT, 2 a TT per port).
    pub protocol: u8,
    /// EP0's max packet in bytes.
    pub max_packet0: u16,
    /// `idVendor`.
    pub vendor: u16,
    /// `idProduct`.
    pub product: u16,
    /// `bcdDevice`.
    pub release: u16,
    /// `bNumConfigurations`.
    pub configurations: u8,
}

impl DeviceDescriptor {
    /// The 18-byte device descriptor.
    pub fn parse(bytes: &[u8]) -> Result<Self, UsbError> {
        let max_packet0 = max_packet0(bytes)?;
        let b = bytes.get(..DEVICE_LEN).ok_or(UsbError::Truncated)?;
        Ok(Self {
            usb: le16(b, 2).ok_or(UsbError::Truncated)?,
            class: b[4],
            subclass: b[5],
            protocol: b[6],
            max_packet0,
            vendor: le16(b, 8).ok_or(UsbError::Truncated)?,
            product: le16(b, 10).ok_or(UsbError::Truncated)?,
            release: le16(b, 12).ok_or(UsbError::Truncated)?,
            configurations: b[17],
        })
    }
}

/// EP0's max packet in bytes from the first eight bytes of a device descriptor — what a host
/// reads before it knows the max packet. 8, 16, 32 or 64; on a USB 3 device the field is an
/// exponent and 9 means 512.
pub fn max_packet0(prefix: &[u8]) -> Result<u16, UsbError> {
    let b = prefix.get(..8).ok_or(UsbError::Truncated)?;
    if b[0] as usize != DEVICE_LEN {
        return Err(UsbError::Length);
    }
    if b[1] != DEVICE {
        return Err(UsbError::Type);
    }
    let usb = le16(b, 2).ok_or(UsbError::Truncated)?;
    match b[7] {
        v @ (8 | 16 | 32 | 64) => Ok(u16::from(v)),
        9 if usb >= 0x0300 => Ok(512),
        _ => Err(UsbError::MaxPacket0),
    }
}

/// A transfer type (`bmAttributes` bits 1:0).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transfer {
    /// Control.
    Control,
    /// Isochronous.
    Isochronous,
    /// Bulk.
    Bulk,
    /// Interrupt.
    Interrupt,
}

/// An endpoint descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Endpoint {
    /// `bEndpointAddress` (bit 7 = IN).
    pub address: u8,
    /// `bmAttributes`.
    pub attributes: u8,
    /// `wMaxPacketSize` as read (bits 12:11 = extra transactions per microframe).
    pub max_packet: u16,
    /// `bInterval`.
    pub interval: u8,
}

impl Endpoint {
    /// The endpoint number (1..=15).
    #[must_use]
    pub const fn number(&self) -> u8 {
        self.address & 0x0f
    }

    /// Device to host.
    #[must_use]
    pub const fn is_in(&self) -> bool {
        self.address & 0x80 != 0
    }

    /// The transfer type.
    #[must_use]
    pub const fn transfer(&self) -> Transfer {
        match self.attributes & 0x03 {
            0 => Transfer::Control,
            1 => Transfer::Isochronous,
            2 => Transfer::Bulk,
            _ => Transfer::Interrupt,
        }
    }

    /// The max packet in bytes (bits 10:0).
    #[must_use]
    pub const fn max_packet_size(&self) -> u16 {
        self.max_packet & 0x07ff
    }

    /// Extra transactions per microframe (high-speed high-bandwidth endpoints; bits 12:11).
    #[must_use]
    pub const fn extra_transactions(&self) -> u8 {
        ((self.max_packet >> 11) & 0x03) as u8
    }
}

/// What a HID boot interface is (class 3, subclass 1, protocol 1 or 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HidRole {
    /// A boot keyboard.
    Keyboard,
    /// A boot mouse.
    Mouse,
}

/// An interface descriptor (one alternate setting) and the descriptors that follow it.
#[derive(Clone, Copy, Debug)]
pub struct Interface<'a> {
    /// `bInterfaceNumber`.
    pub number: u8,
    /// `bAlternateSetting`.
    pub alternate: u8,
    /// `bInterfaceClass`.
    pub class: u8,
    /// `bInterfaceSubClass`.
    pub subclass: u8,
    /// `bInterfaceProtocol`.
    pub protocol: u8,
    /// The descriptors up to the next interface (endpoints, class descriptors).
    rest: &'a [u8],
}

impl<'a> Interface<'a> {
    /// The interface's endpoints, in descriptor order.
    #[must_use]
    pub fn endpoints(&self) -> Endpoints<'a> {
        Endpoints { walk: Walk { bytes: self.rest, at: 0 } }
    }

    /// The boot role, for a HID boot interface.
    #[must_use]
    pub const fn hid_role(&self) -> Option<HidRole> {
        match (self.class, self.subclass, self.protocol) {
            (3, 1, 1) => Some(HidRole::Keyboard),
            (3, 1, 2) => Some(HidRole::Mouse),
            _ => None,
        }
    }

    /// The first interrupt-IN endpoint (a HID interface's report pipe, a hub's change pipe).
    #[must_use]
    pub fn interrupt_in(&self) -> Option<Endpoint> {
        self.endpoints().find(|e| e.is_in() && e.transfer() == Transfer::Interrupt)
    }
}

/// A whole configuration descriptor, proven by [`Configuration::parse`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Configuration<'a> {
    /// `bConfigurationValue` (what SET_CONFIGURATION takes).
    pub value: u8,
    /// `bmAttributes`.
    pub attributes: u8,
    /// `bMaxPower` (2 mA units).
    pub max_power: u8,
    /// `bNumInterfaces`.
    pub interfaces: u8,
    body: &'a [u8],
}

impl<'a> Configuration<'a> {
    /// `wTotalLength` from the nine-byte header — the length to read the whole descriptor with.
    pub fn total_length(header: &[u8]) -> Result<usize, UsbError> {
        let b = header.get(..CONFIG_HEADER_LEN).ok_or(UsbError::Truncated)?;
        if (b[0] as usize) < CONFIG_HEADER_LEN {
            return Err(UsbError::Length);
        }
        if b[1] != CONFIGURATION {
            return Err(UsbError::Type);
        }
        let total = usize::from(le16(b, 2).ok_or(UsbError::Truncated)?);
        if !(CONFIG_HEADER_LEN..=CONFIG_MAX).contains(&total) {
            return Err(UsbError::TotalLength);
        }
        Ok(total)
    }

    /// The whole configuration: every descriptor in it bounded, counted and checked.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, UsbError> {
        let total = Self::total_length(bytes)?;
        let body = bytes.get(..total).ok_or(UsbError::Truncated)?;
        let interfaces = body[4];
        if interfaces > MAX_INTERFACES {
            return Err(UsbError::Interfaces);
        }
        let mut at = body[0] as usize;
        let (mut interface_descriptors, mut in_interface, mut endpoints) = (0usize, false, 0u8);
        while at < total {
            let len = body[at] as usize;
            let kind = *body.get(at + 1).ok_or(UsbError::Length)?;
            if len < 2 || at + len > total {
                return Err(UsbError::Length);
            }
            match kind {
                INTERFACE => {
                    if len < INTERFACE_LEN {
                        return Err(UsbError::Length);
                    }
                    interface_descriptors += 1;
                    if interface_descriptors > MAX_INTERFACE_DESCRIPTORS {
                        return Err(UsbError::Interfaces);
                    }
                    if body[at + 4] > MAX_ENDPOINTS {
                        return Err(UsbError::Endpoints);
                    }
                    (in_interface, endpoints) = (true, 0);
                }
                ENDPOINT => {
                    if len < ENDPOINT_LEN {
                        return Err(UsbError::Length);
                    }
                    if !in_interface {
                        return Err(UsbError::Order);
                    }
                    endpoints += 1;
                    if endpoints > MAX_ENDPOINTS {
                        return Err(UsbError::Endpoints);
                    }
                    let size = le16(body, at + 4).ok_or(UsbError::Length)? & 0x07ff;
                    if size == 0 || size > MAX_PACKET {
                        return Err(UsbError::MaxPacket);
                    }
                }
                HID if len < HID_LEN => return Err(UsbError::Length),
                _ => {}
            }
            at += len;
        }
        Ok(Self { value: body[5], attributes: body[7], max_power: body[8], interfaces, body })
    }

    /// Every interface descriptor (each alternate setting), in order.
    #[must_use]
    pub fn interface_list(&self) -> Interfaces<'a> {
        Interfaces { walk: Walk { bytes: self.body, at: self.body[0] as usize } }
    }
}

/// A walk over descriptors already bounded by [`Configuration::parse`].
#[derive(Clone, Copy, Debug)]
struct Walk<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Iterator for Walk<'a> {
    type Item = (u8, &'a [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        let len = *self.bytes.get(self.at)? as usize;
        let kind = *self.bytes.get(self.at + 1)?;
        let item = self.bytes.get(self.at..self.at + len).filter(|_| len >= 2)?;
        self.at += len;
        Some((kind, item))
    }
}

/// The interface descriptors of a configuration.
#[derive(Clone, Copy, Debug)]
pub struct Interfaces<'a> {
    walk: Walk<'a>,
}

impl<'a> Iterator for Interfaces<'a> {
    type Item = Interface<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let (kind, d) = self.walk.next()?;
            if kind != INTERFACE {
                continue;
            }
            let start = self.walk.at;
            let mut end = start;
            let mut peek = self.walk;
            while let Some((kind, _)) = peek.next() {
                if kind == INTERFACE {
                    break;
                }
                end = peek.at;
            }
            return Some(Interface {
                number: d[2],
                alternate: d[3],
                class: d[5],
                subclass: d[6],
                protocol: d[7],
                rest: self.walk.bytes.get(start..end)?,
            });
        }
    }
}

/// The endpoint descriptors of an interface.
#[derive(Clone, Copy, Debug)]
pub struct Endpoints<'a> {
    walk: Walk<'a>,
}

impl Iterator for Endpoints<'_> {
    type Item = Endpoint;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let (kind, d) = self.walk.next()?;
            if kind == ENDPOINT {
                return Some(Endpoint {
                    address: d[2],
                    attributes: d[3],
                    max_packet: le16(d, 4)?,
                    interval: d[6],
                });
            }
        }
    }
}
