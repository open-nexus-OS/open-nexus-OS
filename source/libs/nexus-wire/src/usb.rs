// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! USB class-client wire protocol (RFC-0099 §5, TASK-0253B). `xhcid` owns the controller; a
//! class service is its client and never touches the hardware. v1 has one class, the HID boot
//! protocol, and one subscriber per class.
//!
//! The client subscribes ONCE on xhcid's server endpoint, cap-moving the SEND half of its own
//! push channel: `SUBSCRIBE` `[U, B, ver, OP, class]`. Everything after that arrives on the push
//! channel, in order: the answer `[U, B, ver, OP_SUBSCRIBE|0x80, status]`, then an `ATTACHED`
//! for every interface of the class already attached and for every later one, the `REPORTS` of
//! each drain, a `DETACHED` when an interface goes away.
//!
//! `ATTACHED`: `[U, B, ver, OP, device:u16le, vendor:u16le, product:u16le, interface:u8,
//! role:u8, max_packet:u16le]` — `device` names the attachment, never reused while the client
//! could still hold it (a re-plugged keyboard is a new device).
//! `REPORTS`: `[U, B, ver, OP, device:u16le, count:u8, len:u8, list…]` — the list is `count`
//! reports, each `len:u8` + the controller's bytes, unparsed (the class's parser is the client's).
//! `DETACHED`: `[U, B, ver, OP, device:u16le]`.

use crate::codec::request_op;

/// First magic byte (`'U'`).
pub const MAGIC0: u8 = b'U';
/// Second magic byte (`'B'`).
pub const MAGIC1: u8 = b'B';
/// Protocol version.
pub const VERSION: u8 = 1;

/// Subscribe to a class (moves the push channel's SEND half alongside).
pub const OP_SUBSCRIBE: u8 = 1;
/// An interface of the subscribed class is attached and reporting.
pub const OP_DEVICE_ATTACHED: u8 = 2;
/// The reports one drain produced for an attached interface.
pub const OP_HID_REPORTS: u8 = 3;
/// An attached interface went away (unplugged, disabled, or its pipe given up).
pub const OP_DEVICE_DETACHED: u8 = 4;
/// The subscription's answer on the push channel (`OP_SUBSCRIBE | 0x80`).
pub const OP_SUBSCRIBED: u8 = OP_SUBSCRIBE | crate::codec::REPLY_BIT;

/// The HID boot protocol (keyboards and mice, RFC-0099 §5).
pub const CLASS_HID_BOOT: u8 = 1;

/// A boot keyboard interface (8-byte reports).
pub const ROLE_KEYBOARD: u8 = 1;
/// A boot mouse interface (3..=8-byte reports).
pub const ROLE_MOUSE: u8 = 2;

/// Subscribed: the class's interfaces follow on the push channel.
pub const STATUS_OK: u8 = 0;
/// The subscription frame was malformed or came without a channel.
pub const STATUS_MALFORMED: u8 = 1;
/// The sender does not hold the class's capability (policyd, deny-by-default).
pub const STATUS_DENIED: u8 = 2;
/// No such class.
pub const STATUS_UNSUPPORTED: u8 = 3;

/// The longest report a frame carries: a full-speed interrupt endpoint's largest packet.
pub const REPORT_MAX: usize = 64;
/// Reports one frame carries at most.
pub const REPORTS_MAX: usize = 16;
/// The report list's bytes at most (each report is its `len:u8` and its bytes).
pub const REPORT_BYTES_MAX: usize = 240;
/// The largest frame of this protocol (a receive buffer of this size never truncates).
pub const FRAME_MAX: usize = 4 + 2 + 1 + 1 + REPORT_BYTES_MAX;

crate::frames! {
    protocol(magic0 = MAGIC0, magic1 = MAGIC1, version = VERSION);

    /// SUBSCRIBE: `[U, B, ver, OP_SUBSCRIBE, class]` (the push channel's SEND half moves along).
    request fixed encode_subscribe / decode_subscribe (op = OP_SUBSCRIBE) {
        class: u8,
    }
    /// The answer, the first frame on the push channel: `[U, B, ver, OP_SUBSCRIBE|0x80, status]`.
    reply fixed encode_subscribed / decode_subscribed (op = OP_SUBSCRIBE) {
        status: u8,
    }
    /// ATTACHED: `[U, B, ver, OP_DEVICE_ATTACHED, device, vendor, product, interface, role,
    /// max_packet]`.
    request fixed encode_attached / decode_attached (op = OP_DEVICE_ATTACHED) {
        device: u16le,
        vendor: u16le,
        product: u16le,
        interface: u8,
        role: u8,
        max_packet: u16le,
    }
    /// REPORTS: `[U, B, ver, OP_HID_REPORTS, device, count, len, list…]` — read the list with
    /// [`reports`], build it with [`ReportList`].
    request encode_reports / decode_reports (op = OP_HID_REPORTS) {
        device: u16le,
        count: u8,
        list: bytes8(min = 2, max = REPORT_BYTES_MAX),
    }
    /// DETACHED: `[U, B, ver, OP_DEVICE_DETACHED, device]`.
    request fixed encode_detached / decode_detached (op = OP_DEVICE_DETACHED) {
        device: u16le,
    }
}

