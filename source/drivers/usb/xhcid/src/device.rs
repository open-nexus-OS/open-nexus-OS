// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A device in the table: where it is (route, root port, the hub it hangs off and the TT that
//! serves it), how far its enumeration got ([`Stage`]), its memory — the contexts, the control
//! ring and buffer (`core`), the interrupt rings and report buffers (`pipes_mem`) — the control
//! transfer in flight, its interrupt pipes and, for a hub, the hub's state.

use nexus_driverkit::{CacheOps, DmaMemory};
use nexus_usb::{HidRole, PortFeature, RouteString, SetupPacket, Speed};

use crate::memory::Region;
use crate::ring::Producer;

/// Devices at most (`CONFIG.MaxSlotsEn`).
pub const MAX_DEVICES: usize = 16;
/// Interrupt pipes per device.
pub const MAX_PIPES: usize = 4;
/// TRBs kept queued on an interrupt pipe (a measured mouse reports every millisecond).
pub const PIPE_TRBS: usize = 4;
/// TRBs in an interrupt pipe's ring.
pub const PIPE_RING_TRBS: usize = 16;

/// The core region: device context, input context, EP0's ring, the control buffer.
pub const CORE_LEN: usize = 8192;
pub const OUT_CONTEXT: usize = 0x0000;
pub const IN_CONTEXT: usize = 0x0800;
pub const EP0_RING: usize = 0x1100;
pub const EP0_TRBS: usize = 32;
pub const CONTROL_BUF: usize = 0x1400;
/// The control buffer: the largest configuration descriptor.
pub const CONTROL_MAX: usize = nexus_usb::descriptor::CONFIG_MAX;

/// How far a device got.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// Enable Slot in flight.
    EnableSlot,
    /// Address Device in flight.
    Address,
    /// The first eight bytes of the device descriptor (EP0's max packet).
    Prefix,
    /// Evaluate Context with the real max packet.
    Evaluate,
    /// The device descriptor.
    Device,
    /// The configuration header (its total length).
    ConfigHeader,
    /// The whole configuration.
    Config,
    /// Configure Endpoint with the pipes the device will use.
    Configure,
    /// SET_CONFIGURATION.
    SetConfiguration,
    /// SET_PROTOCOL(boot) on pipe `n`'s interface.
    SetProtocol(u8),
    /// SET_IDLE(0) on pipe `n`'s interface (a STALL is fine).
    SetIdle(u8),
    /// The hub descriptor.
    HubDescriptor,
    /// Configure Endpoint again with the hub's slot fields.
    HubSlot,
    /// SET_PORT_FEATURE(PORT_POWER) on port `n`.
    HubPower(u8),
    /// The ports' power-good time.
    HubPowerGood { until: u64 },
    /// Enumerated; pipes running (a hub serves its port work).
    Running,
    /// Enumerated, nothing v1 drives.
    Unsupported,
    /// Given up.
    Failed,
    /// Gone: its slot is being disabled; its memory lives until the controller let go.
    Detaching,
}

/// Where a halted endpoint's recovery is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryStep {
    /// Reset Endpoint in flight.
    Reset,
    /// Set TR Dequeue Pointer in flight.
    Dequeue,
}

/// A halted endpoint being recovered (one at a time per device).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Recovery {
    pub dci: u8,
    pub step: RecoveryStep,
    /// The completion code that halted it.
    pub failed: u8,
}

/// What an interrupt pipe carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PipeKind {
    /// A HID boot interface's reports.
    Hid { interface: u8, role: HidRole },
    /// A hub's status-change bitmap.
    HubStatus,
}

