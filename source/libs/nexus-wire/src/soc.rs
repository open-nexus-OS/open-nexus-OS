// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! SoC-glue wire protocol (RFC-0106 v1). `socd` is the ONE writer of the syscon
//! and pinctrl windows; a consumer asks for its node by path and gets a verdict.
//! Request/reply on socd's server endpoint, correlated by a caller nonce.
//!
//! `BRING_UP`: `[S, G, ver, OP, nonce:u32le, len:u8, path…]` →
//! `[S, G, ver, OP|0x80, status, nonce:u32le, domains:u8, resets:u8, clocks:u8,
//! pads:u8, fault_addr:u64le, fault_value:u32le]`.
//! `CLOCK_RATE`: `[S, G, ver, OP, nonce:u32le, len:u8, path…, len:u8, name…]` →
//! `[S, G, ver, OP|0x80, status, nonce:u32le, hz:u64le]`.

use crate::codec::{check_hdr, request_op, Reader, Writer};

/// First magic byte (`'S'`).
pub const MAGIC0: u8 = b'S';
/// Second magic byte (`'G'`).
pub const MAGIC1: u8 = b'G';
/// Protocol version.
pub const VERSION: u8 = 1;

/// Bring a consumer node up: power domain, resets released, clocks on, pads.
pub const OP_BRING_UP: u8 = 1;
/// The rate of one of a node's clocks (`clock-names`), as the registers say.
pub const OP_CLOCK_RATE: u8 = 2;

/// Every step done and read back.
pub const STATUS_OK: u8 = 0;
/// The tree binds nothing for this node (QEMU virt): nothing to do — success for the caller.
pub const STATUS_NOT_NEEDED: u8 = 1;
/// The caller holds no `soc.glue` capability (policyd, deny-by-default).
pub const STATUS_DENIED: u8 = 2;
/// No node at that path.
pub const STATUS_NO_SUCH_NODE: u8 = 3;
/// A step failed; `fault_addr`/`fault_value` name the register and what it read.
pub const STATUS_FAILED: u8 = 4;
/// Malformed request.
pub const STATUS_MALFORMED: u8 = 5;
/// The node binds something the tables or the tree do not cover (an unknown
/// provider, id or a power domain without a measured protocol).
pub const STATUS_UNSUPPORTED: u8 = 6;

/// Longest node path a request may carry.
pub const PATH_MAX: usize = 96;
/// Longest clock name.
pub const NAME_MAX: usize = 32;

/// A decoded `BRING_UP` reply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BringUpReply {
    /// One of the `STATUS_*` values.
    pub status: u8,
    /// The caller's nonce, echoed.
    pub nonce: u32,
    /// Power domains confirmed on.
    pub domains: u8,
    /// Reset lines released.
    pub resets: u8,
    /// Clock gates on.
    pub clocks: u8,
    /// Pads configured.
    pub pads: u8,
    /// On `STATUS_FAILED`: the register that did not read back (mapped address).
    pub fault_addr: u64,
    /// On `STATUS_FAILED`: what it read.
    pub fault_value: u32,
}

/// The opcode of a socd v1 request frame (`None` = not ours).
pub fn decode_request_op(frame: &[u8]) -> Option<u8> {
    request_op(frame, MAGIC0, MAGIC1, VERSION)
}

/// Encode a `BRING_UP` request into `out`; the frame's length.
pub fn encode_bring_up_req(out: &mut [u8], nonce: u32, path: &str) -> Option<usize> {
    let mut w = Writer::new(out);
    w.put_bytes(&[MAGIC0, MAGIC1, VERSION, OP_BRING_UP])?;
    w.put_u32le(nonce)?;
    w.put_len8_str(path, 1, PATH_MAX)?;
    Some(w.pos())
}

/// Decode a `BRING_UP` request → `(nonce, path)`.
pub fn decode_bring_up_req(frame: &[u8]) -> Option<(u32, &str)> {
    let mut r = Reader::new(frame);
    check_hdr(&mut r, MAGIC0, MAGIC1, VERSION, OP_BRING_UP)?;
    let nonce = r.take_u32le()?;
    let len = r.take_u8()? as usize;
    if len == 0 || len > PATH_MAX {
        return None;
    }
    let path = core::str::from_utf8(r.take_bytes(len)?).ok()?;
    Some((nonce, path))
}

/// Encode a `BRING_UP` reply.
pub fn encode_bring_up_rsp(out: &mut [u8], rsp: &BringUpReply) -> Option<usize> {
    let mut w = Writer::new(out);
    w.put_bytes(&[MAGIC0, MAGIC1, VERSION, OP_BRING_UP | 0x80, rsp.status])?;
    w.put_u32le(rsp.nonce)?;
    w.put_bytes(&[rsp.domains, rsp.resets, rsp.clocks, rsp.pads])?;
    w.put_u64le(rsp.fault_addr)?;
    w.put_u32le(rsp.fault_value)?;
    Some(w.pos())
}

/// Decode a `BRING_UP` reply.
pub fn decode_bring_up_rsp(frame: &[u8]) -> Option<BringUpReply> {
    let mut r = Reader::new(frame);
    check_hdr(&mut r, MAGIC0, MAGIC1, VERSION, OP_BRING_UP | 0x80)?;
    let status = r.take_u8()?;
    let nonce = r.take_u32le()?;
    let domains = r.take_u8()?;
    let resets = r.take_u8()?;
    let clocks = r.take_u8()?;
    let pads = r.take_u8()?;
    let fault_addr = r.take_u64le()?;
    let fault_value = r.take_u32le()?;
    Some(BringUpReply { status, nonce, domains, resets, clocks, pads, fault_addr, fault_value })
}

