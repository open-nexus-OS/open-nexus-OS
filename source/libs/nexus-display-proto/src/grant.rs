// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The framebuffer grant (RFC-0093 §5 as amended 2026-09-30, RFC-0098 C7, TASK-0251 P1):
//! the display mode and the scanout memory are gpud's. gpud reads the lane's mode REQUEST from
//! its own read-only tree slot (`/chosen/nexus,display-mode`, written by nxboot from the
//! launcher's knob; absent on the board — [`parse_display_request`]), resolves it against the
//! device's capability with [`crate::resolve_display_mode`], makes the shared framebuffer
//! resource FOR its device (the memory a scanout engine reads must lie in that engine's DMA
//! reach — a display controller without an MMU needs it in one block), and hands both to
//! windowd in ONE answer: windowd asks with [`OP_FRAMEBUFFER_REQUEST`] and receives a
//! [`FramebufferGrant`] with a clone of the framebuffer object moved along. windowd builds its
//! compositor at the granted mode; inputd asks windowd for the same space. Nobody else reads the
//! request and nobody falls back to a default in silence: a grant that is not an honest grant
//! does not decode.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal v1 (the frame is a windowd↔gpud contract)
//! TEST_COVERAGE: the tests below (`test_reject_*` for every refusal)

use crate::layout::{LAYOUT_MAX, RESOURCE_BYTES};

/// windowd → gpud: "grant me the framebuffer and the mode" (`[op]`, 1 byte). gpud answers on its
/// response endpoint with a [`FramebufferGrant`] frame and, when the status is OK, a clone of
/// the framebuffer object moved with it. Opcode 11 stays retired (the polled mode query).
pub const OP_FRAMEBUFFER_REQUEST: u8 = 14;

/// Grant frame: `[status, mode_w: u16 le, mode_h: u16 le, len: u32 le]`.
pub const FRAMEBUFFER_GRANT_LEN: usize = 9;

/// What gpud grants: the visible mode it resolved and the byte length of the framebuffer
/// object it moves along (the whole shared resource of [`crate::layout`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FramebufferGrant {
    pub status: u8,
    pub mode_w: u16,
    pub mode_h: u16,
    pub len: u32,
}

/// The request frame.
#[must_use]
pub const fn encode_framebuffer_request() -> [u8; 1] {
    [OP_FRAMEBUFFER_REQUEST]
}

/// Encode a [`FramebufferGrant`].
#[must_use]
pub fn encode_framebuffer_grant(grant: &FramebufferGrant) -> [u8; FRAMEBUFFER_GRANT_LEN] {
    let mut f = [0u8; FRAMEBUFFER_GRANT_LEN];
    f[0] = grant.status;
    f[1..3].copy_from_slice(&grant.mode_w.to_le_bytes());
    f[3..5].copy_from_slice(&grant.mode_h.to_le_bytes());
    f[5..9].copy_from_slice(&grant.len.to_le_bytes());
    f
}

/// Decode a grant. `None` when the frame is short, and — for an OK status — when the grant is
/// not one windowd can build on: a zero or oversized mode (above [`LAYOUT_MAX`]: the planes are
/// laid out for the maximum), or an object smaller than the layout's resource. A refusal
/// (status not OK) decodes as it is, so the caller can name it.
#[must_use]
pub fn decode_framebuffer_grant(frame: &[u8]) -> Option<FramebufferGrant> {
    if frame.len() < FRAMEBUFFER_GRANT_LEN {
        return None;
    }
    let grant = FramebufferGrant {
        status: frame[0],
        mode_w: u16::from_le_bytes([frame[1], frame[2]]),
        mode_h: u16::from_le_bytes([frame[3], frame[4]]),
        len: u32::from_le_bytes([frame[5], frame[6], frame[7], frame[8]]),
    };
    if grant.status == crate::STATUS_OK {
        let (w, h) = (u32::from(grant.mode_w), u32::from(grant.mode_h));
        if w == 0 || h == 0 || w > LAYOUT_MAX.0 || h > LAYOUT_MAX.1 {
            return None;
        }
        if (grant.len as usize) < RESOURCE_BYTES {
            return None;
        }
    }
    Some(grant)
}

