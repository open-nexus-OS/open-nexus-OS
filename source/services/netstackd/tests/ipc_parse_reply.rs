// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Unit tests for netstackd IPC parse/reply helper seams
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 23 unit tests
//!
//! TEST_SCOPE:
//! - Bounded parse helpers (`u16`, `ipv4`, nonce)
//! - Reply frame helper golden bytes
//!
//! ADR: docs/adr/0005-dsoftbus-architecture.md

#[path = "../src/os/ipc/parse.rs"]
mod parse;
#[path = "../src/os/ipc/reply.rs"]
mod reply;
#[path = "../src/os/ipc/wire.rs"]
mod wire;

fn capture_frame<F>(mut emit: F) -> Vec<u8>
where
    F: FnMut(&mut dyn FnMut(&[u8])),
{
    let mut frame = Vec::new();
    let mut sink = |data: &[u8]| frame.extend_from_slice(data);
    emit(&mut sink);
    frame
}

#[test]
fn test_parse_u16_le_ok() {
    let req = [0x10, 0x34, 0x12, 0x99];
    assert_eq!(parse::parse_u16_le(&req, 1), Some(0x1234));
}

#[test]
fn test_parse_valid_wire_header() {
    let req = [wire::MAGIC0, wire::MAGIC1, wire::VERSION, wire::OP_READ];
    assert!(parse::has_valid_wire_header(&req));
}

#[test]
fn test_reject_parse_invalid_wire_header() {
    let bad_magic = [0u8, wire::MAGIC1, wire::VERSION, wire::OP_READ];
    assert!(!parse::has_valid_wire_header(&bad_magic));
    let truncated = [wire::MAGIC0, wire::MAGIC1, wire::VERSION];
    assert!(!parse::has_valid_wire_header(&truncated));
}

#[test]
fn test_parse_u32_le_ok() {
    let req = [0xaa, 0x78, 0x56, 0x34, 0x12, 0xff];
    assert_eq!(parse::parse_u32_le(&req, 1), Some(0x1234_5678));
}

#[test]
fn test_reject_parse_u16_le_truncated() {
    let req = [0x10];
    assert_eq!(parse::parse_u16_le(&req, 0), None);
}

#[test]
fn test_reject_parse_u32_le_truncated() {
    let req = [0x78, 0x56, 0x34];
    assert_eq!(parse::parse_u32_le(&req, 0), None);
}

#[test]
fn test_reject_parse_u32_le_out_of_bounds_start() {
    let req = [0x78, 0x56, 0x34, 0x12];
    assert_eq!(parse::parse_u32_le(&req, 1), None);
}

#[test]
fn test_parse_ipv4_at_ok() {
    let req = [0xaa, 10, 0, 2, 15, 0xff];
    assert_eq!(parse::parse_ipv4_at(&req, 1), Some([10, 0, 2, 15]));
}

#[test]
fn test_reject_parse_ipv4_at_truncated() {
    let req = [10, 0, 2];
    assert_eq!(parse::parse_ipv4_at(&req, 0), None);
}

#[test]
fn test_parse_nonce_exact_suffix() {
    let nonce = 0x0807_0605_0403_0201u64;
    let mut req = vec![0u8; 10];
    req.extend_from_slice(&nonce.to_le_bytes());
    assert_eq!(parse::parse_nonce(&req, 10), Some(nonce));
}

#[test]
fn test_reject_parse_nonce_wrong_len() {
    let req = [0u8; 11];
    assert_eq!(parse::parse_nonce(&req, 10), None);
}

#[test]
fn test_reply_status_maybe_nonce_without_nonce() {
    let frame = capture_frame(|sink| {
        reply::reply_status_maybe_nonce(sink, wire::OP_READ, wire::STATUS_MALFORMED, None);
    });
    assert_eq!(
        frame,
        vec![
            wire::MAGIC0,
            wire::MAGIC1,
            wire::VERSION,
            wire::OP_READ | 0x80,
            wire::STATUS_MALFORMED
        ]
    );
}

#[test]
fn test_reply_status_maybe_nonce_with_nonce() {
    let nonce = 0x1122_3344_5566_7788u64;
    let frame = capture_frame(|sink| {
        reply::reply_status_maybe_nonce(
            sink,
            wire::OP_WAIT_WRITABLE,
            wire::STATUS_NOT_FOUND,
            Some(nonce),
        );
    });
    let mut expected = vec![
        wire::MAGIC0,
        wire::MAGIC1,
        wire::VERSION,
        wire::OP_WAIT_WRITABLE | 0x80,
        wire::STATUS_NOT_FOUND,
    ];
    expected.extend_from_slice(&nonce.to_le_bytes());
    assert_eq!(frame, expected);
}

