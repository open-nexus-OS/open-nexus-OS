// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: hidrawd's USB source (TASK-0253B, RFC-0099 §5): the frames xhcid pushes on the
//! subscription's channel — its answer, an attach per HID boot interface, each drain's
//! reports, a detach — turned into normalized events per device for the one batch path. The
//! frames are untrusted input, bounded before use: only xhcid's (the kernel-attributed sender,
//! never a payload), only interfaces it attached, every report through the one parser
//! (`HidrawdService` over `userspace/hid`) — a refused report is counted, never half-parsed —
//! and a detach releases what the device held, so nothing stays pressed. Pure: the OS loop
//! receives the frames (`os_lite`), the host tests hand them in.
//! OWNERS: @runtime @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `tests/usb_source.rs` (xhcid's frames as goldens, the rejects, the release)
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

extern crate alloc;

use alloc::vec::Vec;
use hid::{HidEvent, TimestampNs};
use nexus_wire::usb as wire;

use crate::source::{DeviceFrame, Emit};
use crate::{DeviceId, HidDeviceKind, HidrawdService, PointerSource};

/// USB HID interfaces held at once at most (xhcid attaches no more than this).
pub const MAX_USB_DEVICES: usize = 16;
/// USB interfaces are `USB_DEVICE_BASE + n` on the wire — apart from the virtio devices (1..=3).
pub const USB_DEVICE_BASE: u16 = 0x100;

/// An attached USB HID boot interface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UsbDevice {
    /// xhcid's name for the attachment.
    pub attachment: u16,
    /// Its name on the wire to inputd.
    pub device: DeviceId,
    pub vendor: u16,
    pub product: u16,
    pub kind: HidDeviceKind,
}

/// What a frame said.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Heard {
    /// xhcid admitted the subscription.
    Subscribed,
    /// xhcid refused it (`nexus_wire::usb::STATUS_*`).
    Refused(u8),
    /// An interface attached.
    Attached(UsbDevice),
    /// A drain's reports of an interface: how many the parser took and refused.
    Reports { device: UsbDevice, parsed: u8, refused: u8 },
    /// An interface went away; what it held was released.
    Detached(UsbDevice),
}

/// Why a frame was refused (the caller counts it; nothing of it was used).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rejected {
    /// Not from xhcid.
    Foreign,
    /// Not a frame of the class wire, or one that does not add up.
    Malformed,
    /// Reports or a detach for an interface xhcid never attached.
    UnknownDevice,
    /// An attach with a role the boot protocol does not have.
    Role,
    /// An attach past the table's bound.
    Full,
}

/// The USB HID interfaces xhcid attached, their parsers (in the device table) and one reused
/// event buffer.
pub struct UsbHid {
    xhcid: u64,
    table: HidrawdService,
    devices: [Option<UsbDevice>; MAX_USB_DEVICES],
    events: Vec<HidEvent>,
}

impl UsbHid {
    /// Frames are taken from the service whose kernel identity is `xhcid` and no one else.
    #[must_use]
    pub fn new(xhcid: u64) -> Self {
        Self {
            xhcid,
            table: HidrawdService::new(),
            devices: [None; MAX_USB_DEVICES],
            events: Vec::with_capacity(64),
        }
    }

    /// Interfaces attached.
    #[must_use]
    pub fn attached(&self) -> usize {
        self.devices.iter().flatten().count()
    }