/// Parse the lane's display-mode request — the `"<w>x<h>"` text of `/chosen/nexus,display-mode`
/// — into `(w, h)`. Pure and bounded (it moved here from the kernel with syscall 50's
/// deletion): the value ends at the first NUL, newline or space; each dimension is 1..=8192
/// written in at most five decimal digits. Anything else is not a request (`None`), never a
/// degenerate size.
#[must_use]
pub fn parse_display_request(buf: &[u8]) -> Option<(u32, u32)> {
    const MAX_DIM: u32 = 8192;
    let end = buf
        .iter()
        .position(|&c| c == 0 || c == b'\n' || c == b'\r' || c == b' ')
        .unwrap_or(buf.len());
    let text = &buf[..end];
    let sep = text.iter().position(|&c| c == b'x' || c == b'X')?;
    let w = parse_dim(&text[..sep], MAX_DIM)?;
    let h = parse_dim(&text[sep + 1..], MAX_DIM)?;
    Some((w, h))
}

fn parse_dim(bytes: &[u8], max: u32) -> Option<u32> {
    if bytes.is_empty() || bytes.len() > 5 {
        return None;
    }
    let mut v: u32 = 0;
    for &c in bytes {
        if !c.is_ascii_digit() {
            return None;
        }
        v = v.checked_mul(10)?.checked_add(u32::from(c - b'0'))?;
    }
    if v == 0 || v > max {
        return None;
    }
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok_grant(w: u16, h: u16) -> FramebufferGrant {
        FramebufferGrant {
            status: crate::STATUS_OK,
            mode_w: w,
            mode_h: h,
            len: RESOURCE_BYTES as u32,
        }
    }

    #[test]
    fn grant_roundtrip_golden_bytes() {
        let g = ok_grant(1280, 800);
        let f = encode_framebuffer_grant(&g);
        assert_eq!(&f[..5], &[crate::STATUS_OK, 0x00, 0x05, 0x20, 0x03]);
        assert_eq!(&f[5..9], &(RESOURCE_BYTES as u32).to_le_bytes());
        assert_eq!(decode_framebuffer_grant(&f), Some(g));
        assert_eq!(encode_framebuffer_request(), [OP_FRAMEBUFFER_REQUEST]);
    }

    #[test]
    fn the_layout_maximum_is_a_grantable_mode() {
        let g = ok_grant(LAYOUT_MAX.0 as u16, LAYOUT_MAX.1 as u16);
        assert_eq!(decode_framebuffer_grant(&encode_framebuffer_grant(&g)), Some(g));
    }

    /// A grant without a mode is not a grant; neither is a mode the planes are not laid out for.
    #[test]
    fn test_reject_grant_without_a_buildable_mode() {
        for (w, h) in
            [(0, 800), (1280, 0), (LAYOUT_MAX.0 as u16 + 1, 800), (1280, LAYOUT_MAX.1 as u16 + 1)]
        {
            assert_eq!(decode_framebuffer_grant(&encode_framebuffer_grant(&ok_grant(w, h))), None);
        }
    }

    /// An object smaller than the layout's resource would let windowd write past its end.
    #[test]
    fn test_reject_grant_smaller_than_the_layout() {
        let mut g = ok_grant(1280, 800);
        g.len = RESOURCE_BYTES as u32 - 1;
        assert_eq!(decode_framebuffer_grant(&encode_framebuffer_grant(&g)), None);
    }

    #[test]
    fn test_reject_short_grant() {
        let f = encode_framebuffer_grant(&ok_grant(1280, 800));
        assert_eq!(decode_framebuffer_grant(&f[..FRAMEBUFFER_GRANT_LEN - 1]), None);
        assert_eq!(decode_framebuffer_grant(&[]), None);
    }

    /// A refusal decodes as it is — the caller names it (no silent default).
    #[test]
    fn a_refusal_decodes_with_its_status() {
        let g =
            FramebufferGrant { status: crate::STATUS_DEVICE_ERROR, mode_w: 0, mode_h: 0, len: 0 };
        assert_eq!(decode_framebuffer_grant(&encode_framebuffer_grant(&g)), Some(g));
    }

    #[test]
    fn requests_parse() {
        assert_eq!(parse_display_request(b"1280x800"), Some((1280, 800)));
        assert_eq!(parse_display_request(b"1920X1080\0"), Some((1920, 1080)));
        assert_eq!(parse_display_request(b"600x800\n"), Some((600, 800)));
    }

    #[test]
    fn test_reject_malformed_requests() {
        for bad in [
            &b"0x800"[..],
            b"9000x800",
            b"1280",
            b"12a0x800",
            b"",
            b"x",
            b"1280x",
            b"123456x800",
            b"1280x800x600",
        ] {
            assert_eq!(parse_display_request(bad), None, "{:?}", core::str::from_utf8(bad));
        }
    }
}
