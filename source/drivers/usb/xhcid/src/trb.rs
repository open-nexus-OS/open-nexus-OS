// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Transfer Request Blocks (xHCI 1.2 §6.4): the 16-byte entries of every ring — the transfer
//! TRBs a TD is made of, the commands, the link, and the events the controller writes back.

/// A TRB: a 64-bit parameter, a status word and a control word (bit 0 the cycle bit).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Trb {
    /// Parameter (a buffer, a context, a TRB pointer, or inline data).
    pub param: u64,
    /// Status (lengths, interrupter target, completion code).
    pub status: u32,
    /// Control (cycle, flags, type, slot and endpoint).
    pub control: u32,
}

pub const NORMAL: u8 = 1;
pub const SETUP: u8 = 2;
pub const DATA: u8 = 3;
pub const STATUS: u8 = 4;
pub const LINK: u8 = 6;
pub const ENABLE_SLOT: u8 = 9;
pub const DISABLE_SLOT: u8 = 10;
pub const ADDRESS_DEVICE: u8 = 11;
pub const CONFIGURE_ENDPOINT: u8 = 12;
pub const EVALUATE_CONTEXT: u8 = 13;
pub const RESET_ENDPOINT: u8 = 14;
pub const SET_TR_DEQUEUE: u8 = 16;
pub const TRANSFER_EVENT: u8 = 32;
pub const COMMAND_COMPLETION: u8 = 33;
pub const PORT_STATUS_CHANGE: u8 = 34;
pub const HOST_CONTROLLER: u8 = 37;

/// Completion codes (xHCI 1.2 §6.4.5).
pub const CC_SUCCESS: u8 = 1;
pub const CC_STALL: u8 = 6;
pub const CC_SHORT_PACKET: u8 = 13;
/// No completion arrived before the deadline (not a controller code).
pub const CC_TIMEOUT: u8 = 0xff;

pub const CYCLE: u32 = 1 << 0;
const TOGGLE_CYCLE: u32 = 1 << 1;
const ISP: u32 = 1 << 2;
pub const CHAIN: u32 = 1 << 4;
const IOC: u32 = 1 << 5;
const IDT: u32 = 1 << 6;
const DIR_IN: u32 = 1 << 16;
/// Setup stage: transfer type (bits 17:16) — no data, OUT data, IN data.
const TRT_NONE: u32 = 0;
const TRT_OUT: u32 = 2 << 16;
const TRT_IN: u32 = 3 << 16;

const fn kind(t: u8) -> u32 {
    (t as u32) << 10
}

const fn slot(id: u8) -> u32 {
    (id as u32) << 24
}

const fn endpoint(dci: u8) -> u32 {
    ((dci as u32) & 0x1f) << 16
}

impl Trb {
    /// The TRB's bytes as the ring holds them.
    #[must_use]
    pub fn to_bytes(self) -> [u8; 16] {
        let mut out = [0u8; 16];
        out[..8].copy_from_slice(&self.param.to_le_bytes());
        out[8..12].copy_from_slice(&self.status.to_le_bytes());
        out[12..].copy_from_slice(&self.control.to_le_bytes());
        out
    }

    /// A TRB from 16 bytes; zeros past a short slice.
    #[must_use]
    pub fn from_bytes(b: &[u8]) -> Self {
        let word = |at: usize| {
            let mut w = [0u8; 4];
            for (i, byte) in w.iter_mut().enumerate() {
                *byte = b.get(at + i).copied().unwrap_or(0);
            }
            u32::from_le_bytes(w)
        };
        Self {
            param: u64::from(word(0)) | (u64::from(word(4)) << 32),
            status: word(8),
            control: word(12),
        }
    }

    /// The TRB type.
    #[must_use]
    pub const fn kind(&self) -> u8 {
        ((self.control >> 10) & 0x3f) as u8
    }

    /// The cycle bit.
    #[must_use]
    pub const fn cycle(&self) -> bool {
        self.control & CYCLE != 0
    }

    /// An event's completion code.
    #[must_use]
    pub const fn completion(&self) -> u8 {
        (self.status >> 24) as u8
    }

