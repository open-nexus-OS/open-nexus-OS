// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use nexus_usb::Speed;
use xhcid_model::dev::{self, Dev};

use super::qemu_rig;

#[test]
fn a_device_unplugged_from_the_hub_is_detached_and_its_slot_disabled() {
    let (m, mut rig) = qemu_rig();
    m.borrow_mut().hub_at(1, 0).expect("the hub").unplug(2);
    m.borrow_mut().service();
    rig.settle();
    assert_eq!(rig.sink.notes("Detached"), ["Detached { slot: 3 }"]);
    assert!(m.borrow().hc.slots[3].is_none(), "Disable Slot ran");
    assert!(m.borrow().hc.commands.iter().any(|&(kind, cc)| (kind, cc) == (10, 1)));
    // Plugged in again: enumerated again, its slot back.
    let mouse = dev::hid(Speed::Full, dev::hid_descriptors(0x0627, 0x0001, &[(2, 4)]));
    m.borrow_mut().hub_at(1, 0).expect("the hub").plug(2, Dev::Hid(mouse));
    m.borrow_mut().service();
    rig.settle();
    let enumerated = rig.sink.notes("Enumerated");
    assert_eq!(enumerated.len(), 4);
    assert!(enumerated[3].starts_with("Enumerated { slot: 3, route: 2"));
    assert!(rig.sink.fails.is_empty(), "{:?}", rig.sink.fails);
    rig.report(1, 0x2, 0x81, &[1, 2, 3, 0]);
    assert_eq!(rig.sink.reports().len(), 1);
}
