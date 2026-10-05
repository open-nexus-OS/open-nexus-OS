// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: hidrawd's USB source against xhcid's frames (TASK-0253B, RFC-0099 §5): boot reports
//! in the formats the desk's devices send (an 8-byte keyboard report, a 4-byte mouse report with
//! the wheel — `docs/board/measurements/2026-10-04-usb-boot-protocol/`) become the events inputd
//! expects; a detach releases what was held; frames that are not xhcid's, do not add up, or name
//! no attached interface are refused whole, and a report outside the boot formats is refused
//! without touching the others.
//! OWNERS: @runtime @ui

use hid::{HidEvent, HidEventKind, KeyboardUsage, MouseButton, RelativeAxis, TimestampNs};
use hidrawd::source::{DeviceFrame, Emit};
use hidrawd::usb_source::{Heard, Rejected, UsbHid, MAX_USB_DEVICES, USB_DEVICE_BASE};
use hidrawd::{DeviceId, HidDeviceKind, PointerSource};
use nexus_wire::usb as wire;

/// xhcid's kernel identity, as the test pretends it.
const XHCID: u64 = 0x5eed_0001;

type Logical = Vec<(HidEventKind, u16, i32)>;

/// The batch path as the test sees it.
#[derive(Default)]
struct Batches(Vec<(DeviceFrame, u16, Logical)>);

impl Emit for Batches {
    fn emit(&mut self, frame: &DeviceFrame, raw: u16, events: &[HidEvent]) {
        self.0.push((*frame, raw, logical(events)));
    }
}

fn logical(events: &[HidEvent]) -> Logical {
    events.iter().map(|e| (e.kind(), e.code().raw(), e.value().raw())).collect()
}

fn ts(ns: u64) -> TimestampNs {
    TimestampNs::new(ns)
}

fn attach(device: u16, role: u8) -> [u8; 14] {
    wire::encode_attached(device, 0x3434, 0x0123, 0, role, 8)
}

fn reports(device: u16, reports: &[&[u8]]) -> Vec<u8> {
    let mut list = wire::ReportList::new();
    for report in reports {
        assert_eq!(list.push(report), wire::Pushed::Added);
    }
    let mut buf = [0u8; wire::FRAME_MAX];
    let n = list.encode(device, &mut buf).expect("a reports frame");
    buf[..n].to_vec()
}

fn keyboard_frame(n: u16) -> DeviceFrame {
    DeviceFrame {
        device: DeviceId::new(USB_DEVICE_BASE + n),
        pointer: None,
        abs_max_x: 0,
        abs_max_y: 0,
    }
}

fn mouse_frame(n: u16) -> DeviceFrame {
    DeviceFrame {
        device: DeviceId::new(USB_DEVICE_BASE + n),
        pointer: Some(PointerSource::MouseRelative),
        abs_max_x: 0,
        abs_max_y: 0,
    }
}

fn key(usage: u16, value: i32) -> (HidEventKind, u16, i32) {
    (HidEventKind::Key, usage, value)
}

fn shift() -> u16 {
    KeyboardUsage::modifier_from_bit(1).event_code()
}

#[test]
fn the_answer_is_heard() {
    let mut usb = UsbHid::new(XHCID);
    let mut out = Batches::default();
    assert_eq!(
        usb.on_frame(XHCID, &wire::encode_subscribed(wire::STATUS_OK), ts(1), &mut out),
        Ok(Heard::Subscribed)
    );
    assert_eq!(
        usb.on_frame(XHCID, &wire::encode_subscribed(wire::STATUS_DENIED), ts(1), &mut out),
        Ok(Heard::Refused(wire::STATUS_DENIED))
    );
    assert!(out.0.is_empty());
}

