// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The devices on the model's ports: HID boot devices and hubs, answering the standard, HID and
//! hub class requests from their descriptor bytes, holding the reports a test injects and a
//! hub's port changes until an interrupt TRB takes them.

use std::collections::VecDeque;

use nexus_usb::{SetupPacket, Speed};

/// What a control request got.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    /// Data for an IN request (truncated to the request by the controller).
    Data(Vec<u8>),
    /// Done (no data stage).
    Done,
    /// The device STALLs the request.
    Stall,
}

/// A HID device.
#[derive(Clone, Debug)]
pub struct Hid {
    pub speed: Speed,
    /// Device descriptor (18 bytes) followed by the configuration.
    pub descriptors: Vec<u8>,
    /// Interfaces that STALL SET_IDLE (a measured mouse does).
    pub stall_set_idle: Vec<u8>,
    /// SET_PROTOCOL as received: (interface, protocol).
    pub protocols: Vec<(u8, u16)>,
    pub configured: u8,
    /// Reports waiting per endpoint address.
    pub reports: VecDeque<(u8, Vec<u8>)>,
}

/// A hub's downstream port.
#[derive(Clone, Debug, Default)]
pub struct HubPort {
    pub device: Option<Box<Dev>>,
    pub powered: bool,
    pub enabled: bool,
    pub c_connection: bool,
    pub c_reset: bool,
    /// A change the status pipe has not reported yet.
    pub notify: bool,
}

/// A hub.
#[derive(Clone, Debug)]
pub struct Hub {
    pub speed: Speed,
    pub descriptors: Vec<u8>,
    pub hub_descriptor: Vec<u8>,
    pub ports: Vec<HubPort>,
    pub configured: u8,
}

/// A device.
#[derive(Clone, Debug)]
pub enum Dev {
    Hid(Hid),
    Hub(Hub),
}

impl Dev {
    /// The device's speed.
    #[must_use]
    pub fn speed(&self) -> Speed {
        match self {
            Dev::Hid(h) => h.speed,
            Dev::Hub(h) => h.speed,
        }
    }

    /// EP0's max packet (`bMaxPacketSize0`).
    #[must_use]
    pub fn max_packet0(&self) -> u16 {
        u16::from(self.descriptors()[7])
    }

    fn descriptors(&self) -> &[u8] {
        match self {
            Dev::Hid(h) => &h.descriptors,
            Dev::Hub(h) => &h.descriptors,
        }
    }

    /// The endpoints the configuration declares: (address, max packet, bInterval).
    #[must_use]
    pub fn endpoints(&self) -> Vec<(u8, u16, u8)> {
        let d = self.descriptors();
        let mut out = Vec::new();
        let mut at = 18;
        while at + 1 < d.len() {
            let len = usize::from(d[at]);
            if len < 2 {
                break;
            }
            if d[at + 1] == 5 && at + 6 < d.len() {
                let max_packet = u16::from_le_bytes([d[at + 4], d[at + 5]]) & 0x7ff;
                out.push((d[at + 2], max_packet, d[at + 6]));
            }
            at += len;
        }
        out
    }

    /// A control request.
    pub fn control(&mut self, setup: SetupPacket) -> Reply {
        let (kind, index) = ((setup.value >> 8) as u8, setup.value as u8);
        match (setup.request_type, setup.request) {
            (0x80, 6) if kind == 1 => Reply::Data(self.descriptors()[..18].to_vec()),
            (0x80, 6) if kind == 2 && index == 0 => Reply::Data(self.descriptors()[18..].to_vec()),
            (0x00, 9) => {
                match self {
                    Dev::Hid(h) => h.configured = setup.value as u8,
                    Dev::Hub(h) => h.configured = setup.value as u8,
                }
                Reply::Done
            }
            (0x01, 11) => Reply::Done,
            _ => match self {
                Dev::Hid(h) => h.class_request(setup),
                Dev::Hub(h) => h.class_request(setup),
            },
        }
    }

