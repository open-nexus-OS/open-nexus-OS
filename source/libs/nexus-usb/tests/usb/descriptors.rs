// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use nexus_usb::descriptor::{max_packet0, CONFIG_MAX};
use nexus_usb::{Configuration, DeviceDescriptor, HidRole, Interface, Transfer, UsbError};

use super::{config, HUB, KEYBOARD, RECEIVER};

fn interfaces(cfg: &Configuration<'_>) -> Vec<(u8, u8, u8, u8, u8)> {
    cfg.interface_list().map(|i| (i.number, i.alternate, i.class, i.subclass, i.protocol)).collect()
}

fn endpoints(i: &Interface<'_>) -> Vec<(u8, Transfer, u16, u8)> {
    i.endpoints().map(|e| (e.address, e.transfer(), e.max_packet_size(), e.interval)).collect()
}

#[test]
fn the_desk_keyboard_as_measured() {
    let d = DeviceDescriptor::parse(KEYBOARD).expect("device");
    assert_eq!((d.vendor, d.product, d.usb, d.max_packet0), (0x3434, 0x0123, 0x0200, 64));
    assert_eq!((d.class, d.configurations), (0, 1));
    assert_eq!(Configuration::total_length(config(KEYBOARD)), Ok(91));
    let cfg = Configuration::parse(config(KEYBOARD)).expect("configuration");
    assert_eq!((cfg.value, cfg.interfaces, cfg.max_power), (1, 3, 0xfa));
    assert_eq!(interfaces(&cfg), vec![(0, 0, 3, 1, 1), (1, 0, 3, 0, 0), (2, 0, 3, 0, 0)]);
    let list: Vec<Interface<'_>> = cfg.interface_list().collect();
    assert_eq!(list[0].hid_role(), Some(HidRole::Keyboard));
    assert_eq!(endpoints(&list[0]), vec![(0x81, Transfer::Interrupt, 8, 1)]);
    assert_eq!(
        endpoints(&list[1]),
        vec![(0x82, Transfer::Interrupt, 32, 1), (0x03, Transfer::Interrupt, 32, 1)]
    );
    assert_eq!(list[1].hid_role(), None);
    assert_eq!(list[1].interrupt_in().map(|e| e.address), Some(0x82));
}

#[test]
fn the_desk_receiver_as_measured() {
    let d = DeviceDescriptor::parse(RECEIVER).expect("device");
    assert_eq!((d.vendor, d.product, d.max_packet0, d.release), (0x046d, 0xc53f, 32, 0x4401));
    let cfg = Configuration::parse(config(RECEIVER)).expect("configuration");
    let roles: Vec<_> =
        cfg.interface_list().map(|i| (i.number, i.hid_role(), i.interrupt_in())).collect();
    assert_eq!(roles.len(), 3);
    assert_eq!((roles[0].0, roles[0].1), (0, Some(HidRole::Keyboard)));
    assert_eq!(roles[0].2.map(|e| (e.address, e.max_packet_size())), Some((0x81, 12)));
    assert_eq!((roles[1].0, roles[1].1), (1, Some(HidRole::Mouse)));
    let mouse = roles[1].2.expect("the mouse's report pipe");
    assert_eq!((mouse.address, mouse.number(), mouse.is_in()), (0x82, 2, true));
    assert_eq!((mouse.max_packet_size(), mouse.interval, mouse.extra_transactions()), (32, 1, 0));
    assert_eq!(roles[2].1, None);
}

#[test]
fn the_desk_hub_as_measured() {
    let d = DeviceDescriptor::parse(HUB).expect("device");
    assert_eq!((d.vendor, d.product, d.class, d.protocol), (0x2109, 0x2817, 9, 2));
    assert_eq!((d.usb, d.max_packet0), (0x0210, 64));
    let cfg = Configuration::parse(config(HUB)).expect("configuration");
    // Alternate 0 single TT (protocol 1), alternate 1 a TT per port (protocol 2).
    assert_eq!(interfaces(&cfg), vec![(0, 0, 9, 0, 1), (0, 1, 9, 0, 2)]);
    for i in cfg.interface_list() {
        assert_eq!(endpoints(&i), vec![(0x81, Transfer::Interrupt, 1, 12)]);
    }
}

#[test]
fn ep0_max_packet_from_the_first_eight_bytes() {
    assert_eq!(max_packet0(&RECEIVER[..8]), Ok(32));
    assert_eq!(max_packet0(&KEYBOARD[..8]), Ok(64));
    let mut usb3 = [0x12, 0x01, 0x00, 0x03, 0, 0, 0, 9];
    assert_eq!(max_packet0(&usb3), Ok(512), "an exponent on a USB 3 device");
    usb3[3] = 0x02;
    assert_eq!(max_packet0(&usb3), Err(UsbError::MaxPacket0), "9 is no size below USB 3");
}