/// Shift+a, then nothing held: the keyboard's 8-byte boot reports as the events inputd takes,
/// HID usages on the wire as the virtio path has always sent them.
#[test]
fn a_keyboard_types_through_its_boot_reports() {
    let mut usb = UsbHid::new(XHCID);
    let mut out = Batches::default();
    let heard = usb.on_frame(XHCID, &attach(1, wire::ROLE_KEYBOARD), ts(1), &mut out);
    let Ok(Heard::Attached(device)) = heard else { panic!("{heard:?}") };
    assert_eq!((device.attachment, device.kind), (1, HidDeviceKind::Keyboard));
    assert_eq!((device.vendor, device.product), (0x3434, 0x0123));
    let a = KeyboardUsage::A.raw();
    let frame = reports(1, &[&[0x02, 0, a, 0, 0, 0, 0, 0], &[0, 0, 0, 0, 0, 0, 0, 0]]);
    let heard = usb.on_frame(XHCID, &frame, ts(2), &mut out);
    assert!(matches!(heard, Ok(Heard::Reports { parsed: 2, refused: 0, .. })), "{heard:?}");
    assert_eq!(
        out.0,
        [(
            keyboard_frame(0),
            2,
            vec![key(shift(), 1), key(u16::from(a), 1), key(u16::from(a), 0), key(shift(), 0)]
        )]
    );
}

/// The measured receiver's 4-byte boot report: buttons, dx, dy, wheel — a relative pointer.
#[test]
fn a_mouse_moves_clicks_and_scrolls_as_a_relative_pointer() {
    let mut usb = UsbHid::new(XHCID);
    let mut out = Batches::default();
    usb.on_frame(XHCID, &attach(2, wire::ROLE_MOUSE), ts(1), &mut out).expect("attach");
    usb.on_frame(XHCID, &reports(2, &[&[0x01, 0x05, 0xfb, 0x01]]), ts(2), &mut out)
        .expect("reports");
    assert_eq!(
        out.0,
        [(
            mouse_frame(0),
            1,
            vec![
                (HidEventKind::Btn, MouseButton::Left.event_code(), 1),
                (HidEventKind::Rel, RelativeAxis::X.event_code(), 5),
                (HidEventKind::Rel, RelativeAxis::Y.event_code(), -5),
                (HidEventKind::Rel, RelativeAxis::Wheel.event_code(), 1),
            ]
        )]
    );
}

/// A device that goes away leaves nothing pressed: its held keys and buttons are released
/// through the batch path before it is forgotten; its name is unknown afterwards.
#[test]
fn a_detach_releases_every_held_key_and_button() {
    let mut usb = UsbHid::new(XHCID);
    let mut out = Batches::default();
    usb.on_frame(XHCID, &attach(1, wire::ROLE_KEYBOARD), ts(1), &mut out).expect("keyboard");
    usb.on_frame(XHCID, &attach(2, wire::ROLE_MOUSE), ts(1), &mut out).expect("mouse");
    let a = KeyboardUsage::A.raw();
    usb.on_frame(XHCID, &reports(1, &[&[0, 0, a, 0, 0, 0, 0, 0]]), ts(2), &mut out)
        .expect("a held");
    usb.on_frame(XHCID, &reports(2, &[&[0x01, 0, 0, 0]]), ts(2), &mut out).expect("left held");
    out.0.clear();
    let heard = usb.on_frame(XHCID, &wire::encode_detached(1), ts(3), &mut out);
    assert!(matches!(heard, Ok(Heard::Detached(d)) if d.attachment == 1), "{heard:?}");
    let heard = usb.on_frame(XHCID, &wire::encode_detached(2), ts(3), &mut out);
    assert!(matches!(heard, Ok(Heard::Detached(d)) if d.attachment == 2), "{heard:?}");
    assert_eq!(
        out.0,
        [
            (keyboard_frame(0), 0, vec![key(u16::from(a), 0)]),
            (mouse_frame(1), 0, vec![(HidEventKind::Btn, MouseButton::Left.event_code(), 0)]),
        ]
    );
    assert_eq!(usb.attached(), 0);
    let late = reports(1, &[&[0, 0, a, 0, 0, 0, 0, 0]]);
    assert_eq!(usb.on_frame(XHCID, &late, ts(4), &mut out), Err(Rejected::UnknownDevice));
}

/// A second attach of a live name (xhcid never sends one) replaces the first — which is released
/// first, so the replacement cannot inherit a held key.
#[test]
fn a_reattached_name_is_released_then_replaced() {
    let mut usb = UsbHid::new(XHCID);
    let mut out = Batches::default();
    let a = KeyboardUsage::A.raw();
    usb.on_frame(XHCID, &attach(1, wire::ROLE_KEYBOARD), ts(1), &mut out).expect("keyboard");
    usb.on_frame(XHCID, &reports(1, &[&[0, 0, a, 0, 0, 0, 0, 0]]), ts(2), &mut out)
        .expect("a held");
    out.0.clear();
    let heard = usb.on_frame(XHCID, &attach(1, wire::ROLE_MOUSE), ts(3), &mut out);
    assert!(matches!(heard, Ok(Heard::Attached(d)) if d.kind == HidDeviceKind::Mouse), "{heard:?}");
    assert_eq!(out.0, [(keyboard_frame(0), 0, vec![key(u16::from(a), 0)])]);
    assert_eq!(usb.attached(), 1);
}

