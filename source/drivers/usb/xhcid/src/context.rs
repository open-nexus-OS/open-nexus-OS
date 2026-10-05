// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Device and input contexts (xHCI 1.2 §6.2): the slot context, the endpoint contexts and the
//! input control context, at the controller's context size (32 or 64 bytes); endpoint indices
//! (DCI) and interval exponents.

use nexus_usb::Speed;

/// Endpoint types (the endpoint context's EP Type).
pub const EP_CONTROL: u8 = 4;
pub const EP_INTERRUPT_IN: u8 = 7;
/// Errors a TD may take before the controller halts the endpoint.
const CERR: u32 = 3;

/// Where the contexts sit in a device context and an input context.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    /// Bytes per context.
    pub size: usize,
}

impl Layout {
    /// A device (output) context: the slot and 31 endpoints.
    #[must_use]
    pub const fn device_len(self) -> usize {
        32 * self.size
    }

    /// An input context: the input control context, the slot and 31 endpoints.
    #[must_use]
    pub const fn input_len(self) -> usize {
        33 * self.size
    }

    /// Context `dci` (0 = the slot) in an input context.
    #[must_use]
    pub const fn input(self, dci: u8) -> usize {
        (dci as usize + 1) * self.size
    }

    /// Context `dci` (0 = the slot) in a device context.
    #[must_use]
    pub const fn output(self, dci: u8) -> usize {
        dci as usize * self.size
    }
}

/// The device context index of an endpoint address (EP0 = 1; OUT n = 2n, IN n = 2n + 1).
#[must_use]
pub const fn dci(address: u8) -> u8 {
    let number = address & 0x0f;
    if number == 0 {
        1
    } else {
        number * 2 + (address >> 7)
    }
}

/// The speed ID xHCI's default protocol speed table gives a speed.
#[must_use]
pub const fn speed_id(speed: Speed) -> u8 {
    match speed {
        Speed::Full => 1,
        Speed::Low => 2,
        Speed::High => 3,
        Speed::Super => 4,
    }
}

/// The speed a port's speed ID names (the default table).
#[must_use]
pub const fn speed_of(id: u8) -> Option<Speed> {
    match id {
        1 => Some(Speed::Full),
        2 => Some(Speed::Low),
        3 => Some(Speed::High),
        4 => Some(Speed::Super),
        _ => None,
    }
}

/// An interrupt endpoint's interval as the context's exponent (period = 2^n × 125 µs):
/// full/low speed `bInterval` counts frames (1 ms) — the largest power of two not above it,
/// 3..=10; high and SuperSpeed `bInterval` is already an exponent plus one, 1..=16.
#[must_use]
pub fn interval_exponent(speed: Speed, b_interval: u8) -> u8 {
    match speed {
        Speed::Low | Speed::Full => {
            let frames = u32::from(b_interval.max(1));
            (3 + (31 - frames.leading_zeros()) as u8).min(10)
        }
        Speed::High | Speed::Super => b_interval.clamp(1, 16) - 1,
    }
}

/// The slot context.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Slot {
    pub route: u32,
    pub speed_id: u8,
    /// The hub runs a transaction translator per port.
    pub mtt: bool,
    /// The device is a hub.
    pub hub: bool,
    /// The last valid endpoint context (DCI).
    pub entries: u8,
    pub root_port: u8,
    /// A hub's port count.
    pub ports: u8,
    /// The high-speed hub whose TT serves a full/low-speed device: its slot and port.
    pub tt_slot: u8,
    pub tt_port: u8,
    /// A high-speed hub's TT think time (0..=3).
    pub ttt: u8,
}

impl Slot {
    /// Write the context's first four words (`out` is one context).
    pub fn write(&self, out: &mut [u8]) {
        let words = [
            (self.route & 0xf_ffff)
                | (u32::from(self.speed_id) << 20)
                | (u32::from(self.mtt) << 25)
                | (u32::from(self.hub) << 26)
                | (u32::from(self.entries) << 27),
            (u32::from(self.root_port) << 16) | (u32::from(self.ports) << 24),
            u32::from(self.tt_slot) | (u32::from(self.tt_port) << 8) | (u32::from(self.ttt) << 16),
            0,
        ];
        put_words(out, &words);
    }
}

/// An endpoint context.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Endpoint {
    pub kind: u8,
    pub max_packet: u16,
    pub interval: u8,
    /// The transfer ring's first TRB with its cycle state in bit 0.
    pub dequeue: u64,
    pub average_trb: u16,
    /// Bytes per service interval (interrupt endpoints).
    pub max_esit: u16,
}

impl Endpoint {
    /// Write the context's first five words (`out` is one context).
    pub fn write(&self, out: &mut [u8]) {
        let words = [
            u32::from(self.interval) << 16,
            (CERR << 1) | (u32::from(self.kind) << 3) | (u32::from(self.max_packet) << 16),
            self.dequeue as u32,
            (self.dequeue >> 32) as u32,
            u32::from(self.average_trb) | (u32::from(self.max_esit) << 16),
        ];
        put_words(out, &words);
    }
}

/// The input control context: which contexts the command drops and adds.
pub fn input_control(out: &mut [u8], drop: u32, add: u32) {
    put_words(out, &[drop, add]);
}

/// The slot state of a device context's slot context (0 disabled, 1 default, 2 addressed,
/// 3 configured).
#[must_use]
pub fn slot_state(slot_context: &[u8]) -> u8 {
    slot_context.get(15).map_or(0, |b| b >> 3)
}

fn put_words(out: &mut [u8], words: &[u32]) {
    for (chunk, word) in out.chunks_exact_mut(4).zip(words) {
        chunk.copy_from_slice(&word.to_le_bytes());
    }
}