#[test]
fn test_reject_device_descriptor_truncated_or_mislabelled() {
    assert_eq!(DeviceDescriptor::parse(&KEYBOARD[..17]), Err(UsbError::Truncated));
    assert_eq!(max_packet0(&KEYBOARD[..7]), Err(UsbError::Truncated));
    let mut d = KEYBOARD[..18].to_vec();
    d[0] = 17;
    assert_eq!(DeviceDescriptor::parse(&d), Err(UsbError::Length));
    d[0] = 18;
    d[1] = 2;
    assert_eq!(DeviceDescriptor::parse(&d), Err(UsbError::Type));
    d[1] = 1;
    for bad in [0u8, 7, 63, 65, 255] {
        d[7] = bad;
        assert_eq!(DeviceDescriptor::parse(&d), Err(UsbError::MaxPacket0), "{bad}");
    }
}

#[test]
fn test_reject_configuration_total_length() {
    let mut c = config(KEYBOARD).to_vec();
    c[2] = 8;
    assert_eq!(Configuration::parse(&c), Err(UsbError::TotalLength));
    let too_long = (CONFIG_MAX + 1) as u16;
    c[2..4].copy_from_slice(&too_long.to_le_bytes());
    assert_eq!(Configuration::total_length(&c), Err(UsbError::TotalLength));
    // The header names more bytes than arrived.
    let c = &config(KEYBOARD)[..90];
    assert_eq!(Configuration::parse(c), Err(UsbError::Truncated));
    let mut h = config(KEYBOARD).to_vec();
    h[1] = 1;
    assert_eq!(Configuration::parse(&h), Err(UsbError::Type));
    h[1] = 2;
    h[0] = 8;
    assert_eq!(Configuration::parse(&h), Err(UsbError::Length));
}

#[test]
fn test_reject_descriptor_running_past_the_configuration() {
    // The keyboard's last endpoint (7 bytes at offset 84) claims 8: it would run past.
    let mut c = config(KEYBOARD).to_vec();
    assert_eq!(c[84..86], [0x07, 0x05]);
    c[84] = 8;
    assert_eq!(Configuration::parse(&c), Err(UsbError::Length));
    // A descriptor of length 0 or 1 would never advance the walk.
    for len in [0u8, 1] {
        let mut c = config(KEYBOARD).to_vec();
        c[84] = len;
        assert_eq!(Configuration::parse(&c), Err(UsbError::Length), "bLength {len}");
    }
    // An endpoint or interface shorter than its type.
    let mut c = config(KEYBOARD).to_vec();
    c[27] = 6;
    assert_eq!(Configuration::parse(&c), Err(UsbError::Length), "a 6-byte endpoint");
}

#[test]
fn test_reject_too_many_interfaces_or_endpoints() {
    let mut c = config(KEYBOARD).to_vec();
    c[4] = 9;
    assert_eq!(Configuration::parse(&c), Err(UsbError::Interfaces), "bNumInterfaces 9");
    let mut c = config(KEYBOARD).to_vec();
    assert_eq!(c[9 + 4], 1, "interface 0's bNumEndpoints");
    c[9 + 4] = 5;
    assert_eq!(Configuration::parse(&c), Err(UsbError::Endpoints), "bNumEndpoints 5");
    // Seventeen interface descriptors.
    let mut c = vec![0x09, 0x02, 0, 0, 1, 1, 0, 0x80, 50];
    for n in 0..17u8 {
        c.extend_from_slice(&[0x09, 0x04, 0, n, 0, 3, 0, 0, 0]);
    }
    let total = c.len() as u16;
    c[2..4].copy_from_slice(&total.to_le_bytes());
    assert_eq!(Configuration::parse(&c), Err(UsbError::Interfaces), "17 alternates");
    // Five endpoint descriptors after one interface.
    let mut c = vec![0x09, 0x02, 0, 0, 1, 1, 0, 0x80, 50, 0x09, 0x04, 0, 0, 4, 3, 0, 0, 0];
    for n in 1..=5u8 {
        c.extend_from_slice(&[0x07, 0x05, 0x80 | n, 0x03, 0x08, 0x00, 0x01]);
    }
    let total = c.len() as u16;
    c[2..4].copy_from_slice(&total.to_le_bytes());
    assert_eq!(Configuration::parse(&c), Err(UsbError::Endpoints), "5 endpoints");
}

#[test]
fn test_reject_endpoint_outside_an_interface_or_of_absurd_size() {
    let mut c = vec![0x09, 0x02, 16, 0, 1, 1, 0, 0x80, 50];
    c.extend_from_slice(&[0x07, 0x05, 0x81, 0x03, 0x08, 0x00, 0x01]);
    assert_eq!(Configuration::parse(&c), Err(UsbError::Order));
    for size in [0u16, 1025, 0x07ff] {
        let mut c = config(KEYBOARD).to_vec();
        c[31..33].copy_from_slice(&size.to_le_bytes());
        assert_eq!(Configuration::parse(&c), Err(UsbError::MaxPacket), "max packet {size}");
    }
}

#[test]
fn refusal_codes_are_stable() {
    assert_eq!(UsbError::Truncated.code(), "usb.desc.truncated");
    assert_eq!(UsbError::MaxPacket0.code(), "usb.desc.max_packet0");
    assert_eq!(UsbError::HubPorts.code(), "usb.hub.ports");
    assert_eq!(UsbError::RouteDepth.code(), "usb.route.depth");
}