/// Identity is the kernel's: a frame from anyone but xhcid is refused before it is read.
#[test]
fn test_reject_frames_not_from_xhcid() {
    let mut usb = UsbHid::new(XHCID);
    let mut out = Batches::default();
    for frame in
        [&attach(1, wire::ROLE_KEYBOARD)[..], &wire::encode_subscribed(wire::STATUS_OK)[..]]
    {
        assert_eq!(usb.on_frame(XHCID + 1, frame, ts(1), &mut out), Err(Rejected::Foreign));
        assert_eq!(usb.on_frame(0, frame, ts(1), &mut out), Err(Rejected::Foreign));
    }
    assert_eq!(usb.attached(), 0);
    assert!(out.0.is_empty());
}

#[test]
fn test_reject_frames_that_do_not_add_up() {
    let mut usb = UsbHid::new(XHCID);
    let mut out = Batches::default();
    let attached = attach(1, wire::ROLE_KEYBOARD);
    for cut in 0..attached.len() {
        assert_eq!(
            usb.on_frame(XHCID, &attached[..cut], ts(1), &mut out),
            Err(Rejected::Malformed)
        );
    }
    let mut other = attached;
    other[0] = b'S';
    assert_eq!(usb.on_frame(XHCID, &other, ts(1), &mut out), Err(Rejected::Malformed));
    assert_eq!(usb.on_frame(XHCID, &attach(1, 3), ts(1), &mut out), Err(Rejected::Role));
    assert_eq!(
        usb.on_frame(XHCID, &wire::encode_detached(9), ts(1), &mut out),
        Err(Rejected::UnknownDevice)
    );
    assert_eq!(
        usb.on_frame(XHCID, &reports(9, &[&[0; 8]]), ts(1), &mut out),
        Err(Rejected::UnknownDevice)
    );
    usb.on_frame(XHCID, &attached, ts(1), &mut out).expect("attach");
    // A list that says two reports and holds one.
    let mut lying = reports(1, &[&[0; 8]]);
    lying[6] = 2;
    assert_eq!(usb.on_frame(XHCID, &lying, ts(1), &mut out), Err(Rejected::Malformed));
    assert!(out.0.is_empty(), "nothing of a refused frame reached the batch path");
}

/// The stale report-format frame the receiver sends right after the protocol switch (nine
/// bytes) is refused by the parser; the good report beside it still counts.
#[test]
fn test_reject_a_report_outside_the_boot_formats_alone() {
    let mut usb = UsbHid::new(XHCID);
    let mut out = Batches::default();
    usb.on_frame(XHCID, &attach(2, wire::ROLE_MOUSE), ts(1), &mut out).expect("attach");
    let stale = [0x02, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00];
    let frame = reports(2, &[&stale, &[0x00, 0x01, 0x00, 0x00]]);
    let heard = usb.on_frame(XHCID, &frame, ts(2), &mut out);
    assert!(matches!(heard, Ok(Heard::Reports { parsed: 1, refused: 1, .. })), "{heard:?}");
    assert_eq!(
        out.0,
        [(mouse_frame(0), 2, vec![(HidEventKind::Rel, RelativeAxis::X.event_code(), 1)])]
    );
}

#[test]
fn test_reject_attaches_past_the_bound() {
    let mut usb = UsbHid::new(XHCID);
    let mut out = Batches::default();
    for n in 0..MAX_USB_DEVICES as u16 {
        usb.on_frame(XHCID, &attach(n + 1, wire::ROLE_MOUSE), ts(1), &mut out).expect("room");
    }
    let past = attach(MAX_USB_DEVICES as u16 + 1, wire::ROLE_KEYBOARD);
    assert_eq!(usb.on_frame(XHCID, &past, ts(1), &mut out), Err(Rejected::Full));
    assert_eq!(usb.attached(), MAX_USB_DEVICES);
}
