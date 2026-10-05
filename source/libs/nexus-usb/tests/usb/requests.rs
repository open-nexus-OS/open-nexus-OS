// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use nexus_usb::{HidProtocol, PortFeature, SetupPacket};

#[test]
fn setup_packets_on_the_wire() {
    let cases: [(SetupPacket, [u8; 8]); 10] = [
        (SetupPacket::device_descriptor(8), [0x80, 0x06, 0x00, 0x01, 0x00, 0x00, 0x08, 0x00]),
        (SetupPacket::configuration_descriptor(9), [0x80, 0x06, 0x00, 0x02, 0, 0, 0x09, 0x00]),
        (SetupPacket::set_configuration(1), [0x00, 0x09, 0x01, 0x00, 0, 0, 0, 0]),
        (SetupPacket::set_interface(0, 1), [0x01, 0x0b, 0x01, 0x00, 0x00, 0x00, 0, 0]),
        // Exactly what the boot-protocol capture sent (`boot_capture.py`).
        (SetupPacket::hid_set_protocol(1, HidProtocol::Boot), [0x21, 0x0b, 0, 0, 0x01, 0, 0, 0]),
        (SetupPacket::hid_set_idle_infinite(0), [0x21, 0x0a, 0, 0, 0, 0, 0, 0]),
        (SetupPacket::hub_descriptor(9), [0xa0, 0x06, 0x00, 0x29, 0, 0, 0x09, 0x00]),
        (SetupPacket::hub_port_status(2), [0xa3, 0x00, 0, 0, 0x02, 0x00, 0x04, 0x00]),
        (
            SetupPacket::hub_set_port_feature(3, PortFeature::Power),
            [0x23, 0x03, 0x08, 0x00, 0x03, 0x00, 0, 0],
        ),
        (
            SetupPacket::hub_clear_port_feature(2, PortFeature::CConnection),
            [0x23, 0x01, 0x10, 0x00, 0x02, 0x00, 0, 0],
        ),
    ];
    for (packet, bytes) in cases {
        assert_eq!(packet.to_bytes(), bytes, "{packet:?}");
        assert_eq!(SetupPacket::from_bytes(bytes), packet);
    }
    assert!(SetupPacket::device_descriptor(18).is_in());
    assert!(!SetupPacket::set_configuration(1).is_in());
    assert_eq!(SetupPacket::hid_set_protocol(0, HidProtocol::Report).value, 1);
    assert_eq!(SetupPacket::hub_set_port_feature(1, PortFeature::Reset).value, 4);
    assert_eq!(PortFeature::CReset.selector(), 20);
}
