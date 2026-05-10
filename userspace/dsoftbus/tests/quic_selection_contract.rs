// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//!
//! CONTEXT: Behavior-first host proofs for TASK-0021 Phase B QUIC selection contract
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 8 integration tests (selection/reject + deterministic perf budget assertions)
//! ADR: docs/adr/0005-dsoftbus-architecture.md
//!
//! Target behavior: transport selection for `auto|tcp|quic` is deterministic and fail-closed where required.
//! Main break point: silent downgrade in `mode=quic` or warning-only validation failures.
//! Primary proof: requirement-named host tests; secondary proof remains Phase C OS marker wiring.

#![cfg(nexus_env = "host")]

use dsoftbus::transport_selection::{
    fallback_marker_budget, quic_attempts_for_mode, select_transport, QuicProbe, TransportKind,
    TransportMode, TransportSelectionError, AUTO_FALLBACK_MARKER_COUNT,
    MARKER_QUIC_OS_DISABLED_FALLBACK_TCP, MARKER_SELFTEST_QUIC_FALLBACK_OK,
    MARKER_TRANSPORT_SELECTED_QUIC, MARKER_TRANSPORT_SELECTED_TCP,
};

#[test]
fn test_select_quic_positive_path() {
    let outcome = select_transport(
        TransportMode::Quic,
        QuicProbe::Candidate {
            expected_alpn: "nexus.dsoftbus.v1",
            offered_alpn: "nexus.dsoftbus.v1",
            cert_trusted: true,
        },
    )
    .expect("quic selection should succeed with valid alpn/cert");

    assert_eq!(outcome.transport(), TransportKind::Quic);
    assert_eq!(outcome.markers(), &[MARKER_TRANSPORT_SELECTED_QUIC]);
}

#[test]
fn test_reject_quic_wrong_alpn() {
    let err = select_transport(
        TransportMode::Quic,
        QuicProbe::Candidate {
            expected_alpn: "nexus.dsoftbus.v1",
            offered_alpn: "h3",
            cert_trusted: true,
        },
    )
    .expect_err("wrong ALPN must reject");

    assert_eq!(err, TransportSelectionError::RejectQuicWrongAlpn);
}

#[test]
fn test_reject_quic_invalid_or_untrusted_cert() {
    let err = select_transport(
        TransportMode::Quic,
        QuicProbe::Candidate {
            expected_alpn: "nexus.dsoftbus.v1",
            offered_alpn: "nexus.dsoftbus.v1",
            cert_trusted: false,
        },
    )
    .expect_err("untrusted cert must reject");

    assert_eq!(
        err,
        TransportSelectionError::RejectQuicInvalidOrUntrustedCert
    );
}

#[test]
fn test_reject_quic_strict_mode_downgrade() {
    let err = select_transport(TransportMode::Quic, QuicProbe::Disabled)
        .expect_err("strict mode must reject downgrade");

    assert_eq!(err, TransportSelectionError::RejectQuicStrictModeDowngrade);
}

#[test]
fn test_auto_mode_fallback_marker_emitted() {
    let outcome = select_transport(TransportMode::Auto, QuicProbe::Disabled)
        .expect("auto mode should fallback to tcp when quic is disabled");

    assert_eq!(outcome.transport(), TransportKind::Tcp);
    assert_eq!(
        outcome.markers(),
        &[
            MARKER_QUIC_OS_DISABLED_FALLBACK_TCP,
            MARKER_TRANSPORT_SELECTED_TCP,
            MARKER_SELFTEST_QUIC_FALLBACK_OK,
        ],
    );
}

#[test]
fn test_perf_budget_tcp_mode_no_quic_attempts() {
    assert_eq!(quic_attempts_for_mode(TransportMode::Tcp), 0);
    assert_eq!(fallback_marker_budget(TransportMode::Tcp), 0);
}

#[test]
fn test_perf_budget_quic_mode_single_attempt() {
    assert_eq!(quic_attempts_for_mode(TransportMode::Quic), 1);
    assert_eq!(fallback_marker_budget(TransportMode::Quic), 0);
}

#[test]
fn test_perf_budget_auto_mode_single_attempt_and_fallback_marker_count() {
    assert_eq!(quic_attempts_for_mode(TransportMode::Auto), 1);
    assert_eq!(
        fallback_marker_budget(TransportMode::Auto),
        AUTO_FALLBACK_MARKER_COUNT
    );

    let outcome = select_transport(TransportMode::Auto, QuicProbe::Disabled)
        .expect("auto mode should deterministically fallback");
    assert_eq!(outcome.transport(), TransportKind::Tcp);
    assert_eq!(outcome.markers().len(), AUTO_FALLBACK_MARKER_COUNT);
}
