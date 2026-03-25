// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Behavior tests for netstackd loopback buffering and marker formatting seams
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 7 unit tests
//!
//! TEST_SCOPE:
//! - Loopback FIFO/capacity/wrap semantics
//! - Loopback payload bound rejection
//! - Decimal/IP marker formatter correctness
//!
//! ADR: docs/adr/0005-dsoftbus-architecture.md

#[path = "../src/os/loopback.rs"]
mod loopback;
#[path = "../src/os/observability.rs"]
mod observability;

#[test]
fn test_loopbuf_fifo_roundtrip() {
    let mut buf = loopback::LoopBuf::new();
    assert_eq!(buf.push(b"abc"), 3);

    let mut out = [0u8; 3];
    assert_eq!(buf.pop(&mut out), 3);
    assert_eq!(&out, b"abc");
}

#[test]
fn test_loopbuf_wraparound_preserves_order() {
    let mut buf = loopback::LoopBuf::new();
    assert_eq!(buf.push(&[1, 2, 3, 4]), 4);

    let mut first = [0u8; 3];
    assert_eq!(buf.pop(&mut first), 3);
    assert_eq!(first, [1, 2, 3]);

    assert_eq!(buf.push(&[5, 6, 7]), 3);

    let mut out = [0u8; 4];
    assert_eq!(buf.pop(&mut out), 4);
    assert_eq!(out, [4, 5, 6, 7]);
}

#[test]
fn test_loopbuf_capacity_clamps_push() {
    let mut buf = loopback::LoopBuf::new();
    let input = [0x55u8; loopback::LOOPBUF_CAPACITY + 16];
    assert_eq!(buf.push(&input), loopback::LOOPBUF_CAPACITY);

    let mut out = [0u8; loopback::LOOPBUF_CAPACITY + 16];
    let n = buf.pop(&mut out);
    assert_eq!(n, loopback::LOOPBUF_CAPACITY);
    assert_eq!(&out[..n], &input[..loopback::LOOPBUF_CAPACITY]);
}

#[test]
fn test_loopbuf_pop_empty_returns_zero() {
    let mut buf = loopback::LoopBuf::new();
    let mut out = [0u8; 8];
    assert_eq!(buf.pop(&mut out), 0);
}

#[test]
fn test_reject_oversized_loopback_payload() {
    assert!(loopback::reject_oversized_loopback_payload(
        loopback::LOOPBUF_CAPACITY + 1
    ));
    assert!(!loopback::reject_oversized_loopback_payload(
        loopback::LOOPBUF_CAPACITY
    ));
}

#[test]
fn test_write_u8_decimal_encoding() {
    let cases: &[(u8, &str)] = &[
        (0, "0"),
        (9, "9"),
        (10, "10"),
        (42, "42"),
        (99, "99"),
        (100, "100"),
        (255, "255"),
    ];

    for (value, expected) in cases {
        let mut out = [0u8; 3];
        let n = observability::write_u8(*value, &mut out);
        let got = core::str::from_utf8(&out[..n]).expect("formatter must emit UTF-8 ASCII digits");
        assert_eq!(got, *expected);
    }
}

#[test]
fn test_write_ip_dotted_decimal_encoding() {
    let mut out = [0u8; 16];
    let n = observability::write_ip(&[10, 42, 0, 255], &mut out);
    let got = core::str::from_utf8(&out[..n]).expect("formatter must emit UTF-8 ASCII bytes");
    assert_eq!(got, "10.42.0.255");
}