/// The op of a frame of this protocol (magic and version checked), `None` for anything else.
#[must_use]
pub fn frame_op(frame: &[u8]) -> Option<u8> {
    request_op(frame, MAGIC0, MAGIC1, VERSION)
}

/// The reports of a `REPORTS` frame, checked before the first is handed out: exactly `count`
/// of them (1..=[`REPORTS_MAX`]), each 1..=[`REPORT_MAX`] bytes, nothing after the last.
#[must_use]
pub fn reports(count: u8, list: &[u8]) -> Option<Reports<'_>> {
    if count == 0 || usize::from(count) > REPORTS_MAX {
        return None;
    }
    let mut rest = list;
    for _ in 0..count {
        let (&len, tail) = rest.split_first()?;
        if len == 0 || usize::from(len) > REPORT_MAX || usize::from(len) > tail.len() {
            return None;
        }
        rest = &tail[usize::from(len)..];
    }
    rest.is_empty().then_some(Reports { rest: list, left: count })
}

/// The reports of a checked list, in order.
#[derive(Clone, Debug)]
pub struct Reports<'a> {
    rest: &'a [u8],
    left: u8,
}

impl<'a> Iterator for Reports<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<&'a [u8]> {
        if self.left == 0 {
            return None;
        }
        let (&len, tail) = self.rest.split_first()?;
        let (report, rest) = tail.split_at_checked(usize::from(len))?;
        self.rest = rest;
        self.left -= 1;
        Some(report)
    }
}

/// What [`ReportList::push`] did with a report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pushed {
    /// In the list.
    Added,
    /// The list is full: send it, clear it, push again.
    Full,
    /// Empty or longer than [`REPORT_MAX`]: no report of the class (counted, never sent).
    Refused,
}

/// One frame's report list being filled (the controller side): reports appended until the
/// next would not fit — no allocation.
#[derive(Clone, Debug)]
pub struct ReportList {
    bytes: [u8; REPORT_BYTES_MAX],
    len: usize,
    count: u8,
}

impl Default for ReportList {
    fn default() -> Self {
        Self::new()
    }
}

impl ReportList {
    /// An empty list.
    #[must_use]
    pub const fn new() -> Self {
        Self { bytes: [0; REPORT_BYTES_MAX], len: 0, count: 0 }
    }

    /// Appends `report` if it is one and fits.
    pub fn push(&mut self, report: &[u8]) -> Pushed {
        if report.is_empty() || report.len() > REPORT_MAX {
            return Pushed::Refused;
        }
        let end = self.len + 1 + report.len();
        if usize::from(self.count) == REPORTS_MAX || end > REPORT_BYTES_MAX {
            return Pushed::Full;
        }
        self.bytes[self.len] = report.len() as u8;
        self.bytes[self.len + 1..end].copy_from_slice(report);
        self.len = end;
        self.count += 1;
        Pushed::Added
    }

    /// Reports in the list.
    #[must_use]
    pub const fn count(&self) -> u8 {
        self.count
    }

    /// No report in the list.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Empties the list.
    pub fn clear(&mut self) {
        self.len = 0;
        self.count = 0;
    }