    /// An event's bytes not transferred (a transfer event's residual).
    #[must_use]
    pub const fn residual(&self) -> u32 {
        self.status & 0x00ff_ffff
    }

    /// An event's slot ID.
    #[must_use]
    pub const fn slot_id(&self) -> u8 {
        (self.control >> 24) as u8
    }

    /// A transfer event's endpoint (DCI).
    #[must_use]
    pub const fn endpoint_id(&self) -> u8 {
        ((self.control >> 16) & 0x1f) as u8
    }

    /// A port status change event's port.
    #[must_use]
    pub const fn port_id(&self) -> u8 {
        (self.param >> 24) as u8
    }

    /// A control transfer's setup stage, the eight bytes inline.
    #[must_use]
    pub fn setup(packet: [u8; 8], data_len: u16, data_in: bool) -> Self {
        let trt = match (data_len, data_in) {
            (0, _) => TRT_NONE,
            (_, true) => TRT_IN,
            (_, false) => TRT_OUT,
        };
        Self { param: u64::from_le_bytes(packet), status: 8, control: kind(SETUP) | IDT | trt }
    }

    /// A control transfer's data stage; a short packet raises an event (ISP).
    #[must_use]
    pub const fn data(buffer: u64, len: u16, data_in: bool) -> Self {
        Self {
            param: buffer,
            status: len as u32,
            control: kind(DATA) | ISP | if data_in { DIR_IN } else { 0 },
        }
    }

    /// A control transfer's status stage (opposite the data, IN without data); it completes
    /// the TD.
    #[must_use]
    pub const fn status_stage(data_len: u16, data_in: bool) -> Self {
        let dir_in = data_len == 0 || !data_in;
        Self { param: 0, status: 0, control: kind(STATUS) | IOC | if dir_in { DIR_IN } else { 0 } }
    }

    /// A normal TRB: one buffer of an interrupt or bulk TD, completing it.
    #[must_use]
    pub const fn normal(buffer: u64, len: u16) -> Self {
        Self { param: buffer, status: len as u32, control: kind(NORMAL) | ISP | IOC }
    }

    /// The link at a segment's end back to its start, toggling the producer's cycle.
    #[must_use]
    pub const fn link(target: u64) -> Self {
        Self { param: target, status: 0, control: kind(LINK) | TOGGLE_CYCLE }
    }

    /// Enable Slot.
    #[must_use]
    pub const fn enable_slot() -> Self {
        Self { param: 0, status: 0, control: kind(ENABLE_SLOT) }
    }

    /// Disable Slot.
    #[must_use]
    pub const fn disable_slot(id: u8) -> Self {
        Self { param: 0, status: 0, control: kind(DISABLE_SLOT) | slot(id) }
    }

    /// Address Device with the input context at `input` (SET_ADDRESS sent: BSR = 0).
    #[must_use]
    pub const fn address_device(input: u64, id: u8) -> Self {
        Self { param: input, status: 0, control: kind(ADDRESS_DEVICE) | slot(id) }
    }

    /// Configure Endpoint with the input context at `input`.
    #[must_use]
    pub const fn configure_endpoint(input: u64, id: u8) -> Self {
        Self { param: input, status: 0, control: kind(CONFIGURE_ENDPOINT) | slot(id) }
    }

    /// Evaluate Context with the input context at `input`.
    #[must_use]
    pub const fn evaluate_context(input: u64, id: u8) -> Self {
        Self { param: input, status: 0, control: kind(EVALUATE_CONTEXT) | slot(id) }
    }

    /// Reset Endpoint (a halted endpoint back to stopped; the TD is not kept).
    #[must_use]
    pub const fn reset_endpoint(id: u8, dci: u8) -> Self {
        Self { param: 0, status: 0, control: kind(RESET_ENDPOINT) | endpoint(dci) | slot(id) }
    }

    /// Set TR Dequeue Pointer: the ring continues at `dequeue` (bit 0 = its cycle state).
    #[must_use]
    pub const fn set_tr_dequeue(id: u8, dci: u8, dequeue: u64) -> Self {
        Self { param: dequeue, status: 0, control: kind(SET_TR_DEQUEUE) | endpoint(dci) | slot(id) }
    }
}