/// Encode a `CLOCK_RATE` request.
pub fn encode_clock_rate_req(out: &mut [u8], nonce: u32, path: &str, name: &str) -> Option<usize> {
    let mut w = Writer::new(out);
    w.put_bytes(&[MAGIC0, MAGIC1, VERSION, OP_CLOCK_RATE])?;
    w.put_u32le(nonce)?;
    w.put_len8_str(path, 1, PATH_MAX)?;
    w.put_len8_str(name, 1, NAME_MAX)?;
    Some(w.pos())
}

/// Decode a `CLOCK_RATE` request → `(nonce, path, name)`.
pub fn decode_clock_rate_req(frame: &[u8]) -> Option<(u32, &str, &str)> {
    let mut r = Reader::new(frame);
    check_hdr(&mut r, MAGIC0, MAGIC1, VERSION, OP_CLOCK_RATE)?;
    let nonce = r.take_u32le()?;
    let plen = r.take_u8()? as usize;
    if plen == 0 || plen > PATH_MAX {
        return None;
    }
    let path = core::str::from_utf8(r.take_bytes(plen)?).ok()?;
    let nlen = r.take_u8()? as usize;
    if nlen == 0 || nlen > NAME_MAX {
        return None;
    }
    let name = core::str::from_utf8(r.take_bytes(nlen)?).ok()?;
    Some((nonce, path, name))
}

/// Encode a `CLOCK_RATE` reply.
pub fn encode_clock_rate_rsp(out: &mut [u8], status: u8, nonce: u32, hz: u64) -> Option<usize> {
    let mut w = Writer::new(out);
    w.put_bytes(&[MAGIC0, MAGIC1, VERSION, OP_CLOCK_RATE | 0x80, status])?;
    w.put_u32le(nonce)?;
    w.put_u64le(hz)?;
    Some(w.pos())
}

/// Decode a `CLOCK_RATE` reply → `(status, nonce, hz)`.
pub fn decode_clock_rate_rsp(frame: &[u8]) -> Option<(u8, u32, u64)> {
    let mut r = Reader::new(frame);
    check_hdr(&mut r, MAGIC0, MAGIC1, VERSION, OP_CLOCK_RATE | 0x80)?;
    let status = r.take_u8()?;
    let nonce = r.take_u32le()?;
    let hz = r.take_u64le()?;
    Some((status, nonce, hz))
}

/// Encode the short reply every malformed or refused request gets (any op).
pub fn encode_status_rsp(out: &mut [u8], op: u8, status: u8, nonce: u32) -> Option<usize> {
    let mut w = Writer::new(out);
    w.put_bytes(&[MAGIC0, MAGIC1, VERSION, op | 0x80, status])?;
    w.put_u32le(nonce)?;
    Some(w.pos())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bring_up_round_trips_and_bounds_the_path() {
        let mut buf = [0u8; 128];
        let n = encode_bring_up_req(&mut buf, 0x1234, "/soc/mmc@d4281000").unwrap();
        assert_eq!(decode_bring_up_req(&buf[..n]), Some((0x1234, "/soc/mmc@d4281000")));
        assert!(encode_bring_up_req(&mut buf, 1, "").is_none());
        let long = core::str::from_utf8(&[b'a'; PATH_MAX + 1]).unwrap();
        assert!(encode_bring_up_req(&mut buf, 1, long).is_none());
        let rsp = BringUpReply {
            status: STATUS_FAILED,
            nonce: 7,
            domains: 1,
            resets: 2,
            clocks: 1,
            pads: 0,
            fault_addr: 0xd428_2854,
            fault_value: 0x52,
        };
        let n = encode_bring_up_rsp(&mut buf, &rsp).unwrap();
        assert_eq!(n, 25, "5 header + 4 nonce + 4 counts + 8 addr + 4 value");
        assert_eq!(decode_bring_up_rsp(&buf[..n]), Some(rsp));
        assert!(decode_bring_up_rsp(&buf[..n - 1]).is_none(), "truncated");
    }

    #[test]
    fn clock_rate_round_trips() {
        let mut buf = [0u8; 160];
        let n = encode_clock_rate_req(&mut buf, 9, "/soc/mmc@d4281000", "io").unwrap();
        assert_eq!(decode_clock_rate_req(&buf[..n]), Some((9, "/soc/mmc@d4281000", "io")));
        let n = encode_clock_rate_rsp(&mut buf, STATUS_OK, 9, 375_000_000).unwrap();
        assert_eq!(decode_clock_rate_rsp(&buf[..n]), Some((STATUS_OK, 9, 375_000_000)));
    }

    #[test]
    fn test_reject_foreign_magic_and_wrong_op() {
        let mut buf = [0u8; 64];
        let n = encode_bring_up_req(&mut buf, 1, "/x").unwrap();
        buf[0] = b'R';
        assert!(decode_bring_up_req(&buf[..n]).is_none());
        assert_eq!(decode_request_op(&buf[..n]), None);
        buf[0] = MAGIC0;
        assert_eq!(decode_request_op(&buf[..n]), Some(OP_BRING_UP));
        assert!(decode_clock_rate_req(&buf[..n]).is_none());
    }
}
