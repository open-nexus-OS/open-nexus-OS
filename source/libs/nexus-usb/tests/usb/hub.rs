// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use nexus_usb::hub::changed_ports;
use nexus_usb::{HubDescriptor, PortStatus, Speed, UsbError};

use super::HUB_CLASS;

#[test]
fn the_desk_hub_descriptor_as_measured() {
    let h = HubDescriptor::parse(HUB_CLASS).expect("hub descriptor");
    assert_eq!(
        (h.ports, h.characteristics, h.power_good_ms, h.controller_current_ma),
        (5, 0xe9, 350, 100)
    );
    assert_eq!(h.tt_think_time(), 3, "32 full-speed bit times");
    assert!(h.per_port_power());
}

/// QEMU's `usb-hub` (8 ports): `DeviceRemovable` two bytes, `PortPwrCtrlMask` one — ten bytes,
/// as the `usb` lane read them (2026-10-04). A parser that wanted both bitmaps at the first one's
/// size refused it.
#[test]
fn qemus_hub_descriptor_as_measured() {
    let qemu = [0x0a, 0x29, 0x08, 0x0a, 0x00, 0x01, 0x00, 0x00, 0x00, 0xff];
    let h = HubDescriptor::parse(&qemu).expect("QEMU's hub descriptor");
    assert_eq!((h.ports, h.power_good_ms, h.tt_think_time()), (8, 2, 0));
    // Nine bytes cannot hold both bitmaps of eight ports.
    let mut short = qemu;
    short[0] = 9;
    assert_eq!(HubDescriptor::parse(&short[..9]), Err(UsbError::Length));
}

#[test]
fn port_status_and_change_bits() {
    // Connected, enabled, powered, full speed; the connection changed.
    let s = PortStatus::parse(&[0x03, 0x01, 0x01, 0x00]).expect("status");
    assert!(s.connected() && s.enabled() && s.powered() && !s.resetting());
    assert_eq!(s.speed(), Speed::Full);
    assert!(s.connection_changed() && !s.reset_changed());
    // High speed, a reset completed (port 5 of the desk's hub as `lsusb -v` read it: 0x0503).
    let s = PortStatus::parse(&[0x03, 0x05, 0x10, 0x00]).expect("status");
    assert_eq!(s.speed(), Speed::High);
    assert!(s.reset_changed() && !s.connection_changed());
    let s = PortStatus::parse(&[0x03, 0x03, 0x0a, 0x00]).expect("status");
    assert_eq!(s.speed(), Speed::Low);
    assert!(s.enable_changed() && s.over_current_changed());
}

#[test]
fn changed_ports_from_the_bitmap() {
    // Ports 2 and 3 changed (the keyboard and the receiver plugged in), bit 0 the hub itself.
    assert_eq!(changed_ports(&[0b0000_1101], 5).collect::<Vec<_>>(), vec![2, 3]);
    assert_eq!(changed_ports(&[0b0000_0001], 5).count(), 0);
    // A bit past the hub's ports names nothing; a short bitmap names no port beyond it.
    assert_eq!(changed_ports(&[0b1100_0000], 5).count(), 0);
    assert_eq!(changed_ports(&[0x00, 0x81], 15).collect::<Vec<_>>(), vec![8, 15]);
    assert_eq!(changed_ports(&[0xff], 15).collect::<Vec<_>>(), vec![1, 2, 3, 4, 5, 6, 7]);
}

#[test]
fn test_reject_hub_descriptor() {
    assert_eq!(HubDescriptor::parse(&HUB_CLASS[..6]), Err(UsbError::Truncated));
    assert_eq!(HubDescriptor::parse(&HUB_CLASS[..8]), Err(UsbError::Truncated), "bLength 9");
    for ports in [0u8, 16, 255] {
        let mut h = HUB_CLASS.to_vec();
        h[2] = ports;
        assert_eq!(HubDescriptor::parse(&h), Err(UsbError::HubPorts), "{ports} ports");
    }
    let mut h = HUB_CLASS.to_vec();
    h[1] = 0x2a;
    assert_eq!(HubDescriptor::parse(&h), Err(UsbError::Type));
    // Five ports need two one-byte bitmaps: a descriptor of eight bytes is short.
    let mut h = HUB_CLASS.to_vec();
    h[0] = 8;
    assert_eq!(HubDescriptor::parse(&h), Err(UsbError::Length));
    assert_eq!(PortStatus::parse(&[0x03, 0x01, 0x01]), Err(UsbError::Truncated));
}
