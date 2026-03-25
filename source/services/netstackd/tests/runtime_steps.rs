// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Runtime-step tests for netstackd bounded helper behavior
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 9 unit tests
//!
//! TEST_SCOPE:
//! - Bounded step outcomes
//! - UDP receive size capping
//! - Ping RTT capping
//! - Pending-connect state guards
//! - Validation shape rejection
//!
//! ADR: docs/adr/0005-dsoftbus-architecture.md

#[path = "../src/os/facade/ops.rs"]
mod ops;
#[path = "../src/os/facade/ping.rs"]
mod ping;
#[path = "../src/os/facade/tcp.rs"]
mod tcp;
#[path = "../src/os/facade/udp.rs"]
mod udp;
#[path = "../src/os/facade/validation.rs"]
mod validation;

#[test]
fn test_pending_connect_step_outcome() {
    assert_eq!(
        ops::pending_connect_ready(true),
        validation::StepOutcome::Ready
    );
    assert_eq!(
        ops::pending_connect_ready(false),
        validation::StepOutcome::WouldBlock
    );
}

#[test]
fn test_pending_connect_unexpected_state_detection() {
    assert!(ops::is_unexpected_pending_connect_state(true, true));
    assert!(!ops::is_unexpected_pending_connect_state(true, false));
    assert!(!ops::is_unexpected_pending_connect_state(false, true));
}

#[test]
fn test_tcp_wait_writable_outcome() {
    assert_eq!(
        tcp::wait_writable_outcome(true),
        validation::StepOutcome::Ready
    );
    assert_eq!(
        tcp::wait_writable_outcome(false),
        validation::StepOutcome::WouldBlock
    );
}

#[test]
fn test_udp_recv_max_bounded() {
    assert_eq!(udp::recv_max_bounded(12), 12);
    assert_eq!(udp::recv_max_bounded(999), 460);
}

#[test]
fn test_ping_rtt_cap() {
    assert_eq!(ping::cap_rtt_ms(42), 42);
    assert_eq!(ping::cap_rtt_ms(999_999), 65535);
}

#[test]
fn test_validation_outcome_shapes() {
    assert_eq!(
        validation::validate_exact_len(8, 8),
        validation::ValidationOutcome::Valid
    );
    assert!(validation::ValidationOutcome::Valid.is_valid());
    assert!(
        validation::validate_exact_or_nonce_len(9, 8).is_malformed(),
        "unexpected request shape must stay explicit"
    );
    assert_eq!(
        validation::validate_payload_len(16 + 3 + 8, 16, 3),
        validation::ValidationOutcome::Valid
    );
}

#[test]
fn test_validation_exact_or_nonce_len_accepts_only_allowed_shapes() {
    assert_eq!(
        validation::validate_exact_or_nonce_len(16, 16),
        validation::ValidationOutcome::Valid
    );
    assert_eq!(
        validation::validate_exact_or_nonce_len(24, 16),
        validation::ValidationOutcome::Valid
    );
    assert!(
        validation::validate_exact_or_nonce_len(17, 16).is_malformed(),
        "non-nonce offset must stay rejected"
    );
    assert!(
        validation::validate_exact_or_nonce_len(25, 16).is_malformed(),
        "overlong nonce-shaped frames must stay rejected"
    );
}

#[test]
fn test_validation_payload_len_rejects_mismatch() {
    assert_eq!(
        validation::validate_payload_len(16 + 4, 16, 4),
        validation::ValidationOutcome::Valid
    );
    assert_eq!(
        validation::validate_payload_len(16 + 4 + 8, 16, 4),
        validation::ValidationOutcome::Valid
    );
    assert!(
        validation::validate_payload_len(16 + 3, 16, 4).is_malformed(),
        "short payload must be malformed"
    );
    assert!(
        validation::validate_payload_len(16 + 4 + 7, 16, 4).is_malformed(),
        "partial nonce tail must be malformed"
    );
}

#[test]
fn test_validation_exact_len_rejects_mismatch() {
    assert_eq!(
        validation::validate_exact_len(12, 12),
        validation::ValidationOutcome::Valid
    );
    assert!(
        validation::validate_exact_len(11, 12).is_malformed(),
        "short fixed-size requests must be rejected"
    );
}
