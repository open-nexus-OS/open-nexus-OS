// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The HID boot class server (TASK-0253B, RFC-0099 §5) over the real core and the machine: what
//! the one subscriber hears and in which order — the answer, the interfaces attached before
//! it, every drain's reports unparsed, a detach — and what a full or a dead client changes.

use nexus_usb::{HidRole, Speed};
use nexus_wire::usb as wire;
use xhcid::{HidClass, Note};
use xhcid_model::dev::{self, Dev};
use xhcid_model::{Frames, Rig};

use super::{qemu_machine, qemu_rig};

/// A frame as the test reads it.
#[derive(Debug, PartialEq, Eq)]
enum Heard {
    Answer(u8),
    Attached { device: u16, vendor: u16, product: u16, interface: u8, role: u8, max_packet: u16 },
    Reports { device: u16, reports: Vec<Vec<u8>> },
    Detached(u16),
}

fn heard(frames: &Frames) -> Vec<Heard> {
    frames.drain().iter().map(|f| decode(f)).collect()
}

fn decode(frame: &[u8]) -> Heard {
    match wire::frame_op(frame).expect("a class frame") {
        wire::OP_SUBSCRIBED => Heard::Answer(wire::decode_subscribed(frame).expect("answer")),
        wire::OP_DEVICE_ATTACHED => {
            let (device, vendor, product, interface, role, max_packet) =
                wire::decode_attached(frame).expect("attach");
            Heard::Attached { device, vendor, product, interface, role, max_packet }
        }
        wire::OP_HID_REPORTS => {
            let (device, count, list) = wire::decode_reports(frame).expect("reports");
            let reports = wire::reports(count, list).expect("a checked list");
            Heard::Reports { device, reports: reports.map(<[u8]>::to_vec).collect() }
        }
        wire::OP_DEVICE_DETACHED => Heard::Detached(wire::decode_detached(frame).expect("detach")),
        op => panic!("not a class frame: op {op}"),
    }
}

/// QEMU's boot keyboard (`usb-kbd`) as the class names it.
fn keyboard(device: u16) -> Heard {
    Heard::Attached {
        device,
        vendor: 0x0627,
        product: 0x0001,
        interface: 0,
        role: wire::ROLE_KEYBOARD,
        max_packet: 8,
    }
}

/// QEMU's boot mouse (`usb-mouse`).
fn mouse(device: u16) -> Heard {
    Heard::Attached {
        device,
        vendor: 0x0627,
        product: 0x0001,
        interface: 0,
        role: wire::ROLE_MOUSE,
        max_packet: 4,
    }
}

fn reports(device: u16, reports: &[&[u8]]) -> Heard {
    Heard::Reports { device, reports: reports.iter().map(|r| r.to_vec()).collect() }
}

#[test]
fn a_late_subscriber_hears_the_answer_then_every_interface_attached_before_it() {
    let (_m, mut rig) = qemu_rig();
    let frames = rig.subscribe();
    assert_eq!(heard(&frames), [Heard::Answer(wire::STATUS_OK), keyboard(1), mouse(2)]);
    assert!(rig.sink.class.subscribed());
    assert_eq!(rig.sink.class.attached(), 2);
}

#[test]
fn an_early_subscriber_hears_each_interface_as_it_attaches() {
    let m = qemu_machine();
    let mut rig = Rig::new(&m);
    let frames = rig.subscribe();
    assert_eq!(heard(&frames), [Heard::Answer(wire::STATUS_OK)]);
    rig.start();
    assert_eq!(heard(&frames), [keyboard(1), mouse(2)]);
}

