// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use nexus_usb::HidRole;
use xhcid_model::dev::Dev;

use super::{board_rig, qemu_rig};

#[test]
fn a_keyboard_and_a_mouse_behind_qemus_full_speed_hub() {
    let (_m, rig) = qemu_rig();
    assert!(rig.sink.fails.is_empty(), "{:?}", rig.sink.fails);
    assert_eq!(
        rig.sink.notes("Enumerated"),
        [
            "Enumerated { slot: 1, route: 0, speed: Full, vendor: 1033, product: 21930, class: 9 }",
            "Enumerated { slot: 2, route: 1, speed: Full, vendor: 1575, product: 1, class: 3 }",
            "Enumerated { slot: 3, route: 2, speed: Full, vendor: 1575, product: 1, class: 3 }",
        ]
    );
    assert_eq!(
        rig.sink.notes("Hub"),
        ["Hub { slot: 1, root_port: 1, speed: Full, ports: 8, ttt: 0 }"]
    );
    assert_eq!(
        rig.sink.notes("HidInterface"),
        [
            "HidInterface { slot: 2, interface: 0, role: Keyboard, endpoint: 129, max_packet: 8, interval: 10 }",
            "HidInterface { slot: 3, interface: 0, role: Mouse, endpoint: 129, max_packet: 4, interval: 10 }",
        ]
    );
}

#[test]
fn the_desk_devices_behind_the_boards_high_speed_hub_with_tt() {
    let (m, rig) = board_rig();
    assert!(rig.sink.fails.is_empty(), "{:?}", rig.sink.fails);
    assert_eq!(
        rig.sink.notes("Enumerated"),
        [
            "Enumerated { slot: 1, route: 0, speed: High, vendor: 8457, product: 10263, class: 9 }",
            "Enumerated { slot: 2, route: 2, speed: Full, vendor: 13364, product: 291, class: 3 }",
            "Enumerated { slot: 3, route: 3, speed: Full, vendor: 1133, product: 50495, class: 3 }",
        ]
    );
    assert_eq!(
        rig.sink.notes("Hub"),
        ["Hub { slot: 1, root_port: 1, speed: High, ports: 5, ttt: 3 }"]
    );
    // The machine refuses Address Device without the right TT: both got their slots with the
    // hub's slot and their port; the hub's slot carries its ports and think time.
    let mb = m.borrow();
    let slots = &mb.hc.slots;
    assert_eq!(slots[2].as_ref().map(|s| s.tt), Some((1, 2)));
    assert_eq!(slots[3].as_ref().map(|s| s.tt), Some((1, 3)));
    let hub = slots[1].as_ref().expect("the hub's slot");
    assert_eq!((hub.hub, hub.hub_ports, hub.ttt), (true, 5, 3));
    assert_eq!(
        rig.sink.notes("HidInterface"),
        [
            "HidInterface { slot: 2, interface: 0, role: Keyboard, endpoint: 129, max_packet: 8, interval: 1 }",
            "HidInterface { slot: 3, interface: 0, role: Keyboard, endpoint: 129, max_packet: 12, interval: 1 }",
            "HidInterface { slot: 3, interface: 1, role: Mouse, endpoint: 130, max_packet: 32, interval: 1 }",
        ]
    );
    // Every boot interface was asked for the boot protocol; the mouse's STALLed SET_IDLE was
    // recovered (Reset Endpoint, Set TR Dequeue Pointer) and its interface used anyway.
    let hub_dev = mb.ports[0].device.as_ref().expect("the hub");
    let Dev::Hub(h) = hub_dev else { panic!("a hub") };
    let Some(Dev::Hid(receiver)) = h.ports[2].device.as_deref() else { panic!("the receiver") };
    assert_eq!(receiver.protocols, [(0, 0), (1, 0)]);
    let recoveries = mb.hc.commands.iter().filter(|(kind, _)| *kind == 14).count();
    assert_eq!(recoveries, 1, "one Reset Endpoint: the STALLed SET_IDLE");
}

#[test]
fn the_measured_reports_arrive_unparsed() {
    let (_m, mut rig) = board_rig();
    rig.report(1, 0x3, 0x82, &[0x00, 0xaa, 0x7f, 0x00]);
    rig.report(1, 0x3, 0x82, &[0x08, 0x00, 0x00, 0x00]);
    // The stale report-format frame right after the switch: forwarded as bytes — the HID
    // client's parser refuses it by length.
    rig.report(1, 0x3, 0x82, &[0x02, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00]);
    rig.report(1, 0x2, 0x81, &[0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00]);
    assert_eq!(
        rig.sink.reports(),
        [
            (3, 1, HidRole::Mouse, vec![0x00, 0xaa, 0x7f, 0x00]),
            (3, 1, HidRole::Mouse, vec![0x08, 0x00, 0x00, 0x00]),
            (3, 1, HidRole::Mouse, vec![0x02, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00]),
            (2, 0, HidRole::Keyboard, vec![0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00]),
        ]
    );
    assert_eq!(rig.xhci.deadline(), None, "nothing waits between reports");
}

#[test]
fn a_thousand_reports_wrap_every_ring() {
    // The event ring (256), the interrupt ring (16) and the report buffers (4) all wrap many
    // times; ERDP keeps up (the machine refuses a full event ring).
    let (_m, mut rig) = qemu_rig();
    for i in 0..1000u32 {
        rig.report(1, 0x2, 0x81, &[0, (i % 7) as u8, 1, 0]);
    }
    let reports = rig.sink.reports();
    assert_eq!(reports.len(), 1000);
    assert!(reports.iter().enumerate().all(|(i, r)| r.3 == [0, (i % 7) as u8, 1, 0]));
}