    /// One frame from the push channel, `sender` as the kernel attributed it.
    pub fn on_frame(
        &mut self,
        sender: u64,
        frame: &[u8],
        now: TimestampNs,
        emit: &mut dyn Emit,
    ) -> Result<Heard, Rejected> {
        if sender != self.xhcid {
            return Err(Rejected::Foreign);
        }
        match wire::frame_op(frame).ok_or(Rejected::Malformed)? {
            wire::OP_SUBSCRIBED => match wire::decode_subscribed(frame) {
                Some(wire::STATUS_OK) => Ok(Heard::Subscribed),
                Some(status) => Ok(Heard::Refused(status)),
                None => Err(Rejected::Malformed),
            },
            wire::OP_DEVICE_ATTACHED => {
                let (attachment, vendor, product, _interface, role, _max_packet) =
                    wire::decode_attached(frame).ok_or(Rejected::Malformed)?;
                let kind = match role {
                    wire::ROLE_KEYBOARD => HidDeviceKind::Keyboard,
                    wire::ROLE_MOUSE => HidDeviceKind::Mouse,
                    _ => return Err(Rejected::Role),
                };
                // xhcid never names two live interfaces alike: a second attach of a name
                // replaces the first, which is released.
                let _ = self.detach(attachment, now, emit);
                self.attach(attachment, vendor, product, kind).map(Heard::Attached)
            }
            wire::OP_HID_REPORTS => {
                let (attachment, count, list) =
                    wire::decode_reports(frame).ok_or(Rejected::Malformed)?;
                let reports = wire::reports(count, list).ok_or(Rejected::Malformed)?;
                let device = self.find(attachment).ok_or(Rejected::UnknownDevice)?;
                self.events.clear();
                let (mut parsed, mut refused) = (0u8, 0u8);
                for report in reports {
                    match self.table.ingest_report_into(
                        device.device,
                        now,
                        report,
                        &mut self.events,
                    ) {
                        Ok(_) => parsed = parsed.saturating_add(1),
                        Err(_) => refused = refused.saturating_add(1),
                    }
                }
                emit.emit(&frame_of(&device), u16::from(count), &self.events);
                Ok(Heard::Reports { device, parsed, refused })
            }
            wire::OP_DEVICE_DETACHED => {
                let attachment = wire::decode_detached(frame).ok_or(Rejected::Malformed)?;
                self.detach(attachment, now, emit)
                    .map(Heard::Detached)
                    .ok_or(Rejected::UnknownDevice)
            }
            _ => Err(Rejected::Malformed),
        }
    }

    fn attach(
        &mut self,
        attachment: u16,
        vendor: u16,
        product: u16,
        kind: HidDeviceKind,
    ) -> Result<UsbDevice, Rejected> {
        let free = self.devices.iter().position(Option::is_none).ok_or(Rejected::Full)?;
        let device = DeviceId::new(USB_DEVICE_BASE + free as u16);
        let registered = match kind {
            HidDeviceKind::Keyboard => self.table.register_keyboard(device),
            HidDeviceKind::Mouse => self.table.register_mouse(device),
        };
        if !registered {
            return Err(Rejected::Full);
        }
        let attached = UsbDevice { attachment, device, vendor, product, kind };
        self.devices[free] = Some(attached);
        Ok(attached)
    }

    /// The interface leaves the table; what it held is released through the batch path.
    fn detach(
        &mut self,
        attachment: u16,
        now: TimestampNs,
        emit: &mut dyn Emit,
    ) -> Option<UsbDevice> {
        let slot =
            self.devices.iter().position(|d| d.is_some_and(|d| d.attachment == attachment))?;
        let gone = self.devices[slot].take()?;
        self.events.clear();
        let _ = self.table.release_into(gone.device, now, &mut self.events);
        if !self.events.is_empty() {
            emit.emit(&frame_of(&gone), 0, &self.events);
        }
        Some(gone)
    }

    fn find(&self, attachment: u16) -> Option<UsbDevice> {
        self.devices.iter().flatten().find(|d| d.attachment == attachment).copied()
    }
}

fn frame_of(device: &UsbDevice) -> DeviceFrame {
    DeviceFrame {
        device: device.device,
        pointer: match device.kind {
            HidDeviceKind::Keyboard => None,
            HidDeviceKind::Mouse => Some(PointerSource::MouseRelative),
        },
        abs_max_x: 0,
        abs_max_y: 0,
    }
}