/// The controller's bytes reach the client unparsed — its parser is the one parser — and a
/// drain's reports of one interface travel in one frame.
#[test]
fn reports_reach_the_subscriber_unparsed_one_frame_per_drain() {
    let (m, mut rig) = qemu_rig();
    let frames = rig.subscribe();
    frames.drain();
    rig.report(1, 0x1, 0x81, &[0x02, 0, 0x04, 0, 0, 0, 0, 0]);
    rig.report(1, 0x2, 0x81, &[0x01, 0x05, 0xfb, 0x00]);
    assert_eq!(
        heard(&frames),
        [reports(1, &[&[0x02, 0, 0x04, 0, 0, 0, 0, 0]]), reports(2, &[&[0x01, 0x05, 0xfb, 0x00]])]
    );
    // Two reports before the driver wakes: one interrupt, one frame.
    m.borrow_mut().report(1, 0x2, 0x81, &[0, 1, 0, 0]);
    m.borrow_mut().report(1, 0x2, 0x81, &[0, 2, 0, 0]);
    rig.settle();
    assert_eq!(heard(&frames), [reports(2, &[&[0, 1, 0, 0], &[0, 2, 0, 0]])]);
    assert_eq!(rig.sink.class.dropped(), 0);
}

#[test]
fn a_detached_interface_is_told_and_a_replugged_one_is_a_new_device() {
    let (m, mut rig) = qemu_rig();
    let frames = rig.subscribe();
    frames.drain();
    m.borrow_mut().hub_at(1, 0).expect("the hub").unplug(2);
    m.borrow_mut().service();
    rig.settle();
    assert_eq!(heard(&frames), [Heard::Detached(2)]);
    let again = dev::hid(Speed::Full, dev::hid_descriptors(0x0627, 0x0001, &[(2, 4)]));
    m.borrow_mut().hub_at(1, 0).expect("the hub").plug(2, Dev::Hid(again));
    m.borrow_mut().service();
    rig.settle();
    // The same slot, a new name: a report still queued for the old device can never be read
    // as the new one's.
    assert_eq!(heard(&frames), [mouse(3)]);
}

/// A client that does not drain: attaches and detaches stay owed, in order, and go out when it
/// drains again (on the retry deadline); reports are dropped and counted.
#[test]
fn a_full_client_keeps_attaches_and_detaches_owed_and_drops_reports() {
    let (m, mut rig) = qemu_rig();
    let frames = rig.subscribe();
    frames.drain();
    frames.full.set(true);
    rig.report(1, 0x2, 0x81, &[0, 1, 1, 0]);
    m.borrow_mut().hub_at(1, 0).expect("the hub").unplug(2);
    m.borrow_mut().service();
    rig.settle();
    let keyboard_on_3 = dev::hid(Speed::Full, dev::hid_descriptors(0x0627, 0x0001, &[(1, 8)]));
    m.borrow_mut().hub_at(1, 0).expect("the hub").plug(3, Dev::Hid(keyboard_on_3));
    m.borrow_mut().service();
    rig.settle();
    assert!(heard(&frames).is_empty());
    assert_eq!(rig.sink.class.dropped(), 1);
    assert!(rig.sink.class.deadline().is_some(), "a retry is armed");
    frames.full.set(false);
    rig.retry();
    assert_eq!(heard(&frames), [Heard::Detached(2), keyboard(3)]);
    assert_eq!(rig.sink.class.deadline(), None);
    rig.report(1, 0x3, 0x81, &[0, 0, 0x05, 0, 0, 0, 0, 0]);
    assert_eq!(heard(&frames), [reports(3, &[&[0, 0, 0x05, 0, 0, 0, 0, 0]])]);
}

/// An interface that comes and goes while the client is full: its attach is withdrawn, so the
/// client never hears of it at all.
#[test]
fn an_attach_the_client_never_heard_is_withdrawn_not_detached() {
    let (m, mut rig) = qemu_rig();
    let frames = rig.subscribe();
    frames.drain();
    frames.full.set(true);
    let brief = dev::hid(Speed::Full, dev::hid_descriptors(0x0627, 0x0001, &[(1, 8)]));
    m.borrow_mut().hub_at(1, 0).expect("the hub").plug(3, Dev::Hid(brief));
    m.borrow_mut().service();
    rig.settle();
    m.borrow_mut().hub_at(1, 0).expect("the hub").unplug(3);
    m.borrow_mut().service();
    rig.settle();
    frames.full.set(false);
    rig.retry();
    assert!(heard(&frames).is_empty());
}

