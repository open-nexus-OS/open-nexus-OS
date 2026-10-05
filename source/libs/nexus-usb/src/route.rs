// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Speeds and route strings. A route string (USB 3.2 §8.9) names a device's path below its
//! root port: one 4-bit hub port per tier, tier 1 in bits 3:0, at most five tiers; a device
//! on a root port has the route 0.

use crate::UsbError;

/// A device's speed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Speed {
    /// 1.5 Mbit/s.
    Low,
    /// 12 Mbit/s.
    Full,
    /// 480 Mbit/s.
    High,
    /// 5 Gbit/s and faster.
    Super,
}

impl Speed {
    /// EP0's max packet before the device descriptor names it: 8 below high speed (the first
    /// eight bytes then name the real one), 64 at high speed, 512 at SuperSpeed.
    #[must_use]
    pub const fn default_max_packet0(self) -> u16 {
        match self {
            Self::Low | Self::Full => 8,
            Self::High => 64,
            Self::Super => 512,
        }
    }

    /// A full- or low-speed device: behind a high-speed hub it needs that hub's transaction
    /// translator.
    #[must_use]
    pub const fn needs_tt(self) -> bool {
        matches!(self, Self::Low | Self::Full)
    }
}

/// A device's path below its root port.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RouteString(u32);

impl RouteString {
    /// A device on a root port.
    pub const ROOT: Self = Self(0);
    /// The deepest tier.
    pub const MAX_DEPTH: u8 = 5;

    /// The route of the device on `port` of the hub this route names.
    pub fn child(self, port: u8) -> Result<Self, UsbError> {
        if port == 0 || port > 15 {
            return Err(UsbError::RoutePort);
        }
        let depth = self.depth();
        if depth >= Self::MAX_DEPTH {
            return Err(UsbError::RouteDepth);
        }
        Ok(Self(self.0 | (u32::from(port) << (4 * depth))))
    }

    /// Hubs between the root port and the device.
    #[must_use]
    pub const fn depth(self) -> u8 {
        let mut depth = 0;
        while depth < Self::MAX_DEPTH && (self.0 >> (4 * depth)) & 0xf != 0 {
            depth += 1;
        }
        depth
    }

    /// The 20-bit value a slot context carries.
    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }
}
