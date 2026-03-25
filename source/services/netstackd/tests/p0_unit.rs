// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Unit tests for netstackd extracted pure seams and typed helpers
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 10 unit tests
//!
//! TEST_SCOPE:
//! - Fallback IPv4 profile mapping
//! - Typed handle conversions
//! - Typed handle rejection semantics
//!
//! ADR: docs/adr/0005-dsoftbus-architecture.md

#[path = "../src/os/entry_pure.rs"]
mod entry_pure;
#[path = "../src/os/ipc/handles.rs"]
mod handles;

#[test]
fn test_fallback_ipv4_qemu_smoke_profile() {
    let (ip, prefix, gw) =
        entry_pure::fallback_ipv4_config(true, [0x52, 0x54, 0x00, 0x12, 0x34, 0x0a]);
    assert_eq!(ip, entry_pure::QEMU_USERNET_FALLBACK_IP);
    assert_eq!(prefix, 24);
    assert_eq!(gw, Some(entry_pure::QEMU_USERNET_GATEWAY_IP));
}

#[test]
fn test_fallback_ipv4_os2vm_profile_uses_mac_lsb() {
    let (ip, prefix, gw) =
        entry_pure::fallback_ipv4_config(false, [0x52, 0x54, 0x00, 0x12, 0x34, 0x0a]);
    assert_eq!(ip, entry_pure::OS2VM_NODE_A_IP);
    assert_eq!(prefix, 24);
    assert_eq!(gw, None);
}

#[test]
fn test_fallback_ipv4_os2vm_profile_zero_mac_lsb_maps_to_one() {
    let (ip, prefix, gw) =
        entry_pure::fallback_ipv4_config(false, [0x52, 0x54, 0x00, 0x12, 0x34, 0x00]);
    assert_eq!(ip, [10, 42, 0, 1]);
    assert_eq!(prefix, 24);
    assert_eq!(gw, None);
}

#[test]
fn test_is_qemu_loopback_target_only_for_expected_endpoints() {
    assert!(entry_pure::is_qemu_loopback_target(
        entry_pure::QEMU_USERNET_FALLBACK_IP,
        8080,
        8080,
        8081
    ));
    assert!(entry_pure::is_qemu_loopback_target(
        entry_pure::QEMU_USERNET_FALLBACK_IP,
        8081,
        8080,
        8081
    ));
    assert!(!entry_pure::is_qemu_loopback_target([10, 0, 2, 16], 8080, 8080, 8081));
    assert!(!entry_pure::is_qemu_loopback_target(
        entry_pure::QEMU_USERNET_FALLBACK_IP,
        8090,
        8080,
        8081
    ));
}

#[test]
fn test_typed_handle_roundtrip() {
    assert_eq!(handles::ListenerId::to_wire(0), 1);
    assert_eq!(handles::ListenerId::from_wire(1).map(|id| id.index()), Some(0));
    assert_eq!(handles::StreamId::to_wire(1), 2);
    assert_eq!(handles::StreamId::from_index(1).index(), 1);
    assert_eq!(handles::UdpId::to_wire(2), 3);
    assert_eq!(handles::UdpId::from_wire(3).map(|id| id.index()), Some(2));
    assert_eq!(handles::StreamId::from_wire(5).map(|id| id.index()), Some(4));
}

#[test]
fn test_reply_cap_slot_roundtrip() {
    let slot = handles::ReplyCapSlot::new(6);
    assert_eq!(slot.raw(), 6);
}

#[test]
fn test_reject_invalid_wire_handles() {
    assert_eq!(handles::ListenerId::from_wire(0), None);
    assert_eq!(handles::StreamId::from_wire(0), None);
    assert_eq!(handles::UdpId::from_wire(0), None);
}

#[test]
fn test_dns_probe_response_accepts_port53_with_txid_and_response_flag() {
    let mut frame = [0u8; 12];
    frame[0] = 0x12;
    frame[1] = 0x34;
    frame[2] = 0x80; // QR bit
    assert!(entry_pure::is_dns_probe_response(&frame, entry_pure::DNS_SERVER_PORT));
}

#[test]
fn test_dns_probe_response_rejects_wrong_port_txid_or_short_frame() {
    let mut frame = [0u8; 12];
    frame[0] = 0x12;
    frame[1] = 0x34;
    frame[2] = 0x80;
    assert!(!entry_pure::is_dns_probe_response(&frame, 9_999));

    frame[1] = 0x35;
    assert!(!entry_pure::is_dns_probe_response(&frame, entry_pure::DNS_SERVER_PORT));

    let short = [0x12u8, 0x34u8];
    assert!(!entry_pure::is_dns_probe_response(&short, entry_pure::DNS_SERVER_PORT));
}

#[test]
fn test_address_profile_constants_contract() {
    assert_eq!(entry_pure::QEMU_USERNET_DNS_PRIMARY_IP, [10, 0, 2, 3]);
    assert_eq!(entry_pure::OS2VM_NODE_B_IP, [10, 42, 0, 11]);
}
