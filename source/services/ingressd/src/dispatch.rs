// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Wire-level dispatcher: one request frame from a kernel-attributed
//! sender → registry verdict → one reply frame + the behaviour event the OS
//! loop turns into markers/counters (`ingressd: port open (…)`,
//! `ingressd: deny (reason=…)`). Malformed input answers `STATUS_MALFORMED`,
//! unknown ops `STATUS_UNSUPPORTED`; no path panics on untrusted bytes.
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! TEST_COVERAGE: tests/ingress_host/, tests/wire_contract.rs

use crate::intent::{IntentHost, Registry};
use crate::table::Proto;
use crate::wire::{
    decode_request, encode_expose_reply, encode_status_reply, DecodeError, Reason, OP_EXPOSE,
    OP_EXPOSE_STATUS, OP_UNEXPOSE, STATUS_ALLOW, STATUS_DENY, STATUS_MALFORMED, STATUS_UNSUPPORTED,
};

/// What happened, for markers and counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// Exposure `idx` is open (fresh or idempotent re-open).
    Opened {
        idx: usize,
        port: u16,
        proto: Proto,
    },
    /// Exposure `idx` was closed by its owner.
    Closed {
        idx: usize,
        port: u16,
        proto: Proto,
    },
    /// Status read served.
    Status,
    /// Refused with a reason (the sender is the accountable subject).
    Denied {
        reason: Reason,
    },
    /// Frame rejected before any verdict.
    Malformed,
    Unsupported,
}

/// Reply bytes written + the event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outcome {
    pub reply_len: usize,
    pub event: Event,
}

/// Handles one frame. `out` must hold `STATUS_REPLY_LEN` bytes; a smaller
/// buffer yields `reply_len = 0` (nothing sent — never a truncated frame).
pub fn handle_frame<const N: usize>(
    reg: &mut Registry<'_, N>,
    host: &mut impl IntentHost,
    sender: u64,
    frame: &[u8],
    out: &mut [u8],
) -> Outcome {
    let req = match decode_request(frame) {
        Ok(r) => r,
        Err(DecodeError::Malformed) => {
            let n = encode_expose_reply(0, 0, STATUS_MALFORMED, Reason::None, out).unwrap_or(0);
            return Outcome { reply_len: n, event: Event::Malformed };
        }
        Err(DecodeError::Unsupported { op, nonce }) => {
            let n =
                encode_expose_reply(op, nonce, STATUS_UNSUPPORTED, Reason::None, out).unwrap_or(0);
            return Outcome { reply_len: n, event: Event::Unsupported };
        }
    };
    match req.op {
        OP_EXPOSE => match reg.open(host, sender, req.port, req.proto) {
            Ok(idx) => Outcome {
                reply_len: encode_expose_reply(req.op, req.nonce, STATUS_ALLOW, Reason::None, out)
                    .unwrap_or(0),
                event: Event::Opened { idx, port: req.port, proto: req.proto },
            },
            Err(reason) => deny(req.op, req.nonce, reason, out),
        },
        OP_UNEXPOSE => match reg.close(sender, req.port, req.proto) {
            Ok(idx) => Outcome {
                reply_len: encode_expose_reply(req.op, req.nonce, STATUS_ALLOW, Reason::None, out)
                    .unwrap_or(0),
                event: Event::Closed { idx, port: req.port, proto: req.proto },
            },
            Err(reason) => deny(req.op, req.nonce, reason, out),
        },
        OP_EXPOSE_STATUS => match reg.status(host, sender, req.port, req.proto) {
            Ok((open, counters)) => Outcome {
                reply_len: encode_status_reply(req.nonce, STATUS_ALLOW, open, counters, out)
                    .unwrap_or(0),
                event: Event::Status,
            },
            Err(reason) => deny(req.op, req.nonce, reason, out),
        },
        _ => Outcome {
            reply_len: encode_expose_reply(
                req.op,
                req.nonce,
                STATUS_UNSUPPORTED,
                Reason::None,
                out,
            )
            .unwrap_or(0),
            event: Event::Unsupported,
        },
    }
}

fn deny(op: u8, nonce: u32, reason: Reason, out: &mut [u8]) -> Outcome {
    Outcome {
        reply_len: encode_expose_reply(op, nonce, STATUS_DENY, reason, out).unwrap_or(0),
        event: Event::Denied { reason },
    }
}
