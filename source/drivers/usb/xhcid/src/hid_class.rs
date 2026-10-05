// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The HID boot class server (RFC-0099 §5, TASK-0253B): what xhcid owes its one HID client.
//! The core notes what happened on the bus; [`HidClass::note`] keeps the notes about HID boot
//! interfaces — one attachment per interface, named by a device id never reused while the
//! client could still hold it — and turns them into the class's frames (`nexus_wire::usb`) on
//! the subscriber's push channel: the answer to its SUBSCRIBE, an attach per interface (every
//! one attached so far, for a new subscriber), each drain's reports, a detach.
//!
//! Delivery: attaches, detaches and the answer are OWED — kept in order until the channel takes
//! them; a full channel is tried again on the next wake or after [`RETRY_NS`]. Reports are not:
//! a frame the full channel refuses is dropped and counted (a boot keyboard report carries the
//! whole key state, the next one heals it; a dropped mouse frame loses its deltas only). A
//! report never overtakes its interface's attach, and a detaching interface's last reports go
//! out before its detach. Pure and allocation-free: the OS loop hands it a [`Channel`] on the
//! moved capability, the tests a recorder.

use nexus_usb::HidRole;
use nexus_wire::usb as wire;

use crate::Note;

/// HID boot interfaces attached at once at most (keyboards and mice on the whole bus).
pub const MAX_INTERFACES: usize = 16;
/// Frames owed at most: the answer, and an attach and a detach per interface.
const OWED_MAX: usize = 2 * MAX_INTERFACES + 1;
/// A channel that was full is tried again after this long.
pub const RETRY_NS: u64 = 4_000_000;

/// What a push did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pushed {
    /// The client's queue took the frame.
    Sent,
    /// The client's queue is full (it has not drained yet).
    Full,
    /// The client's channel is dead: the subscription ends.
    Gone,
}

/// Where the class's frames go: the subscriber's push channel.
pub trait Channel {
    /// One frame, never blocking.
    fn push(&mut self, frame: &[u8]) -> Pushed;
}

/// An attached HID boot interface.
struct Attached {
    device: u16,
    slot: u8,
    interface: u8,
    role: HidRole,
    vendor: u16,
    product: u16,
    max_packet: u16,
    /// Its attach reached the subscriber: its reports may follow.
    announced: bool,
    /// Its reports of the current drain.
    reports: wire::ReportList,
}

/// A frame owed to the subscriber.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Owed {
    Answer,
    Attach(u16),
    Detach(u16),
}

/// The class server: the attached interfaces, the one subscriber, what it is owed.
pub struct HidClass<C: Channel> {
    channel: Option<C>,
    attached: [Option<Attached>; MAX_INTERFACES],
    owed: [Owed; OWED_MAX],
    owed_len: usize,
    last_device: u16,
    retry_at: Option<u64>,
    dropped: u32,
    refused: u32,
}

impl<C: Channel> Default for HidClass<C> {
    fn default() -> Self {
        Self::new()
    }
}

