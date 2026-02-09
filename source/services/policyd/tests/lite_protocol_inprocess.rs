// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! Host-deterministic integration tests for policyd OS-lite wire handler.

#![forbid(unsafe_code)]
// These tests exercise the OS-lite wire handler; compile them only when the OS-lite
// implementation is enabled.
#![cfg(feature = "os-lite")]

use nexus_sel::{Policy, PolicyEntry};

#[test]
fn inprocess_check_binds_identity_to_sender() {
    let selftest_sid = nexus_abi::service_id_from_name(b"selftest-client");
    let bundle_sid = nexus_abi::service_id_from_name(b"bundlemgrd");
    let entries = [PolicyEntry { service_id: selftest_sid, capabilities: &["ipc.core"] }];
    let policy = Policy::new(&entries);

    // Spoof payload says "selftest-client", but sender is bundlemgrd and not privileged.
    let mut frame = Vec::new();
    frame.extend_from_slice(&[b'P', b'O', 1, 1]); // MAGIC, v1, OP_CHECK
    frame.push("selftest-client".len() as u8);
    frame.extend_from_slice(b"selftest-client");

    let out = policyd::lite_protocol::handle_frame(&policy, &frame, bundle_sid, false);
    // v1 response: status at byte 4
    assert_eq!(out.buf[4], 1 /* DENY */);
}

#[test]
fn inprocess_route_v3_spoof_rejected() {
    let samgrd = nexus_abi::service_id_from_name(b"samgrd");
    let execd = nexus_abi::service_id_from_name(b"execd");
    let bundle = nexus_abi::service_id_from_name(b"bundlemgrd");
    let entries = [PolicyEntry { service_id: samgrd, capabilities: &["ipc.core"] }];
    let policy = Policy::new(&entries);

    let mut buf = [0u8; 64];
    let n = nexus_abi::policyd::encode_route_v3_id(0x11223344, samgrd, execd, &mut buf).unwrap();
    let out = policyd::lite_protocol::handle_frame(&policy, &buf[..n], bundle, false);
    assert_eq!(out.len, 10);
    assert_eq!(out.buf[8], 1 /* DENY */);
}