#[test]
fn test_reply_u32_status_maybe_nonce_variants() {
    let no_nonce = capture_frame(|sink| {
        reply::reply_u32_status_maybe_nonce(sink, wire::OP_LISTEN, wire::STATUS_OK, 7, None);
    });
    assert_eq!(
        no_nonce,
        vec![
            wire::MAGIC0,
            wire::MAGIC1,
            wire::VERSION,
            wire::OP_LISTEN | 0x80,
            wire::STATUS_OK,
            7,
            0,
            0,
            0
        ]
    );

    let nonce = 0x1111_2222_3333_4444u64;
    let with_nonce = capture_frame(|sink| {
        reply::reply_u32_status_maybe_nonce(
            sink,
            wire::OP_ACCEPT,
            wire::STATUS_NOT_FOUND,
            9,
            Some(nonce),
        );
    });
    let mut expected = vec![
        wire::MAGIC0,
        wire::MAGIC1,
        wire::VERSION,
        wire::OP_ACCEPT | 0x80,
        wire::STATUS_NOT_FOUND,
        9,
        0,
        0,
        0,
    ];
    expected.extend_from_slice(&nonce.to_le_bytes());
    assert_eq!(with_nonce, expected);
}

#[test]
fn test_reply_u16_field_status_maybe_nonce_variants() {
    let no_nonce = capture_frame(|sink| {
        reply::reply_u16_field_status_maybe_nonce(
            sink,
            wire::OP_ICMP_PING,
            wire::STATUS_OK,
            42,
            None,
        );
    });
    assert_eq!(
        no_nonce,
        vec![
            wire::MAGIC0,
            wire::MAGIC1,
            wire::VERSION,
            wire::OP_ICMP_PING | 0x80,
            wire::STATUS_OK,
            42,
            0
        ]
    );

    let nonce = 0xaabb_ccdd_eeff_0011u64;
    let with_nonce = capture_frame(|sink| {
        reply::reply_u16_field_status_maybe_nonce(
            sink,
            wire::OP_WRITE,
            wire::STATUS_WOULD_BLOCK,
            0,
            Some(nonce),
        );
    });
    let mut expected = vec![
        wire::MAGIC0,
        wire::MAGIC1,
        wire::VERSION,
        wire::OP_WRITE | 0x80,
        wire::STATUS_WOULD_BLOCK,
        0,
        0,
    ];
    expected.extend_from_slice(&nonce.to_le_bytes());
    assert_eq!(with_nonce, expected);
}

#[test]
fn test_status_frame_shape() {
    let frame = reply::status_frame(wire::OP_CONNECT, wire::STATUS_MALFORMED);
    assert_eq!(
        frame,
        [
            wire::MAGIC0,
            wire::MAGIC1,
            wire::VERSION,
            wire::OP_CONNECT | 0x80,
            wire::STATUS_MALFORMED,
        ]
    );
}

#[test]
fn test_fill_header_prefix_populates_first_five_bytes() {
    let mut out = [0xffu8; 8];
    reply::fill_header_prefix(&mut out, wire::OP_LOCAL_ADDR, wire::STATUS_OK);
    assert_eq!(
        &out[..5],
        &[
            wire::MAGIC0,
            wire::MAGIC1,
            wire::VERSION,
            wire::OP_LOCAL_ADDR | 0x80,
            wire::STATUS_OK,
        ]
    );
    assert_eq!(&out[5..], &[0xff, 0xff, 0xff]);
}

#[test]
fn test_append_nonce_writes_le_bytes() {
    let nonce = 0x0102_0304_0506_0708u64;
    let mut out = [0u8; 8];
    reply::append_nonce(&mut out, nonce);
    assert_eq!(out, nonce.to_le_bytes());
}

#[test]
fn test_reply_u16_len_payload_status_maybe_nonce_variants() {
    let no_nonce = capture_frame(|sink| {
        reply::reply_u16_len_payload_status_maybe_nonce(
            sink,
            wire::OP_READ,
            wire::STATUS_OK,
            &[0xaa, 0xbb, 0xcc],
            None,
        );
    });
    assert_eq!(
        no_nonce,
        vec![
            wire::MAGIC0,
            wire::MAGIC1,
            wire::VERSION,
            wire::OP_READ | 0x80,
            wire::STATUS_OK,
            3,
            0,
            0xaa,
            0xbb,
            0xcc,
        ]
    );

    let nonce = 0x8877_6655_4433_2211u64;
    let with_nonce = capture_frame(|sink| {
        reply::reply_u16_len_payload_status_maybe_nonce(
            sink,
            wire::OP_READ,
            wire::STATUS_OK,
            &[0x10, 0x20],
            Some(nonce),
        );
    });
    let mut expected = vec![
        wire::MAGIC0,
        wire::MAGIC1,
        wire::VERSION,
        wire::OP_READ | 0x80,
        wire::STATUS_OK,
        2,
        0,
        0x10,
        0x20,
    ];
    expected.extend_from_slice(&nonce.to_le_bytes());
    assert_eq!(with_nonce, expected);
}