impl<C: Channel> HidClass<C> {
    /// No interface, no subscriber.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            channel: None,
            attached: [const { None }; MAX_INTERFACES],
            owed: [Owed::Answer; OWED_MAX],
            owed_len: 0,
            last_device: 0,
            retry_at: None,
            dropped: 0,
            refused: 0,
        }
    }

    /// An admitted subscriber (policyd granted its sender the class): it replaces the one
    /// before — returned, for the caller to close — and is owed the answer, then an attach for
    /// every interface attached so far. Nothing is sent before the next [`Self::flush`].
    pub fn subscribe(&mut self, channel: C) -> Option<C> {
        let old = self.channel.replace(channel);
        self.owed_len = 0;
        self.retry_at = None;
        self.owe(Owed::Answer);
        for i in 0..MAX_INTERFACES {
            if let Some(a) = self.attached[i].as_mut() {
                a.announced = false;
                a.reports.clear();
                let device = a.device;
                self.owe(Owed::Attach(device));
            }
        }
        old
    }

    /// Answers a refused subscriber on its own channel (`status`), then lets go of it.
    pub fn refuse(mut channel: C, status: u8) {
        let _ = channel.push(&wire::encode_subscribed(status));
    }

    /// A subscriber holds the class.
    #[must_use]
    pub const fn subscribed(&self) -> bool {
        self.channel.is_some()
    }

    /// Interfaces attached.
    #[must_use]
    pub fn attached(&self) -> usize {
        self.attached.iter().flatten().count()
    }

    /// Reports dropped: the client's queue was full, or the attach was still owed.
    #[must_use]
    pub const fn dropped(&self) -> u32 {
        self.dropped
    }

    /// Reports and interfaces refused: no report of the class (empty, longer than a report
    /// may be), or no room for one more interface.
    #[must_use]
    pub const fn refused(&self) -> u32 {
        self.refused
    }

    /// When a full channel is tried again, while something is owed.
    #[must_use]
    pub const fn deadline(&self) -> Option<u64> {
        self.retry_at
    }

    /// The core's note: the HID ones change the table and what the subscriber is owed.
    pub fn note(&mut self, note: &Note<'_>) {
        match *note {
            Note::HidInterface { slot, interface, role, max_packet, vendor, product, .. } => {
                self.attach(slot, interface, role, max_packet, vendor, product);
            }
            Note::Report { slot, interface, bytes, .. } => self.report(slot, interface, bytes),
            Note::HidLost { slot, interface } => {
                self.detach(|a| a.slot == slot && a.interface == interface);
            }
            Note::Detached { slot } => self.detach(|a| a.slot == slot),
            _ => {}
        }
    }

    /// The retry deadline passed: what is owed, again.
    pub fn on_timer(&mut self, now: u64) {
        if self.retry_at.is_some_and(|at| now >= at) {
            self.retry_at = None;
            self.flush(now);
        }
    }

    /// What is owed, in order, then every interface's reports of this drain. Called after every
    /// wake that noted something, and when a subscriber was admitted.
    pub fn flush(&mut self, now: u64) {
        while self.owed_len > 0 && self.channel.is_some() {
            let owed = self.owed[0];
            let mut frame = [0u8; wire::FRAME_MAX];
            let Some(n) = self.encode(owed, &mut frame) else {
                self.unowe(0);
                continue;
            };
            match self.push(&frame[..n]) {
                Pushed::Sent => {
                    self.unowe(0);
                    if let Owed::Attach(device) = owed {
                        if let Some(a) =
                            self.attached.iter_mut().flatten().find(|a| a.device == device)
                        {
                            a.announced = true;
                        }
                    }
                }
                Pushed::Full => {
                    self.retry_at = Some(now.saturating_add(RETRY_NS));
                    return;
                }
                Pushed::Gone => return,
            }
        }
        self.retry_at = None;
        for i in 0..MAX_INTERFACES {
            self.send_reports(i);
        }
    }

    fn attach(
        &mut self,
        slot: u8,
        interface: u8,
        role: HidRole,
        max_packet: u16,
        vendor: u16,
        product: u16,
    ) {
        // The core notes an interface once per configuration; a second note replaces the first.
        self.detach(|a| a.slot == slot && a.interface == interface);
        let Some(free) = self.attached.iter().position(Option::is_none) else {
            self.refused = self.refused.saturating_add(1);
            return;
        };
        let device = self.next_device();
        self.attached[free] = Some(Attached {
            device,
            slot,
            interface,
            role,
            vendor,
            product,
            max_packet,
            announced: false,
            reports: wire::ReportList::new(),
        });
        if self.channel.is_some() {
            self.owe(Owed::Attach(device));
        }
    }

    fn report(&mut self, slot: u8, interface: u8, bytes: &[u8]) {
        if self.channel.is_none() {
            return;
        }
        let Some(i) = self.find(slot, interface) else { return };
        let Some(a) = self.attached[i].as_mut() else { return };
        if !a.announced {
            self.dropped = self.dropped.saturating_add(1);
            return;
        }
        match a.reports.push(bytes) {
            wire::Pushed::Added => {}
            wire::Pushed::Refused => self.refused = self.refused.saturating_add(1),
            wire::Pushed::Full => {
                self.send_reports(i);
                if let Some(a) = self.attached[i].as_mut() {
                    if a.reports.push(bytes) != wire::Pushed::Added {
                        self.dropped = self.dropped.saturating_add(1);
                    }
                }
            }
        }
    }

    /// Every interface `gone` names leaves the table: its last reports go out, then its detach
    /// is owed — or, if its attach was still owed, the attach is withdrawn (the client never
    /// heard of it).
    fn detach(&mut self, gone: impl Fn(&Attached) -> bool) {
        for i in 0..MAX_INTERFACES {
            let Some(device) = self.attached[i].as_ref().filter(|a| gone(a)).map(|a| a.device)
            else {
                continue;
            };
            self.send_reports(i);
            self.attached[i] = None;
            if self.channel.is_none() {
                continue;
            }
            match self.owed[..self.owed_len].iter().position(|o| *o == Owed::Attach(device)) {
                Some(at) => self.unowe(at),
                None => self.owe(Owed::Detach(device)),
            }
        }
    }

    /// Interface `i`'s reports of this drain as one frame (if it has any and was announced).
    fn send_reports(&mut self, i: usize) {
        let Some(a) = self.attached[i].as_mut() else { return };
        if a.reports.is_empty() {
            return;
        }
        let count = u32::from(a.reports.count());
        let mut frame = [0u8; wire::FRAME_MAX];
        let encoded = if a.announced { a.reports.encode(a.device, &mut frame) } else { None };
        a.reports.clear();
        let Some(n) = encoded else {
            self.dropped = self.dropped.saturating_add(count);
            return;
        };
        if self.push(&frame[..n]) != Pushed::Sent {
            self.dropped = self.dropped.saturating_add(count);
        }
    }

    fn push(&mut self, frame: &[u8]) -> Pushed {
        let Some(channel) = self.channel.as_mut() else { return Pushed::Gone };
        let pushed = channel.push(frame);
        if pushed == Pushed::Gone {
            self.end();
        }
        pushed
    }

    /// The subscriber is gone: nothing is owed to anyone; the interfaces stay attached for the
    /// next subscriber.
    fn end(&mut self) {
        self.channel = None;
        self.owed_len = 0;
        self.retry_at = None;
        for a in self.attached.iter_mut().flatten() {
            a.announced = false;
            a.reports.clear();
        }
    }

    fn encode(&self, owed: Owed, out: &mut [u8; wire::FRAME_MAX]) -> Option<usize> {
        match owed {
            Owed::Answer => put(out, &wire::encode_subscribed(wire::STATUS_OK)),
            Owed::Attach(device) => {
                let a = self.attached.iter().flatten().find(|a| a.device == device)?;
                let role = role_byte(a.role);
                let frame = wire::encode_attached(
                    device,
                    a.vendor,
                    a.product,
                    a.interface,
                    role,
                    a.max_packet,
                );
                put(out, &frame)
            }
            Owed::Detach(device) => put(out, &wire::encode_detached(device)),
        }
    }

    fn find(&self, slot: u8, interface: u8) -> Option<usize> {
        self.attached
            .iter()
            .position(|a| a.as_ref().is_some_and(|a| a.slot == slot && a.interface == interface))
    }

    /// The next device id: never 0, never one the table or the queue still names.
    fn next_device(&mut self) -> u16 {
        loop {
            self.last_device = self.last_device.wrapping_add(1);
            let d = self.last_device;
            let named = self.attached.iter().flatten().any(|a| a.device == d)
                || self.owed[..self.owed_len]
                    .iter()
                    .any(|o| matches!(*o, Owed::Attach(x) | Owed::Detach(x) if x == d));
            if d != 0 && !named {
                return d;
            }
        }
    }

    fn owe(&mut self, owed: Owed) {
        // Bounded by construction (an answer, then at most an attach and a detach per
        // interface); a full queue would mean a broken invariant — the frame is not owed then.
        if self.owed_len < OWED_MAX {
            self.owed[self.owed_len] = owed;
            self.owed_len += 1;
        }
    }

    fn unowe(&mut self, at: usize) {
        if at < self.owed_len {
            self.owed.copy_within(at + 1..self.owed_len, at);
            self.owed_len -= 1;
        }
    }
}

fn put(out: &mut [u8], frame: &[u8]) -> Option<usize> {
    out.get_mut(..frame.len())?.copy_from_slice(frame);
    Some(frame.len())
}

const fn role_byte(role: HidRole) -> u8 {
    match role {
        HidRole::Keyboard => wire::ROLE_KEYBOARD,
        HidRole::Mouse => wire::ROLE_MOUSE,
    }
}
