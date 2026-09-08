// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: ingressd wire v1 (RFC-0092 §3): magic `I`,`G`, version 1,
//! `OP_EXPOSE / OP_UNEXPOSE / OP_EXPOSE_STATUS`, statuses and the deny
//! reason vocabulary. Fixed-size little-endian frames; every decode is
//! bounds-checked and anything unexpected is `Malformed` (fail closed).
//! The host IDL twin is `tools/nexus-idl/schemas/ingress.capnp`
//! (`tests/ingress_host/` pins the two vocabularies together).
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! API_STABILITY: Unstable (additive only)
//! TEST_COVERAGE: tests/wire_contract.rs

use crate::table::Proto;

pub const MAGIC0: u8 = b'I';
pub const MAGIC1: u8 = b'G';
pub const VERSION: u8 = 1;

pub const OP_EXPOSE: u8 = 1;
pub const OP_UNEXPOSE: u8 = 2;
pub const OP_EXPOSE_STATUS: u8 = 3;

pub const STATUS_ALLOW: u8 = 0;
pub const STATUS_DENY: u8 = 1;
pub const STATUS_MALFORMED: u8 = 2;
pub const STATUS_UNSUPPORTED: u8 = 3;

/// `[I][G][v][op][nonce u32][port u16][proto u8]`.
pub const REQUEST_LEN: usize = 11;
/// `[I][G][v][op][nonce u32][status][reason]`.
pub const EXPOSE_REPLY_LEN: usize = 10;
/// `[I][G][v][3][nonce u32][status][open][accepted u32][denied_cidr u32][denied_rate u32]`.
pub const STATUS_REPLY_LEN: usize = 22;

/// Why the gateway refused (RFC-0092 failure model). `Policy..=Tls` travel
/// on the wire (intent replies); `Cidr`/`Rate` are accept-side labels for
/// markers and counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Reason {
    None = 0,
    /// No declared exposure, capability refused, or authority unreachable.
    Policy = 1,
    /// Sender is not the declared subject.
    Identity = 2,
    /// A bound (open exposures, links) is exhausted.
    Limit = 3,
    /// Termination slot not delivered.
    Tls = 4,
    /// Peer outside `cidr_allow`.
    Cidr = 5,
    /// Token bucket empty.
    Rate = 6,
}

impl Reason {
    /// Stable marker label (`ingressd: deny (reason=<label>)`).
    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Policy => "policy",
            Self::Identity => "identity",
            Self::Limit => "limit",
            Self::Tls => "tls",
            Self::Cidr => "cidr",
            Self::Rate => "rate",
        }
    }

    /// Wire byte → reason (intent replies carry `0..=4` only).
    pub const fn from_wire(b: u8) -> Option<Self> {
        match b {
            0 => Some(Self::None),
            1 => Some(Self::Policy),
            2 => Some(Self::Identity),
            3 => Some(Self::Limit),
            4 => Some(Self::Tls),
            5 => Some(Self::Cidr),
            6 => Some(Self::Rate),
            _ => None,
        }
    }
}

/// A decoded intent request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Request {
    pub op: u8,
    pub nonce: u32,
    pub port: u16,
    pub proto: Proto,
}

/// Decode failure classes (each maps to one status byte).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    Malformed,
    /// Well-formed header, unknown op.
    Unsupported {
        op: u8,
        nonce: u32,
    },
}

/// Decodes one request frame; exact length, magic and version required.
pub fn decode_request(frame: &[u8]) -> Result<Request, DecodeError> {
    if frame.len() != REQUEST_LEN || frame[0] != MAGIC0 || frame[1] != MAGIC1 || frame[2] != VERSION
    {
        return Err(DecodeError::Malformed);
    }
    let op = frame[3];
    let nonce = u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
    if !matches!(op, OP_EXPOSE | OP_UNEXPOSE | OP_EXPOSE_STATUS) {
        return Err(DecodeError::Unsupported { op, nonce });
    }
    let port = u16::from_le_bytes([frame[8], frame[9]]);
    let proto = Proto::from_wire(frame[10]).ok_or(DecodeError::Malformed)?;
    if port == 0 {
        return Err(DecodeError::Malformed);
    }
    Ok(Request { op, nonce, port, proto })
}