    /// A report for interrupt endpoint `address`, if one waits.
    pub fn take_report(&mut self, address: u8) -> Option<Vec<u8>> {
        match self {
            Dev::Hid(h) => {
                let at = h.reports.iter().position(|(ep, _)| *ep == address)?;
                h.reports.remove(at).map(|(_, bytes)| bytes)
            }
            Dev::Hub(h) => h.take_status(),
        }
    }
}

impl Hid {
    fn class_request(&mut self, setup: SetupPacket) -> Reply {
        let interface = setup.index as u8;
        match (setup.request_type, setup.request) {
            (0x21, 0x0b) => {
                self.protocols.push((interface, setup.value));
                Reply::Done
            }
            (0x21, 0x0a) if self.stall_set_idle.contains(&interface) => Reply::Stall,
            (0x21, 0x0a) => Reply::Done,
            _ => Reply::Stall,
        }
    }
}

impl Hub {
    fn class_request(&mut self, setup: SetupPacket) -> Reply {
        let port = usize::from(setup.index).wrapping_sub(1);
        match (setup.request_type, setup.request) {
            (0xa0, 6) if setup.value >> 8 == 0x29 => Reply::Data(self.hub_descriptor.clone()),
            (0xa3, 0) => {
                let Some(p) = self.ports.get(port) else { return Reply::Stall };
                let speed_bits = match p.device.as_ref().map(|d| d.speed()) {
                    Some(Speed::Low) => 0x0200,
                    Some(Speed::High) => 0x0400,
                    _ => 0,
                };
                let connected = p.powered && p.device.is_some();
                let status = u16::from(connected)
                    | (u16::from(p.enabled) << 1)
                    | (u16::from(p.powered) << 8)
                    | if connected { speed_bits } else { 0 };
                let change = u16::from(p.c_connection) | (u16::from(p.c_reset) << 4);
                let mut out = status.to_le_bytes().to_vec();
                out.extend_from_slice(&change.to_le_bytes());
                Reply::Data(out)
            }
            (0x23, 3) => {
                let Some(p) = self.ports.get_mut(port) else { return Reply::Stall };
                match setup.value {
                    8 => {
                        if !p.powered {
                            p.powered = true;
                            if p.device.is_some() {
                                p.c_connection = true;
                                p.notify = true;
                            }
                        }
                    }
                    4 => {
                        if p.powered && p.device.is_some() {
                            p.enabled = true;
                            p.c_reset = true;
                            p.notify = true;
                        }
                    }
                    _ => {}
                }
                Reply::Done
            }
            (0x23, 1) => {
                let Some(p) = self.ports.get_mut(port) else { return Reply::Stall };
                match setup.value {
                    16 => p.c_connection = false,
                    20 => p.c_reset = false,
                    _ => {}
                }
                Reply::Done
            }
            _ => Reply::Stall,
        }
    }

    /// The status-change bitmap, once per new change.
    fn take_status(&mut self) -> Option<Vec<u8>> {
        let mut bitmap = vec![0u8; (self.ports.len() + 1).div_ceil(8)];
        let mut any = false;
        for (i, p) in self.ports.iter_mut().enumerate() {
            if std::mem::take(&mut p.notify) {
                let bit = i + 1;
                bitmap[bit / 8] |= 1 << (bit % 8);
                any = true;
            }
        }
        any.then_some(bitmap)
    }

    /// Plug `device` into port `port` (1-based).
    pub fn plug(&mut self, port: u8, device: Dev) {
        let p = &mut self.ports[usize::from(port) - 1];
        p.device = Some(Box::new(device));
        if p.powered {
            p.c_connection = true;
            p.notify = true;
        }
    }

    /// Unplug port `port`.
    pub fn unplug(&mut self, port: u8) {
        let p = &mut self.ports[usize::from(port) - 1];
        p.device = None;
        p.enabled = false;
        p.c_connection = true;
        p.notify = true;
    }
}

