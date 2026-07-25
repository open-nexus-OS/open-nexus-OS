// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the shared 4-byte frame envelope every message on windowd's SERVER
//! endpoint carries — `[b'I', b'N', version, op]`, the input-live-protocol
//! header family that the client-surface ops (`crate::client_surface`) and the
//! input ops 1–4 share. Split out of `client_surface.rs` so the "is this frame
//! even mine?" question has one small home: a consumer of a DIFFERENT endpoint
//! must be able to recognise this envelope and refuse to swallow the frame.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Stable shape (the magic bytes are a wire contract)
//! TEST_COVERAGE: tests/client_envelope.rs

/// Shared envelope (input-live-protocol family on windowd's server endpoint).
pub const ENVELOPE_MAGIC0: u8 = b'I';
pub const ENVELOPE_MAGIC1: u8 = b'N';
pub const ENVELOPE_VERSION: u8 = 1;
pub(crate) const HEADER_LEN: usize = 4;

pub(crate) fn header(op: u8) -> [u8; HEADER_LEN] {
    [ENVELOPE_MAGIC0, ENVELOPE_MAGIC1, ENVELOPE_VERSION, op]
}

pub(crate) fn has_op(frame: &[u8], op: u8) -> bool {
    is_client_envelope(frame) && frame[3] == op
}

/// True if `frame` carries this envelope — i.e. it is addressed to windowd's
/// SERVER endpoint (any client-surface or input-live op).
///
/// Exists so a consumer of a DIFFERENT endpoint can detect that it is draining
/// traffic it does not own instead of discarding it: windowd's gpud-reply drain
/// read such frames as unknown present verdicts and dropped them, and on
/// 2026-07-25 that silently ate the desktop's events-attach and geometry intent
/// plus inputd's batches off an aliased endpoint — a 320x240 desktop that never
/// routed input (`windowd …::gpud::ensure_gpud_client` has the full account).
#[must_use]
pub fn is_client_envelope(frame: &[u8]) -> bool {
    frame.len() >= HEADER_LEN
        && frame[0] == ENVELOPE_MAGIC0
        && frame[1] == ENVELOPE_MAGIC1
        && frame[2] == ENVELOPE_VERSION
}
