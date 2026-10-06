// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! What the driver tells its host (the OS loop prints markers and hands the HID notes to the
//! class server, `hid_class`; a test records everything): one [`Note`] per thing that
//! happened, through a [`Sink`] — the core never prints, allocates or blocks.

use nexus_usb::{HidRole, Speed};

/// The step a failure happened in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// The controller did not become ready, halt, reset or run in time.
    Controller,
    /// The controller's memory could not be made.
    Memory,
    /// A root or hub port did not finish its reset.
    PortReset,
    /// Enable Slot.
    EnableSlot,
    /// Address Device.
    Address,
    /// A descriptor was refused or could not be read.
    Descriptor,
    /// Evaluate Context / Configure Endpoint.
    Configure,
    /// A control request.
    Control,
    /// An interrupt endpoint kept failing.
    Interrupt,
    /// Too many devices for the table.
    Devices,
}

impl Step {
    /// The name the marker prints.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Controller => "controller",
            Self::Memory => "memory",
            Self::PortReset => "port-reset",
            Self::EnableSlot => "enable-slot",
            Self::Address => "address",
            Self::Descriptor => "descriptor",
            Self::Configure => "configure",
            Self::Control => "control",
            Self::Interrupt => "interrupt",
            Self::Devices => "devices",
        }
    }
}

/// One thing that happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note<'a> {
    /// The controller runs.
    ControllerOk { version: u16, ports: u8, slots: u8, context_size: u8, scratchpads: u16 },
    /// Every root port is powered; enumeration begins.
    Ready { ports: u8, connected: u8 },
    /// A root port runs a USB 3 link v1 leaves alone.
    SuperSpeedPort { port: u8 },
    /// A hub is configured and its ports are powered.
    Hub { slot: u8, root_port: u8, speed: Speed, ports: u8, ttt: u8 },
    /// A device answered its descriptors.
    Enumerated { slot: u8, route: u32, speed: Speed, vendor: u16, product: u16, class: u8 },
    /// A HID boot interface reports (its device's identity along, for the class's client).
    HidInterface {
        slot: u8,
        interface: u8,
        role: HidRole,
        endpoint: u8,
        max_packet: u16,
        interval: u8,
        vendor: u16,
        product: u16,
    },
    /// A boot report as the controller delivered it (unparsed).
    Report { slot: u8, interface: u8, role: HidRole, bytes: &'a [u8] },
    /// A HID interface's pipe was given up (it kept failing): the interface reports no more,
    /// though its device stays (the [`Note::Fail`] before it names the step and the code).
    HidLost { slot: u8, interface: u8 },
    /// A device went away.
    Detached { slot: u8 },
    /// A controller wait ran out (just before the [`Note::Fail`]): which one (1 ready, 2 halt,
    /// 3 reset, 4 start) and the command and status words it read last.
    ControllerStuck { phase: u8, usbcmd: u32, usbsts: u32 },
    /// Something failed: the step and the completion code (or 0).
    Fail { step: Step, code: u8 },
}

/// Where notes go.
pub trait Sink {
    /// One note.
    fn note(&mut self, note: Note<'_>);
}
