// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: ADR-0042 client-surface transport frames — the wire contract an
//! app process speaks to windowd: `SURFACE_CREATE` (moves the app's surface
//! VMO capability with the message), `SURFACE_PRESENT` (seq + bounded damage
//! rects), `SURFACE_DESTROY`. Acks return over the app's dedicated reply
//! channel. Fixed-layout, length-guarded codecs — no allocation.
//! OWNERS: @ui @runtime
//! STATUS: Experimental (TASK-0080D R1)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests below + windowd `client_surface` host tests
//!
//! ENVELOPE: these frames arrive on windowd's SERVER endpoint, which speaks
//! the `[b'I', b'N', version, op]`-header family (input-live-protocol).
//! The op numbers here live in that shared space and MUST NOT collide with
//! the input ops (1–4) — pinned by a unit test on the windowd side.

/// Shared envelope (input-live-protocol family on windowd's server endpoint).
pub const ENVELOPE_MAGIC0: u8 = b'I';
pub const ENVELOPE_MAGIC1: u8 = b'N';
pub const ENVELOPE_VERSION: u8 = 1;
const HEADER_LEN: usize = 4;

/// Creates an app surface. Payload: `w:u16, h:u16, format:u8`. The message
/// MOVES the app's surface VMO capability (gpud-attach pattern).
pub const OP_SURFACE_CREATE: u8 = 8;
/// Presents damage. Payload: `surface_id:u32, seq:u32, count:u8,
/// (x:u16,y:u16,w:u16,h:u16) * count`.
pub const OP_SURFACE_PRESENT: u8 = 9;
/// Destroys a surface. Payload: `surface_id:u32`.
pub const OP_SURFACE_DESTROY: u8 = 10;

/// Pixel format tags. v1: BGRA8888 only.
pub const FORMAT_BGRA8888: u8 = 0;

/// Bounded damage list per present (ADR-0042).
pub const MAX_DAMAGE_RECTS: usize = 4;

/// Ack status codes (reply frames).
pub const SURFACE_STATUS_OK: u8 = 0;
pub const SURFACE_STATUS_MALFORMED: u8 = 1;
pub const SURFACE_STATUS_DENIED: u8 = 2;
pub const SURFACE_STATUS_QUOTA: u8 = 3;
pub const SURFACE_STATUS_BAD_SURFACE: u8 = 4;
pub const SURFACE_STATUS_BAD_SEQ: u8 = 5;

/// One damage rect in surface-local pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DamageRect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

fn header(op: u8) -> [u8; HEADER_LEN] {
    [ENVELOPE_MAGIC0, ENVELOPE_MAGIC1, ENVELOPE_VERSION, op]
}

fn has_op(frame: &[u8], op: u8) -> bool {
    frame.len() >= HEADER_LEN
        && frame[0] == ENVELOPE_MAGIC0
        && frame[1] == ENVELOPE_MAGIC1
        && frame[2] == ENVELOPE_VERSION
        && frame[3] == op
}

// ------------------------------------------------------------------ create

pub const SURFACE_CREATE_FRAME_LEN: usize = HEADER_LEN + 5;

#[must_use]
pub fn encode_surface_create(width: u16, height: u16, format: u8) -> [u8; SURFACE_CREATE_FRAME_LEN] {
    let mut f = [0u8; SURFACE_CREATE_FRAME_LEN];
    f[..HEADER_LEN].copy_from_slice(&header(OP_SURFACE_CREATE));
    f[4..6].copy_from_slice(&width.to_le_bytes());
    f[6..8].copy_from_slice(&height.to_le_bytes());
    f[8] = format;
    f
}

/// `(width, height, format)`.
#[must_use]
pub fn decode_surface_create(frame: &[u8]) -> Option<(u16, u16, u8)> {
    if !has_op(frame, OP_SURFACE_CREATE) || frame.len() != SURFACE_CREATE_FRAME_LEN {
        return None;
    }
    Some((
        u16::from_le_bytes([frame[4], frame[5]]),
        u16::from_le_bytes([frame[6], frame[7]]),
        frame[8],
    ))
}

// ----------------------------------------------------------------- present

pub const SURFACE_PRESENT_MAX_LEN: usize = HEADER_LEN + 9 + MAX_DAMAGE_RECTS * 8;

/// Encodes a present frame; `damage` is clamped to [`MAX_DAMAGE_RECTS`].
/// Returns the frame length.
#[must_use]
pub fn encode_surface_present(
    surface_id: u32,
    seq: u32,
    damage: &[DamageRect],
    out: &mut [u8; SURFACE_PRESENT_MAX_LEN],
) -> usize {
    let count = damage.len().min(MAX_DAMAGE_RECTS);
    out[..HEADER_LEN].copy_from_slice(&header(OP_SURFACE_PRESENT));
    out[4..8].copy_from_slice(&surface_id.to_le_bytes());
    out[8..12].copy_from_slice(&seq.to_le_bytes());
    out[12] = count as u8;
    let mut pos = 13;
    for rect in &damage[..count] {
        out[pos..pos + 2].copy_from_slice(&rect.x.to_le_bytes());
        out[pos + 2..pos + 4].copy_from_slice(&rect.y.to_le_bytes());
        out[pos + 4..pos + 6].copy_from_slice(&rect.width.to_le_bytes());
        out[pos + 6..pos + 8].copy_from_slice(&rect.height.to_le_bytes());
        pos += 8;
    }
    pos
}

