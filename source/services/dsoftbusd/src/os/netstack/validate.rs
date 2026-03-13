//! Pure reply/frame validation helpers for deterministic transport tests.

const MAGIC0: u8 = b'N';
const MAGIC1: u8 = b'S';
const VERSION: u8 = 1;

#[inline]
pub(crate) fn nonce_matches_tail(buf: &[u8], nonce: u64) -> bool {
    if buf.len() < 13 {
        return false;
    }
    let mut b = [0u8; 8];
    b.copy_from_slice(&buf[buf.len() - 8..]);
    u64::from_le_bytes(b) == nonce
}

#[inline]
pub(crate) fn response_matches(buf: &[u8], expect_rsp_op: u8, nonce: u64) -> bool {
    buf.len() >= 5
        && buf[0] == MAGIC0
        && buf[1] == MAGIC1
        && buf[2] == VERSION
        && buf[3] == expect_rsp_op
        && nonce_matches_tail(buf, nonce)
}

#[inline]
pub(crate) fn extract_netstack_reply_nonce(buf: &[u8]) -> Option<u64> {
    if buf.len() < 13 || buf[0] != MAGIC0 || buf[1] != MAGIC1 || buf[2] != VERSION {
        return None;
    }
    let mut b = [0u8; 8];
    b.copy_from_slice(&buf[buf.len() - 8..]);
    Some(u64::from_le_bytes(b))
}

#[inline]
pub(crate) fn parse_status_frame(rsp: &[u8], expected_op: u8) -> core::result::Result<u8, ()> {
    if rsp.len() < 5 {
        return Err(());
    }
    if rsp[0] != MAGIC0 || rsp[1] != MAGIC1 || rsp[2] != VERSION || rsp[3] != expected_op {
        return Err(());
    }
    Ok(rsp[4])
}

#[inline]
pub(crate) fn parse_read_ok_len(rsp: &[u8]) -> core::result::Result<usize, ()> {
    if rsp.len() < 7 {
        return Err(());
    }
    let n = u16::from_le_bytes([rsp[5], rsp[6]]) as usize;
    if n == 0 || 7 + n > rsp.len() {
        return Err(());
    }
    Ok(n)
}

#[inline]
pub(crate) fn parse_write_ok_wrote(rsp: &[u8]) -> core::result::Result<usize, ()> {
    if rsp.len() < 7 {
        return Err(());
    }
    let wrote = u16::from_le_bytes([rsp[5], rsp[6]]) as usize;
    if wrote == 0 {
        return Err(());
    }
    Ok(wrote)
}

#[inline]
pub(crate) fn is_valid_udp_payload_len(len: usize) -> bool {
    len <= 256
}
