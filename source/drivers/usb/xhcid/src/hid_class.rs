// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The HID boot class server (RFC-0099 §5, TASK-0253B): what xhcid owes its one HID client.
//! The core notes what happened on the bus; [`HidClass::note`] keeps the notes about HID boot
//! interfaces — one attachment per interface, named by a device id never reused while the
//! client could still hold it — and turns them into the class's frames (`nexus_wire::usb`) on
//! the subscriber's push channel: the answer to its SUBSCRIBE, an attach per interface (every
//! one attached so far, for a new subscriber), each drain's reports, a detach.
//!
//! Delivery: attaches, detaches, the answer and the report frames the channel refused are OWED —
//! kept in order until the channel takes them; a full channel is tried again on the next wake or
//! after [`RETRY_NS`]. A boot keyboard report carries the keys held at that moment: a press and
//! its release in two reports, the first one dropped, is a letter lost for good (RFC-0113 §2 —
//! a key is never dropped for a full queue). So a keyboard's frame takes any of [`HELD_MAX`]
//! places, a pointer's only [`HELD_POINTER_MAX`] (a dropped pointer frame loses its deltas only);
//! with no place left a frame is dropped and counted, a keyboard's as lost keys. Nothing
//! overtakes: a report never its interface's attach, a frame never one owed before it, and a
//! detaching interface's last reports go out before its detach. Pure and allocation-free: the OS
//! loop hands it a [`Channel`] on the moved capability, the tests a recorder.

use nexus_usb::HidRole;
use nexus_wire::usb as wire;

use crate::Note;

/// HID boot interfaces attached at once at most (keyboards and mice on the whole bus).
pub const MAX_INTERFACES: usize = 16;
/// Report frames held at most while the client is full.
pub const HELD_MAX: usize = 8;
/// Of them, a pointer's at most: the keys keep the rest.
pub const HELD_POINTER_MAX: usize = 4;
/// Frames owed at most: the answer, an attach and a detach per interface, the held reports.
const OWED_MAX: usize = 2 * MAX_INTERFACES + 1 + HELD_MAX;
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
    /// The report frame held in that place.
    Reports(u8),
}

/// A report frame the full channel refused, waiting its turn.
struct Held {
    frame: [u8; wire::FRAME_MAX],
    len: usize,
    count: u32,
    keyboard: bool,
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
    held: [Option<Held>; HELD_MAX],
    /// Keyboard reports dropped (no place to hold them): keys lost.
    keys_lost: u32,
    /// The time of the last flush (a held frame's retry is armed from it).
    now: u64,
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
            held: [const { None }; HELD_MAX],
            keys_lost: 0,
            now: 0,
        }
    }

    /// An admitted subscriber (policyd granted its sender the class): it replaces the one
    /// before — returned, for the caller to close — and is owed the answer, then an attach for
    /// every interface attached so far. Nothing is sent before the next [`Self::flush`].
    pub fn subscribe(&mut self, channel: C) -> Option<C> {
        let old = self.channel.replace(channel);
        self.owed_len = 0;
        self.drop_held();
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

    /// Keyboard reports dropped for want of a place to hold them: keys lost (RFC-0113 §2).
    #[must_use]
    pub const fn keys_lost(&self) -> u32 {
        self.keys_lost
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
        self.now = now;
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
                    match owed {
                        Owed::Attach(device) => {
                            if let Some(a) =
                                self.attached.iter_mut().flatten().find(|a| a.device == device)
                            {
                                a.announced = true;
                            }
                        }
                        Owed::Reports(at) => self.held[usize::from(at)] = None,
                        Owed::Answer | Owed::Detach(_) => {}
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

    /// Interface `i`'s reports of this drain as one frame (if it has any and was announced) —
    /// at once when nothing is owed before it, else held behind what is.
    fn send_reports(&mut self, i: usize) {
        let Some(a) = self.attached[i].as_mut() else { return };
        if a.reports.is_empty() {
            return;
        }
        let count = u32::from(a.reports.count());
        let keyboard = a.role == HidRole::Keyboard;
        let mut frame = [0u8; wire::FRAME_MAX];
        let encoded = if a.announced { a.reports.encode(a.device, &mut frame) } else { None };
        a.reports.clear();
        let Some(n) = encoded else {
            self.dropped = self.dropped.saturating_add(count);
            return;
        };
        if self.owed_len == 0 {
            match self.push(&frame[..n]) {
                Pushed::Sent => return,
                Pushed::Gone => {
                    self.dropped = self.dropped.saturating_add(count);
                    return;
                }
                Pushed::Full => {}
            }
        }
        self.hold(&frame[..n], count, keyboard);
    }

    /// A report frame the client cannot take yet: owed in order behind what is owed already. A
    /// keyboard's takes any free place, a pointer's only its share; with no place the frame is
    /// dropped — a keyboard's counted as keys lost.
    fn hold(&mut self, frame: &[u8], count: u32, keyboard: bool) {
        let pointers = self.held.iter().flatten().filter(|h| !h.keyboard).count();
        let room = keyboard || pointers < HELD_POINTER_MAX;
        let free = self.held.iter().position(Option::is_none).filter(|_| room);
        let (Some(at), true) = (free, self.channel.is_some()) else {
            self.dropped = self.dropped.saturating_add(count);
            if keyboard {
                self.keys_lost = self.keys_lost.saturating_add(count);
            }
            return;
        };
        let mut held = Held { frame: [0; wire::FRAME_MAX], len: frame.len(), count, keyboard };
        held.frame[..frame.len()].copy_from_slice(frame);
        self.held[at] = Some(held);
        self.owe(Owed::Reports(at as u8));
        if self.retry_at.is_none() {
            self.retry_at = Some(self.now.saturating_add(RETRY_NS));
        }
    }

    /// Every held frame let go (the subscriber changed or died), counted as dropped.
    fn drop_held(&mut self) {
        for place in self.held.iter_mut() {
            if let Some(h) = place.take() {
                self.dropped = self.dropped.saturating_add(h.count);
                if h.keyboard {
                    self.keys_lost = self.keys_lost.saturating_add(h.count);
                }
            }
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
        self.drop_held();
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
            Owed::Reports(at) => {
                let held = self.held.get(usize::from(at))?.as_ref()?;
                put(out, &held.frame[..held.len])
            }
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
