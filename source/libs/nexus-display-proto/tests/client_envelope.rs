// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the client-surface envelope must be recognisable as "addressed to
//! windowd's SERVER endpoint" from its first three bytes, and a gpud present
//! verdict (a bare status byte) must never look like one. windowd's gpud-reply
//! drain uses `is_client_envelope` to tell "not a gpud reply" from "not my
//! endpoint at all"; on 2026-07-25 it could not, and silently ate 29 client
//! frames off an aliased endpoint (`build/logs/manual--2026-07-25T15-57-20`) —
//! the desktop's events-attach (len=12) and geometry intent plus inputd's
//! batches (len=32), producing a 320x240 desktop that never routed input.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal (test-only)
//! TEST_COVERAGE: cargo test -p nexus-display-proto --test client_envelope

use nexus_display_proto::client_surface as wire;

/// The first byte of every client frame is `b'I'` (0x49) — the byte that showed
/// up in windowd's log as a bogus "status". Pinned so the next reader of such a
/// log line can map 0x49 back to this envelope instead of hunting a status code.
#[test]
fn envelope_magic_is_pinned() {
    assert_eq!(wire::ENVELOPE_MAGIC0, b'I');
    assert_eq!(wire::ENVELOPE_MAGIC1, b'N');
    assert_eq!(wire::ENVELOPE_VERSION, 1);
}

#[test]
fn client_frames_are_claimed() {
    assert!(wire::is_client_envelope(&wire::encode_surface_events(0xDEAD_BEEF)));
    // Input-live ops (1..=4) share the header family on the same endpoint.
    for op in [1u8, 2, 3, 4, wire::OP_SURFACE_INTENT, wire::OP_SURFACE_PRESENT] {
        let frame = [wire::ENVELOPE_MAGIC0, wire::ENVELOPE_MAGIC1, wire::ENVELOPE_VERSION, op];
        assert!(wire::is_client_envelope(&frame), "op {op} must be claimed");
    }
}

#[test]
fn test_reject_gpud_verdicts_and_malformed() {
    // gpud present verdicts are `[status, handoff_id…]`, stati 0/1/2.
    for status in [
        nexus_display_proto::STATUS_OK,
        nexus_display_proto::STATUS_MALFORMED,
        nexus_display_proto::STATUS_DEVICE_ERROR,
    ] {
        assert!(!wire::is_client_envelope(&[status, 0, 0, 0, 0]));
    }
    // Truncated, wrong magic, wrong version: not ours.
    assert!(!wire::is_client_envelope(&[]));
    assert!(!wire::is_client_envelope(&[wire::ENVELOPE_MAGIC0, wire::ENVELOPE_MAGIC1]));
    assert!(!wire::is_client_envelope(&[b'X', wire::ENVELOPE_MAGIC1, wire::ENVELOPE_VERSION, 12]));
    assert!(!wire::is_client_envelope(&[
        wire::ENVELOPE_MAGIC0,
        wire::ENVELOPE_MAGIC1,
        wire::ENVELOPE_VERSION + 1,
        12
    ]));
}
