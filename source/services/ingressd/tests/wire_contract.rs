// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: ingressd wire v1 contract (RFC-0092 §3) — frame shapes,
//! bounds and the deny vocabulary; every malformed shape answers
//! `STATUS_MALFORMED` and nothing panics on untrusted bytes.
//! OWNERS: @runtime @security
//! STATUS: Experimental

#![forbid(unsafe_code)]

use ingressd::table::Proto;
use ingressd::wire::{
    decode_reply, decode_request, encode_expose_reply, encode_request, encode_status_reply,
    Counters, DecodeError, Reason, EXPOSE_REPLY_LEN, OP_EXPOSE, OP_EXPOSE_STATUS, OP_UNEXPOSE,
    REQUEST_LEN, STATUS_ALLOW, STATUS_DENY, STATUS_REPLY_LEN,
};

#[test]
fn request_roundtrip_is_exact() {
    let mut buf = [0u8; 32];
    let n = encode_request(OP_EXPOSE, 0xA1B2_C3D4, 8080, Proto::Udp, &mut buf).unwrap();
    assert_eq!(n, REQUEST_LEN);
    assert_eq!(&buf[..4], &[b'I', b'G', 1, OP_EXPOSE]);
    let req = decode_request(&buf[..n]).unwrap();
    assert_eq!(
        (req.op, req.nonce, req.port, req.proto),
        (OP_EXPOSE, 0xA1B2_C3D4, 8080, Proto::Udp)
    );
    // Too small an output buffer encodes nothing (never a truncated frame).
    assert_eq!(encode_request(OP_EXPOSE, 1, 1, Proto::Tcp, &mut buf[..REQUEST_LEN - 1]), None);
}

#[test]
fn test_reject_malformed_frames() {
    let mut good = [0u8; REQUEST_LEN];
    encode_request(OP_UNEXPOSE, 7, 443, Proto::Tcp, &mut good).unwrap();
    // Short, long, bad magic, bad version, bad proto, port 0.
    assert_eq!(decode_request(&good[..REQUEST_LEN - 1]), Err(DecodeError::Malformed));
    let mut long = [0u8; REQUEST_LEN + 1];
    long[..REQUEST_LEN].copy_from_slice(&good);
    assert_eq!(decode_request(&long), Err(DecodeError::Malformed));
    let mut bad = good;
    bad[0] = b'N';
    assert_eq!(decode_request(&bad), Err(DecodeError::Malformed));
    let mut bad = good;
    bad[2] = 2;
    assert_eq!(decode_request(&bad), Err(DecodeError::Malformed));
    let mut bad = good;
    bad[10] = 9;
    assert_eq!(decode_request(&bad), Err(DecodeError::Malformed));
    let mut bad = good;
    bad[8] = 0;
    bad[9] = 0;
    assert_eq!(decode_request(&bad), Err(DecodeError::Malformed));
    assert_eq!(decode_request(&[]), Err(DecodeError::Malformed));
    // A well-formed header with an unknown op keeps its nonce for the reply.
    let mut unk = good;
    unk[3] = 42;
    assert_eq!(decode_request(&unk), Err(DecodeError::Unsupported { op: 42, nonce: 7 }));
}

#[test]
fn replies_carry_status_reason_and_counters() {
    let mut buf = [0u8; 32];
    let n = encode_expose_reply(OP_EXPOSE, 9, STATUS_DENY, Reason::Identity, &mut buf).unwrap();
    assert_eq!(n, EXPOSE_REPLY_LEN);
    let r = decode_reply(&buf[..n]).unwrap();
    assert_eq!((r.op, r.nonce, r.status, r.reason), (OP_EXPOSE, 9, STATUS_DENY, Reason::Identity));

    let counters = Counters { accepted: 3, denied_cidr: 1, denied_rate: 2 };
    let n = encode_status_reply(11, STATUS_ALLOW, true, counters, &mut buf).unwrap();
    assert_eq!(n, STATUS_REPLY_LEN);
    let r = decode_reply(&buf[..n]).unwrap();
    assert_eq!((r.op, r.nonce, r.status, r.open), (OP_EXPOSE_STATUS, 11, STATUS_ALLOW, true));
    assert_eq!(r.counters, counters);
    // Shape mismatch (status op with the short length) is not a reply.
    assert_eq!(decode_reply(&buf[..EXPOSE_REPLY_LEN]), None);
}

#[test]
fn reason_labels_are_the_marker_vocabulary() {
    let labels: Vec<&str> = (0u8..=6).map(|b| Reason::from_wire(b).unwrap().label()).collect();
    assert_eq!(labels, ["none", "policy", "identity", "limit", "tls", "cidr", "rate"]);
    assert_eq!(Reason::from_wire(7), None);
}