/// A HID boot device's descriptors: one interface per `(protocol, max packet)` (1 keyboard,
/// 2 mouse), endpoint 0x81, 0x82, … at a 10 ms interval — the shape QEMU's devices have.
#[must_use]
pub fn hid_descriptors(vendor: u16, product: u16, interfaces: &[(u8, u16)]) -> Vec<u8> {
    let mut d = vec![0x12, 0x01, 0x00, 0x02, 0, 0, 0, 8];
    d.extend_from_slice(&vendor.to_le_bytes());
    d.extend_from_slice(&product.to_le_bytes());
    d.extend_from_slice(&[0x00, 0x01, 1, 2, 0, 1]);
    let mut body = Vec::new();
    for (i, &(protocol, max_packet)) in interfaces.iter().enumerate() {
        body.extend_from_slice(&[0x09, 0x04, i as u8, 0, 1, 3, 1, protocol, 0]);
        body.extend_from_slice(&[0x09, 0x21, 0x11, 0x01, 0, 1, 0x22, 0x3f, 0]);
        let mp = max_packet.to_le_bytes();
        body.extend_from_slice(&[0x07, 0x05, 0x81 + i as u8, 0x03, mp[0], mp[1], 10]);
    }
    let total = (9 + body.len()) as u16;
    let t = total.to_le_bytes();
    d.extend_from_slice(&[0x09, 0x02, t[0], t[1], interfaces.len() as u8, 1, 0, 0xa0, 50]);
    d.extend_from_slice(&body);
    d
}

/// A hub's descriptors (device + configuration, class descriptor): `ports` ports at `speed`,
/// TT think time `ttt` (0..=3) for a high-speed hub.
#[must_use]
pub fn hub_descriptors(speed: Speed, ports: u8, ttt: u8) -> (Vec<u8>, Vec<u8>) {
    let (usb, protocol, mp0) = match speed {
        Speed::High => (0x0200u16, 1, 64),
        _ => (0x0110, 0, 8),
    };
    let u = usb.to_le_bytes();
    let mut d = vec![0x12, 0x01, u[0], u[1], 9, 0, protocol, mp0, 0x09, 0x04, 0xaa, 0x55];
    d.extend_from_slice(&[0x01, 0x01, 0, 0, 0, 1]);
    d.extend_from_slice(&[0x09, 0x02, 25, 0, 1, 1, 0, 0xe0, 0]);
    d.extend_from_slice(&[0x09, 0x04, 0, 0, 1, 9, 0, 0, 0]);
    let status_len = (u16::from(ports) + 1).div_ceil(8);
    let interval = if speed == Speed::High { 12 } else { 255 };
    let s = status_len.to_le_bytes();
    d.extend_from_slice(&[0x07, 0x05, 0x81, 0x03, s[0], s[1], interval]);
    // QEMU's layout: `DeviceRemovable` a bit per port plus bit 0, `PortPwrCtrlMask` a bit per
    // port (10 bytes for 8 ports, as the `usb` lane read it).
    let removable = usize::from(ports + 1).div_ceil(8);
    let power_mask = usize::from(ports).div_ceil(8);
    let characteristics = 0x0009u16 | (u16::from(ttt) << 5);
    let c = characteristics.to_le_bytes();
    let mut h = vec![(7 + removable + power_mask) as u8, 0x29, ports, c[0], c[1], 10, 0];
    h.extend(std::iter::repeat_n(0u8, removable));
    h.extend(std::iter::repeat_n(0xffu8, power_mask));
    (d, h)
}

/// A hub device with `ports` empty ports.
#[must_use]
pub fn hub(speed: Speed, ports: u8, ttt: u8) -> Hub {
    let (descriptors, hub_descriptor) = hub_descriptors(speed, ports, ttt);
    Hub {
        speed,
        descriptors,
        hub_descriptor,
        ports: vec![HubPort::default(); usize::from(ports)],
        configured: 0,
    }
}

/// A HID device from descriptor bytes.
#[must_use]
pub fn hid(speed: Speed, descriptors: Vec<u8>) -> Hid {
    Hid {
        speed,
        descriptors,
        stall_set_idle: Vec::new(),
        protocols: Vec::new(),
        configured: 0,
        reports: VecDeque::new(),
    }
}