/// An interrupt-IN pipe: its ring and its report buffers, [`PIPE_TRBS`] TRBs always queued.
#[derive(Clone, Copy, Debug)]
pub struct Pipe {
    pub kind: PipeKind,
    pub endpoint: u8,
    pub dci: u8,
    pub max_packet: u16,
    pub interval: u8,
    pub ring: Producer,
    /// The first report buffer's offset in the pipes region and the distance between buffers.
    pub buffers: usize,
    pub stride: usize,
    /// The queued TRBs' bus addresses, oldest first, and which buffer each fills.
    pub queued: [(u64, u8); PIPE_TRBS],
    pub head: usize,
    /// The next buffer a requeue uses.
    pub next_buffer: u8,
    /// Failed transfers in a row.
    pub errors: u8,
}

/// A control transfer in flight on EP0.
#[derive(Clone, Copy, Debug)]
pub struct Control {
    pub setup: SetupPacket,
    /// The data stage's TRB, if there is one.
    pub data_trb: Option<u64>,
    /// The status stage's TRB (its completion ends the transfer).
    pub status_trb: u64,
    /// Bytes the data stage moved short of the request.
    pub short: u32,
}

/// Work a hub's control pipe does in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HubWork {
    /// GET_STATUS(port).
    Status(u8),
    /// CLEAR_FEATURE(port, feature).
    Clear(u8, PortFeature),
    /// SET_FEATURE(port, feature).
    Set(u8, PortFeature),
}

/// Hub work waiting at most.
const HUB_WORK: usize = 32;

/// A hub's state.
#[derive(Clone, Copy, Debug)]
pub struct Hub {
    pub ports: u8,
    pub ttt: u8,
    pub power_good_ms: u16,
    /// The device on each port (index = port − 1).
    pub children: [Option<usize>; 15],
    work: [Option<HubWork>; HUB_WORK],
    head: usize,
    count: usize,
    /// The work item whose control transfer is in flight.
    pub doing: Option<HubWork>,
}

impl Hub {
    /// A hub with `ports` ports.
    #[must_use]
    pub const fn new(ports: u8, ttt: u8, power_good_ms: u16) -> Self {
        Self {
            ports,
            ttt,
            power_good_ms,
            children: [None; 15],
            work: [None; HUB_WORK],
            head: 0,
            count: 0,
            doing: None,
        }
    }

    /// Queue `work` unless the same item already waits; false when full.
    pub fn push(&mut self, work: HubWork) -> bool {
        let waiting = (0..self.count).any(|i| self.work[(self.head + i) % HUB_WORK] == Some(work));
        if waiting {
            return true;
        }
        if self.count == HUB_WORK {
            return false;
        }
        self.work[(self.head + self.count) % HUB_WORK] = Some(work);
        self.count += 1;
        true
    }

    /// The next work item.
    pub fn pop(&mut self) -> Option<HubWork> {
        while self.count > 0 {
            let item = self.work[self.head].take();
            self.head = (self.head + 1) % HUB_WORK;
            self.count -= 1;
            if item.is_some() {
                return item;
            }
        }
        None
    }
}

/// A device.
pub struct Device<M: DmaMemory, C: CacheOps> {
    /// The controller's slot ID (0 until Enable Slot completes).
    pub slot: u8,
    pub route: RouteString,
    pub root_port: u8,
    pub speed: Speed,
    /// The hub (device index) and port it hangs off.
    pub parent: Option<(usize, u8)>,
    /// The high-speed hub whose transaction translator serves it: (slot, port).
    pub tt: Option<(u8, u8)>,
    pub stage: Stage,
    pub core: Region<M, C>,
    pub pipes_mem: Option<Region<M, C>>,
    pub ep0: Producer,
    pub max_packet0: u16,
    pub control: Option<Control>,
    pub vendor: u16,
    pub product: u16,
    pub class: u8,
    pub config_value: u8,
    pub pipes: [Option<Pipe>; MAX_PIPES],
    pub hub: Option<Hub>,
    /// A halted endpoint being recovered.
    pub recovery: Option<Recovery>,
    /// Disable Slot is in flight (a detached device).
    pub disabling: bool,
}
