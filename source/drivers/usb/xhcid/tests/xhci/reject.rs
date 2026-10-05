// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use nexus_usb::Speed;
use xhcid::Step;
use xhcid_model::dev::{self, Dev};
use xhcid_model::{machine, Config, Rig};

/// A device with the given descriptor bytes alone on root port 1.
fn alone(descriptors: Vec<u8>) -> Rig {
    let m = machine(Config::qemu());
    m.borrow_mut().plug(1, Dev::Hid(dev::hid(Speed::Full, descriptors)));
    let mut rig = Rig::new(&m);
    rig.start();
    rig
}

fn keyboard() -> Vec<u8> {
    dev::hid_descriptors(0x0627, 0x0001, &[(1, 8)])
}

#[test]
fn test_reject_a_configuration_past_its_bound() {
    let mut d = keyboard();
    d[18 + 2..18 + 4].copy_from_slice(&2000u16.to_le_bytes());
    let rig = alone(d);
    assert_eq!(rig.sink.fails, [(Step::Descriptor, nexus_usb::UsbError::TotalLength as u8)]);
    assert!(rig.sink.notes("Enumerated").is_empty());
}

/// The configuration header names more bytes than the device then sends: the data stage ends
/// short, and only what arrived is parsed — never the buffer's older bytes behind it.
#[test]
fn test_reject_a_configuration_shorter_than_it_claims() {
    let mut d = keyboard();
    let total = u16::from_le_bytes([d[18 + 2], d[18 + 3]]);
    d[18 + 2..18 + 4].copy_from_slice(&(total + 9).to_le_bytes());
    let rig = alone(d);
    assert_eq!(rig.sink.fails, [(Step::Descriptor, nexus_usb::UsbError::Truncated as u8)]);
}

#[test]
fn test_reject_an_ep0_max_packet_that_is_no_size() {
    let mut d = keyboard();
    d[7] = 7;
    let rig = alone(d);
    assert_eq!(rig.sink.fails, [(Step::Descriptor, nexus_usb::UsbError::MaxPacket0 as u8)]);
}

#[test]
fn test_reject_a_hub_without_ports() {
    let m = machine(Config::qemu());
    let mut hub = dev::hub(Speed::Full, 1, 0);
    hub.hub_descriptor[2] = 0;
    m.borrow_mut().plug(1, Dev::Hub(hub));
    let mut rig = Rig::new(&m);
    rig.start();
    assert_eq!(rig.sink.fails, [(Step::Descriptor, nexus_usb::UsbError::HubPorts as u8)]);
    assert!(rig.sink.notes("Hub {").is_empty());
}

#[test]
fn test_reject_a_babbling_pipe_after_bounded_recoveries() {
    let m = machine(Config::qemu());
    m.borrow_mut().plug(1, Dev::Hid(dev::hid(Speed::Full, keyboard())));
    let mut rig = Rig::new(&m);
    rig.start();
    // Longer than the max packet (8): the controller halts the pipe; the driver recovers it.
    rig.report(1, 0, 0x81, &[0u8; 9]);
    rig.report(1, 0, 0x81, &[0, 0, 5, 0, 0, 0, 0, 0]);
    assert_eq!(rig.sink.reports().len(), 1, "recovered: the next report arrives");
    assert!(rig.sink.fails.is_empty());
    for _ in 0..4 {
        rig.report(1, 0, 0x81, &[0u8; 9]);
    }
    assert_eq!(rig.sink.fails.len(), 1);
    assert_eq!(rig.sink.fails[0].0, Step::Interrupt);
}