    /// The `REPORTS` frame of `device` carrying the list (`None` while it is empty).
    pub fn encode(&self, device: u16, out: &mut [u8]) -> Option<usize> {
        if self.is_empty() {
            return None;
        }
        encode_reports(device, self.count, &self.bytes[..self.len], out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::testing::assert_reject_matrix;

    // Golden byte layouts: the wire is the contract between xhcid and the class client.
    #[test]
    fn subscribe_and_answer_golden_bytes() {
        assert_eq!(encode_subscribe(CLASS_HID_BOOT), [b'U', b'B', 1, OP_SUBSCRIBE, 1]);
        assert_eq!(decode_subscribe(&[b'U', b'B', 1, 1, 1]), Some(CLASS_HID_BOOT));
        assert_eq!(encode_subscribed(STATUS_DENIED), [b'U', b'B', 1, 0x81, STATUS_DENIED]);
        assert_eq!(frame_op(&encode_subscribed(STATUS_OK)), Some(OP_SUBSCRIBED));
        assert_eq!(decode_subscribed(&encode_subscribed(STATUS_OK)), Some(STATUS_OK));
    }

    #[test]
    fn attached_and_detached_golden_bytes() {
        let frame = encode_attached(0x0102, 0x3434, 0x0123, 0, ROLE_KEYBOARD, 64);
        assert_eq!(
            frame,
            [b'U', b'B', 1, OP_DEVICE_ATTACHED, 0x02, 0x01, 0x34, 0x34, 0x23, 0x01, 0, 1, 64, 0]
        );
        assert_eq!(decode_attached(&frame), Some((0x0102, 0x3434, 0x0123, 0, ROLE_KEYBOARD, 64)));
        assert_eq!(encode_detached(7), [b'U', b'B', 1, OP_DEVICE_DETACHED, 7, 0]);
        assert_eq!(decode_detached(&encode_detached(7)), Some(7));
    }

    #[test]
    fn reports_round_trip_in_order() {
        let mut list = ReportList::new();
        assert_eq!(list.push(&[0, 0, 4, 0, 0, 0, 0, 0]), Pushed::Added);
        assert_eq!(list.push(&[1, 0xfe, 2, 0]), Pushed::Added);
        let mut buf = [0u8; FRAME_MAX];
        let n = list.encode(3, &mut buf).unwrap();
        assert_eq!(&buf[..8], &[b'U', b'B', 1, OP_HID_REPORTS, 3, 0, 2, 14]);
        let (device, count, bytes) = decode_reports(&buf[..n]).unwrap();
        assert_eq!(device, 3);
        let got: Vec<&[u8]> = reports(count, bytes).unwrap().collect();
        assert_eq!(got, [&[0, 0, 4, 0, 0, 0, 0, 0][..], &[1, 0xfe, 2, 0][..]]);
    }

    #[test]
    fn a_full_list_says_so_and_a_frame_stays_inside_frame_max() {
        let mut list = ReportList::new();
        for _ in 0..REPORTS_MAX {
            assert_eq!(list.push(&[0; 8]), Pushed::Added);
        }
        assert_eq!(list.push(&[0; 8]), Pushed::Full);
        let mut buf = [0u8; FRAME_MAX];
        assert!(list.encode(1, &mut buf).is_some());
        list.clear();
        assert!(list.is_empty());
        assert_eq!(list.encode(1, &mut buf), None, "an empty list is no frame");
        // Large reports fill the bytes before the count.
        for _ in 0..3 {
            assert_eq!(list.push(&[0; REPORT_MAX]), Pushed::Added);
        }
        assert_eq!(list.push(&[0; REPORT_MAX]), Pushed::Full);
        assert!(list.encode(1, &mut buf).unwrap() <= FRAME_MAX);
    }

    #[test]
    fn test_reject_reports_outside_the_class() {
        let mut list = ReportList::new();
        assert_eq!(list.push(&[]), Pushed::Refused, "a zero-length packet is no report");
        assert_eq!(list.push(&[0; REPORT_MAX + 1]), Pushed::Refused);
        assert!(list.is_empty());
    }

    #[test]
    fn test_reject_report_lists_that_do_not_add_up() {
        let two = [3, 1, 2, 3, 1, 9];
        assert!(reports(2, &two).is_some());
        assert!(reports(0, &two).is_none(), "no reports");
        assert!(reports(1, &two).is_none(), "bytes after the last report");
        assert!(reports(3, &two).is_none(), "fewer reports than counted");
        assert!(reports(1, &[0]).is_none(), "an empty report");
        assert!(reports(1, &[4, 1, 2, 3]).is_none(), "a report past the list");
        let mut long = [0u8; 1 + REPORT_MAX + 1];
        long[0] = (REPORT_MAX + 1) as u8;
        assert!(reports(1, &long).is_none(), "a report past REPORT_MAX");
        assert!(reports(REPORTS_MAX as u8 + 1, &[1, 0]).is_none(), "more than REPORTS_MAX");
    }

    #[test]
    fn test_reject_malformed_frames() {
        assert_reject_matrix(&encode_subscribe(CLASS_HID_BOOT), 4, &|f| {
            decode_subscribe(f).is_some()
        });
        assert_reject_matrix(&encode_attached(1, 2, 3, 0, ROLE_MOUSE, 4), 4, &|f| {
            decode_attached(f).is_some()
        });
        assert_reject_matrix(&encode_detached(1), 4, &|f| decode_detached(f).is_some());
        let mut list = ReportList::new();
        list.push(&[1, 2, 3]);
        let mut buf = [0u8; FRAME_MAX];
        let n = list.encode(9, &mut buf).unwrap();
        assert_reject_matrix(&buf[..n], 4, &|f| decode_reports(f).is_some());
        assert_eq!(frame_op(&[b'S', b'T', 1, 1]), None, "another protocol's magic");
    }
}
