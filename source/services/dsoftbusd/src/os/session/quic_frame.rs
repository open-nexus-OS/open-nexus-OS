//! QUIC-v2 UDP frame helpers (pure, host-testable).

pub(crate) const QUIC_FRAME_MAGIC0: u8 = b'Q';
pub(crate) const QUIC_FRAME_MAGIC1: u8 = b'D';
pub(crate) const QUIC_FRAME_VERSION: u8 = 1;
pub(crate) const QUIC_FRAME_HEADER_LEN: usize = 10;
pub(crate) const QUIC_OP_MSG1: u8 = 1;
pub(crate) const QUIC_OP_MSG2: u8 = 2;
pub(crate) const QUIC_OP_MSG3: u8 = 3;
pub(crate) const QUIC_OP_PING: u8 = 4;
pub(crate) const QUIC_OP_PONG: u8 = 5;

#[must_use]
pub(crate) fn encode_quic_frame(
    op: u8,
    session_nonce: u32,
    payload: &[u8],
    out: &mut [u8; 256],
) -> Option<usize> {
    if payload.len() > out.len().saturating_sub(QUIC_FRAME_HEADER_LEN) {
        return None;
    }
    out[0] = QUIC_FRAME_MAGIC0;
    out[1] = QUIC_FRAME_MAGIC1;
    out[2] = QUIC_FRAME_VERSION;
    out[3] = op;
    out[4..8].copy_from_slice(&session_nonce.to_le_bytes());
    out[8..10].copy_from_slice(&(payload.len() as u16).to_le_bytes());
    out[10..10 + payload.len()].copy_from_slice(payload);
    Some(QUIC_FRAME_HEADER_LEN + payload.len())
}

#[must_use]
pub(crate) fn decode_quic_frame(buf: &[u8], n: usize) -> Option<(u8, u32, &[u8])> {
    if n < QUIC_FRAME_HEADER_LEN {
        return None;
    }
    if buf[0] != QUIC_FRAME_MAGIC0 || buf[1] != QUIC_FRAME_MAGIC1 || buf[2] != QUIC_FRAME_VERSION {
        return None;
    }
    let op = buf[3];
    let session_nonce = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
    let payload_len = u16::from_le_bytes([buf[8], buf[9]]) as usize;
    let payload_end = QUIC_FRAME_HEADER_LEN.checked_add(payload_len)?;
    if payload_end > n || payload_end > buf.len() {
        return None;
    }
    Some((op, session_nonce, &buf[QUIC_FRAME_HEADER_LEN..payload_end]))
}