/// `(surface_id, seq, damage)` — count and length strictly validated.
#[must_use]
pub fn decode_surface_present(
    frame: &[u8],
) -> Option<(u32, u32, [DamageRect; MAX_DAMAGE_RECTS], usize)> {
    if !has_op(frame, OP_SURFACE_PRESENT) || frame.len() < HEADER_LEN + 9 {
        return None;
    }
    let surface_id = u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
    let seq = u32::from_le_bytes([frame[8], frame[9], frame[10], frame[11]]);
    let count = frame[12] as usize;
    if count > MAX_DAMAGE_RECTS || frame.len() != HEADER_LEN + 9 + count * 8 {
        return None;
    }
    let mut rects = [DamageRect { x: 0, y: 0, width: 0, height: 0 }; MAX_DAMAGE_RECTS];
    for (i, rect) in rects.iter_mut().enumerate().take(count) {
        let pos = 13 + i * 8;
        *rect = DamageRect {
            x: u16::from_le_bytes([frame[pos], frame[pos + 1]]),
            y: u16::from_le_bytes([frame[pos + 2], frame[pos + 3]]),
            width: u16::from_le_bytes([frame[pos + 4], frame[pos + 5]]),
            height: u16::from_le_bytes([frame[pos + 6], frame[pos + 7]]),
        };
    }
    Some((surface_id, seq, rects, count))
}

// ----------------------------------------------------------------- destroy

pub const SURFACE_DESTROY_FRAME_LEN: usize = HEADER_LEN + 4;

#[must_use]
pub fn encode_surface_destroy(surface_id: u32) -> [u8; SURFACE_DESTROY_FRAME_LEN] {
    let mut f = [0u8; SURFACE_DESTROY_FRAME_LEN];
    f[..HEADER_LEN].copy_from_slice(&header(OP_SURFACE_DESTROY));
    f[4..8].copy_from_slice(&surface_id.to_le_bytes());
    f
}

#[must_use]
pub fn decode_surface_destroy(frame: &[u8]) -> Option<u32> {
    if !has_op(frame, OP_SURFACE_DESTROY) || frame.len() != SURFACE_DESTROY_FRAME_LEN {
        return None;
    }
    Some(u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]))
}

// -------------------------------------------------------------------- acks

/// Ack layout (all ops): `[hdr(op | 0x80), status, value:u32]` — `value` is
/// the surface id (create) or the acked seq (present/destroy: surface id).
pub const SURFACE_ACK_FRAME_LEN: usize = HEADER_LEN + 5;

#[must_use]
pub fn encode_surface_ack(op: u8, status: u8, value: u32) -> [u8; SURFACE_ACK_FRAME_LEN] {
    let mut f = [0u8; SURFACE_ACK_FRAME_LEN];
    f[..HEADER_LEN].copy_from_slice(&header(op | 0x80));
    f[4] = status;
    f[5..9].copy_from_slice(&value.to_le_bytes());
    f
}

/// `(status, value)` for the given op's ack.
#[must_use]
pub fn decode_surface_ack(frame: &[u8], op: u8) -> Option<(u8, u32)> {
    if !has_op(frame, op | 0x80) || frame.len() != SURFACE_ACK_FRAME_LEN {
        return None;
    }
    Some((frame[4], u32::from_le_bytes([frame[5], frame[6], frame[7], frame[8]])))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_round_trip_and_guards() {
        let f = encode_surface_create(320, 240, FORMAT_BGRA8888);
        assert_eq!(decode_surface_create(&f), Some((320, 240, FORMAT_BGRA8888)));
        assert_eq!(decode_surface_create(&f[..f.len() - 1]), None);
        let mut wrong = f;
        wrong[3] = OP_SURFACE_PRESENT;
        assert_eq!(decode_surface_create(&wrong), None);
    }

    #[test]
    fn present_round_trip_clamps_and_validates() {
        let damage = [
            DamageRect { x: 0, y: 0, width: 320, height: 240 },
            DamageRect { x: 10, y: 20, width: 30, height: 40 },
        ];
        let mut buf = [0u8; SURFACE_PRESENT_MAX_LEN];
        let len = encode_surface_present(7, 3, &damage, &mut buf);
        let (id, seq, rects, count) = decode_surface_present(&buf[..len]).expect("decodes");
        assert_eq!((id, seq, count), (7, 3, 2));
        assert_eq!(rects[1], damage[1]);
        // Truncated + over-count frames are rejected.
        assert_eq!(decode_surface_present(&buf[..len - 1]), None);
        let mut bad = buf;
        bad[12] = (MAX_DAMAGE_RECTS + 1) as u8;
        assert_eq!(decode_surface_present(&bad[..len]), None);
    }

    #[test]
    fn destroy_and_ack_round_trip() {
        let f = encode_surface_destroy(9);
        assert_eq!(decode_surface_destroy(&f), Some(9));
        let ack = encode_surface_ack(OP_SURFACE_PRESENT, SURFACE_STATUS_OK, 3);
        assert_eq!(decode_surface_ack(&ack, OP_SURFACE_PRESENT), Some((SURFACE_STATUS_OK, 3)));
        assert_eq!(decode_surface_ack(&ack, OP_SURFACE_CREATE), None);
    }

    #[test]
    fn ops_do_not_collide_with_the_input_family() {
        // input-live-protocol occupies 1–4 on the same endpoint envelope.
        for op in [OP_SURFACE_CREATE, OP_SURFACE_PRESENT, OP_SURFACE_DESTROY] {
            assert!(op > 4, "op {op} collides with the input family");
        }
    }
}