#[test]
fn a_dead_client_ends_the_subscription_and_the_next_hears_everything() {
    let (_m, mut rig) = qemu_rig();
    let frames = rig.subscribe();
    frames.drain();
    frames.gone.set(true);
    rig.report(1, 0x1, 0x81, &[0, 0, 0x04, 0, 0, 0, 0, 0]);
    assert!(!rig.sink.class.subscribed());
    let next = rig.subscribe();
    assert_eq!(heard(&next), [Heard::Answer(wire::STATUS_OK), keyboard(1), mouse(2)]);
}

/// A subscriber policyd refused hears its status on its own channel, and nothing after it.
#[test]
fn test_reject_a_refused_subscriber_hears_its_status_and_nothing_else() {
    let (_m, mut rig) = qemu_rig();
    let frames = Frames::default();
    HidClass::refuse(frames.clone(), wire::STATUS_DENIED);
    rig.report(1, 0x1, 0x81, &[0, 0, 0x04, 0, 0, 0, 0, 0]);
    assert_eq!(heard(&frames), [Heard::Answer(wire::STATUS_DENIED)]);
    assert!(!rig.sink.class.subscribed());
}

fn hid_interface(slot: u8, role: HidRole) -> Note<'static> {
    Note::HidInterface {
        slot,
        interface: 0,
        role,
        endpoint: 0x81,
        max_packet: 64,
        interval: 1,
        vendor: 0x1234,
        product: 0x5678,
    }
}

/// Bytes that are no report of the class (a zero-length packet, more than a report may be)
/// are refused; so is an interface past the table's bound.
#[test]
fn test_reject_reports_outside_the_class_and_interfaces_past_the_table() {
    let mut class = HidClass::<Frames>::new();
    let frames = Frames::default();
    drop(class.subscribe(frames.clone()));
    class.note(&hid_interface(2, HidRole::Keyboard));
    class.flush(0);
    frames.drain();
    for bytes in [&[][..], &[0; wire::REPORT_MAX + 1][..]] {
        class.note(&Note::Report { slot: 2, interface: 0, role: HidRole::Keyboard, bytes });
    }
    class.flush(0);
    assert!(heard(&frames).is_empty());
    assert_eq!(class.refused(), 2);
    for slot in 10..10 + xhcid::hid_class::MAX_INTERFACES as u8 {
        class.note(&hid_interface(slot, HidRole::Mouse));
    }
    assert_eq!(class.attached(), xhcid::hid_class::MAX_INTERFACES);
    assert_eq!(class.refused(), 3, "the 17th interface found no room");
}

/// A pipe given up detaches its interface; a detaching interface's last reports go out first;
/// a drain fuller than a frame spills into the next.
#[test]
fn a_lost_pipe_detaches_and_a_fuller_drain_spills_into_more_frames() {
    let mut class = HidClass::<Frames>::new();
    let frames = Frames::default();
    drop(class.subscribe(frames.clone()));
    class.note(&hid_interface(2, HidRole::Keyboard));
    class.note(&hid_interface(3, HidRole::Mouse));
    class.flush(0);
    frames.drain();
    for n in 0..=wire::REPORTS_MAX as u8 {
        class.note(&Note::Report {
            slot: 2,
            interface: 0,
            role: HidRole::Keyboard,
            bytes: &[n; 8],
        });
    }
    class.flush(0);
    let spilled = heard(&frames);
    assert_eq!(spilled.len(), 2);
    assert!(
        matches!(&spilled[0], Heard::Reports { device: 1, reports } if reports.len() == wire::REPORTS_MAX)
    );
    assert_eq!(spilled[1], reports(1, &[&[wire::REPORTS_MAX as u8; 8]]));
    class.note(&Note::Report { slot: 3, interface: 0, role: HidRole::Mouse, bytes: &[1, 0, 0] });
    class.note(&Note::HidLost { slot: 3, interface: 0 });
    class.flush(0);
    assert_eq!(heard(&frames), [reports(2, &[&[1, 0, 0]]), Heard::Detached(2)]);
    assert_eq!(class.attached(), 1);
}
