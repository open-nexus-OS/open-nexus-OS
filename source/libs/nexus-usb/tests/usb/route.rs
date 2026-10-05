// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use nexus_usb::{RouteString, Speed, UsbError};

#[test]
fn route_strings_name_the_path_tier_by_tier() {
    let root = RouteString::ROOT;
    assert_eq!((root.raw(), root.depth()), (0, 0));
    // The keyboard: port 2 of the hub on a root port.
    let kbd = root.child(2).expect("tier 1");
    assert_eq!((kbd.raw(), kbd.depth()), (0x2, 1));
    let deeper = kbd.child(15).expect("tier 2");
    assert_eq!((deeper.raw(), deeper.depth()), (0xf2, 2));
    let mut r = RouteString::ROOT;
    for port in [1u8, 2, 3, 4, 5] {
        r = r.child(port).expect("five tiers");
    }
    assert_eq!((r.raw(), r.depth()), (0x54321, 5));
}

#[test]
fn test_reject_route_port_or_depth() {
    assert_eq!(RouteString::ROOT.child(0), Err(UsbError::RoutePort));
    assert_eq!(RouteString::ROOT.child(16), Err(UsbError::RoutePort));
    let mut r = RouteString::ROOT;
    for _ in 0..5 {
        r = r.child(1).expect("tier");
    }
    assert_eq!(r.child(1), Err(UsbError::RouteDepth), "a sixth hub");
}

#[test]
fn speeds() {
    assert_eq!(Speed::Full.default_max_packet0(), 8);
    assert_eq!(Speed::High.default_max_packet0(), 64);
    assert_eq!(Speed::Super.default_max_packet0(), 512);
    assert!(Speed::Full.needs_tt() && Speed::Low.needs_tt());
    assert!(!Speed::High.needs_tt() && !Speed::Super.needs_tt());
}