#[test]
fn test_reply_u16_len_payload_status_max_payload_boundary() {
    let payload = vec![0xa5u8; 480];
    let frame = capture_frame(|sink| {
        reply::reply_u16_len_payload_status_maybe_nonce(
            sink,
            wire::OP_READ,
            wire::STATUS_OK,
            &payload,
            None,
        );
    });
    assert_eq!(frame.len(), 7 + 480);
    assert_eq!(frame[0], wire::MAGIC0);
    assert_eq!(frame[1], wire::MAGIC1);
    assert_eq!(frame[2], wire::VERSION);
    assert_eq!(frame[3], wire::OP_READ | 0x80);
    assert_eq!(frame[4], wire::STATUS_OK);
    assert_eq!(u16::from_le_bytes([frame[5], frame[6]]), 480);
    assert!(frame[7..].iter().all(|b| *b == 0xa5));
}

#[test]
fn test_reply_u16_len_ipv4_port_payload_status_maybe_nonce_variants() {
    let no_nonce = capture_frame(|sink| {
        reply::reply_u16_len_ipv4_port_payload_status_maybe_nonce(
            sink,
            wire::OP_UDP_RECV_FROM,
            wire::STATUS_OK,
            [10, 0, 2, 15],
            9999,
            &[0xde, 0xad],
            None,
        );
    });
    assert_eq!(
        no_nonce,
        vec![
            wire::MAGIC0,
            wire::MAGIC1,
            wire::VERSION,
            wire::OP_UDP_RECV_FROM | 0x80,
            wire::STATUS_OK,
            2,
            0,
            10,
            0,
            2,
            15,
            0x0f,
            0x27,
            0xde,
            0xad,
        ]
    );

    let nonce = 0x0102_0304_0506_0708u64;
    let with_nonce = capture_frame(|sink| {
        reply::reply_u16_len_ipv4_port_payload_status_maybe_nonce(
            sink,
            wire::OP_UDP_RECV_FROM,
            wire::STATUS_OK,
            [1, 2, 3, 4],
            53,
            &[0xbe],
            Some(nonce),
        );
    });
    let mut expected = vec![
        wire::MAGIC0,
        wire::MAGIC1,
        wire::VERSION,
        wire::OP_UDP_RECV_FROM | 0x80,
        wire::STATUS_OK,
        1,
        0,
        1,
        2,
        3,
        4,
        53,
        0,
        0xbe,
    ];
    expected.extend_from_slice(&nonce.to_le_bytes());
    assert_eq!(with_nonce, expected);
}

#[test]
fn test_reply_u16_len_ipv4_port_payload_max_payload_boundary_with_nonce() {
    let payload = vec![0x5au8; 460];
    let nonce = 0x8899_aabb_ccdd_eeffu64;
    let frame = capture_frame(|sink| {
        reply::reply_u16_len_ipv4_port_payload_status_maybe_nonce(
            sink,
            wire::OP_UDP_RECV_FROM,
            wire::STATUS_OK,
            [10, 42, 0, 15],
            5353,
            &payload,
            Some(nonce),
        );
    });
    assert_eq!(frame.len(), 13 + 460 + 8);
    assert_eq!(frame[0], wire::MAGIC0);
    assert_eq!(frame[1], wire::MAGIC1);
    assert_eq!(frame[2], wire::VERSION);
    assert_eq!(frame[3], wire::OP_UDP_RECV_FROM | 0x80);
    assert_eq!(frame[4], wire::STATUS_OK);
    assert_eq!(u16::from_le_bytes([frame[5], frame[6]]), 460);
    assert_eq!(&frame[7..11], &[10, 42, 0, 15]);
    assert_eq!(u16::from_le_bytes([frame[11], frame[12]]), 5353);
    assert!(frame[13..13 + 460].iter().all(|b| *b == 0x5a));
    assert_eq!(&frame[13 + 460..13 + 460 + 8], &nonce.to_le_bytes());
}

#[test]
fn test_wire_constants_contract_coverage() {
    let op_codes = [
        wire::OP_LISTEN,
        wire::OP_ACCEPT,
        wire::OP_CONNECT,
        wire::OP_READ,
        wire::OP_WRITE,
        wire::OP_UDP_BIND,
        wire::OP_UDP_SEND_TO,
        wire::OP_UDP_RECV_FROM,
        wire::OP_ICMP_PING,
        wire::OP_LOCAL_ADDR,
        wire::OP_CLOSE,
        wire::OP_WAIT_WRITABLE,
    ];
    for op in op_codes {
        assert!(op > 0, "op code must stay non-zero");
    }
    let statuses = [
        wire::STATUS_OK,
        wire::STATUS_NOT_FOUND,
        wire::STATUS_MALFORMED,
        wire::STATUS_WOULD_BLOCK,
        wire::STATUS_IO,
        wire::STATUS_TIMED_OUT,
    ];
    for status in statuses {
        assert!(status <= wire::STATUS_TIMED_OUT);
    }
}