/// Encodes a request (client side / tests).
pub fn encode_request(
    op: u8,
    nonce: u32,
    port: u16,
    proto: Proto,
    out: &mut [u8],
) -> Option<usize> {
    let dst = out.get_mut(..REQUEST_LEN)?;
    dst[0] = MAGIC0;
    dst[1] = MAGIC1;
    dst[2] = VERSION;
    dst[3] = op;
    dst[4..8].copy_from_slice(&nonce.to_le_bytes());
    dst[8..10].copy_from_slice(&port.to_le_bytes());
    dst[10] = proto as u8;
    Some(REQUEST_LEN)
}

/// Encodes an `OP_EXPOSE` / `OP_UNEXPOSE` (or error) reply.
pub fn encode_expose_reply(
    op: u8,
    nonce: u32,
    status: u8,
    reason: Reason,
    out: &mut [u8],
) -> Option<usize> {
    let dst = out.get_mut(..EXPOSE_REPLY_LEN)?;
    dst[0] = MAGIC0;
    dst[1] = MAGIC1;
    dst[2] = VERSION;
    dst[3] = op;
    dst[4..8].copy_from_slice(&nonce.to_le_bytes());
    dst[8] = status;
    dst[9] = reason as u8;
    Some(EXPOSE_REPLY_LEN)
}

/// Per-exposure observability counters (saturating).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counters {
    pub accepted: u32,
    pub denied_cidr: u32,
    pub denied_rate: u32,
}

/// Encodes an `OP_EXPOSE_STATUS` reply.
pub fn encode_status_reply(
    nonce: u32,
    status: u8,
    open: bool,
    counters: Counters,
    out: &mut [u8],
) -> Option<usize> {
    let dst = out.get_mut(..STATUS_REPLY_LEN)?;
    dst[0] = MAGIC0;
    dst[1] = MAGIC1;
    dst[2] = VERSION;
    dst[3] = OP_EXPOSE_STATUS;
    dst[4..8].copy_from_slice(&nonce.to_le_bytes());
    dst[8] = status;
    dst[9] = u8::from(open);
    dst[10..14].copy_from_slice(&counters.accepted.to_le_bytes());
    dst[14..18].copy_from_slice(&counters.denied_cidr.to_le_bytes());
    dst[18..22].copy_from_slice(&counters.denied_rate.to_le_bytes());
    Some(STATUS_REPLY_LEN)
}

/// A decoded reply of either shape (client side / tests).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reply {
    pub op: u8,
    pub nonce: u32,
    pub status: u8,
    /// `OP_EXPOSE`/`OP_UNEXPOSE`: the deny reason; status replies carry `None`.
    pub reason: Reason,
    pub open: bool,
    pub counters: Counters,
}

/// Decodes a reply frame of either length.
pub fn decode_reply(frame: &[u8]) -> Option<Reply> {
    if frame.len() < EXPOSE_REPLY_LEN
        || frame[0] != MAGIC0
        || frame[1] != MAGIC1
        || frame[2] != VERSION
    {
        return None;
    }
    let op = frame[3];
    let nonce = u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
    let status = frame[8];
    // A status read that succeeded is the long shape; every other reply
    // (intent verdicts, denies of any op, malformed/unsupported) is short.
    let long = op == OP_EXPOSE_STATUS && status == STATUS_ALLOW;
    match (long, frame.len()) {
        (false, EXPOSE_REPLY_LEN) => Some(Reply {
            op,
            nonce,
            status,
            reason: Reason::from_wire(frame[9])?,
            open: false,
            counters: Counters::default(),
        }),
        (true, STATUS_REPLY_LEN) => {
            let u =
                |i: usize| u32::from_le_bytes([frame[i], frame[i + 1], frame[i + 2], frame[i + 3]]);
            Some(Reply {
                op,
                nonce,
                status,
                reason: Reason::None,
                open: frame[9] == 1,
                counters: Counters { accepted: u(10), denied_cidr: u(14), denied_rate: u(18) },
            })
        }
        _ => None,
    }
}
