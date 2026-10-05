// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The hub class (USB 2.0 §11.23–11.24): the hub descriptor, a port's status and change bits,
//! and the status-change bitmap the hub's interrupt pipe reports.

use crate::route::Speed;
use crate::{le16, UsbError};

/// The hub descriptor's type.
pub const HUB_DESCRIPTOR: u8 = 0x29;
/// The most ports a hub may have (a route string's nibble).
pub const MAX_PORTS: u8 = 15;
/// The descriptor's fixed part. Two bitmaps follow: `DeviceRemovable` with a bit per port plus
/// the reserved bit 0 (⌈(ports + 1) / 8⌉ bytes) and `PortPwrCtrlMask` with a bit per port
/// (⌈ports / 8⌉ bytes) — QEMU's hub sends exactly that (10 bytes for 8 ports, measured
/// 2026-10-04 in the `usb` lane); a hub that pads the mask to the first bitmap's size is longer
/// and fine.
const FIXED_LEN: usize = 7;

/// The hub descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HubDescriptor {
    /// `bNbrPorts`.
    pub ports: u8,
    /// `wHubCharacteristics`.
    pub characteristics: u16,
    /// How long a port takes from power-on to power-good, in ms (`bPwrOn2PwrGood` × 2).
    pub power_good_ms: u16,
    /// `bHubContrCurrent`, mA.
    pub controller_current_ma: u8,
}

impl HubDescriptor {
    /// The descriptor, its port count bounded and its bitmaps present.
    pub fn parse(bytes: &[u8]) -> Result<Self, UsbError> {
        let b = bytes.get(..FIXED_LEN).ok_or(UsbError::Truncated)?;
        if b[1] != HUB_DESCRIPTOR {
            return Err(UsbError::Type);
        }
        let ports = b[2];
        if ports == 0 || ports > MAX_PORTS {
            return Err(UsbError::HubPorts);
        }
        let removable = (usize::from(ports) + 1).div_ceil(8);
        let power_mask = usize::from(ports).div_ceil(8);
        let len = b[0] as usize;
        if len < FIXED_LEN + removable + power_mask {
            return Err(UsbError::Length);
        }
        if bytes.len() < len {
            return Err(UsbError::Truncated);
        }
        Ok(Self {
            ports,
            characteristics: le16(b, 3).ok_or(UsbError::Truncated)?,
            power_good_ms: u16::from(b[5]) * 2,
            controller_current_ma: b[6],
        })
    }

    /// The transaction translator's think time as the slot context's TTT encodes it: 0..=3 for
    /// 8, 16, 24 or 32 full-speed bit times (bits 6:5; a high-speed hub's).
    #[must_use]
    pub const fn tt_think_time(&self) -> u8 {
        ((self.characteristics >> 5) & 0x03) as u8
    }

    /// Ports are powered one by one (bits 1:0 = 01), not all together.
    #[must_use]
    pub const fn per_port_power(&self) -> bool {
        self.characteristics & 0x03 == 0x01
    }
}

/// A port's status and change bits (GET_STATUS(port)).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PortStatus {
    /// `wPortStatus`.
    pub status: u16,
    /// `wPortChange`.
    pub change: u16,
}

impl PortStatus {
    /// The four bytes GET_STATUS(port) returns.
    pub fn parse(bytes: &[u8]) -> Result<Self, UsbError> {
        Ok(Self {
            status: le16(bytes, 0).ok_or(UsbError::Truncated)?,
            change: le16(bytes, 2).ok_or(UsbError::Truncated)?,
        })
    }

    /// A device is connected.
    #[must_use]
    pub const fn connected(&self) -> bool {
        self.status & 0x0001 != 0
    }

    /// The port is enabled (after a reset).
    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.status & 0x0002 != 0
    }

    /// A reset is in progress.
    #[must_use]
    pub const fn resetting(&self) -> bool {
        self.status & 0x0010 != 0
    }

    /// The port is powered.
    #[must_use]
    pub const fn powered(&self) -> bool {
        self.status & 0x0100 != 0
    }

    /// The attached device's speed: low (bit 9), high (bit 10), else full.
    #[must_use]
    pub const fn speed(&self) -> Speed {
        if self.status & 0x0200 != 0 {
            Speed::Low
        } else if self.status & 0x0400 != 0 {
            Speed::High
        } else {
            Speed::Full
        }
    }

    /// The connection changed.
    #[must_use]
    pub const fn connection_changed(&self) -> bool {
        self.change & 0x0001 != 0
    }

    /// The hub disabled the port (an error).
    #[must_use]
    pub const fn enable_changed(&self) -> bool {
        self.change & 0x0002 != 0
    }

    /// The over-current state changed.
    #[must_use]
    pub const fn over_current_changed(&self) -> bool {
        self.change & 0x0008 != 0
    }

    /// A reset completed.
    #[must_use]
    pub const fn reset_changed(&self) -> bool {
        self.change & 0x0010 != 0
    }
}

/// The ports a status-change bitmap names (bit 0 is the hub itself, bit n port n), at most
/// `ports`.
pub fn changed_ports(bitmap: &[u8], ports: u8) -> impl Iterator<Item = u8> + '_ {
    (1..=ports.min(MAX_PORTS))
        .filter(move |&p| bitmap.get(usize::from(p / 8)).is_some_and(|b| b & (1 << (p % 8)) != 0))
}
