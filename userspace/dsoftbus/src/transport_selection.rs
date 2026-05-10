// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//!
//! CONTEXT: Deterministic transport selection contract for host-first QUIC scaffold
//! OWNERS: @runtime
//! STATUS: Functional (TASK-0021 host-first selection contract)
//! API_STABILITY: Experimental
//! TEST_COVERAGE: `userspace/dsoftbus/tests/quic_selection_contract.rs`, `userspace/dsoftbus/tests/quic_host_transport_contract.rs`
//! ADR: docs/adr/0005-dsoftbus-architecture.md

#![forbid(unsafe_code)]

use thiserror::Error;

pub const MARKER_QUIC_OS_DISABLED_FALLBACK_TCP: &str = "dsoftbus: quic os disabled (fallback tcp)";
pub const MARKER_SELFTEST_QUIC_FALLBACK_OK: &str = "SELFTEST: quic fallback ok";
pub const MARKER_TRANSPORT_SELECTED_TCP: &str = "dsoftbusd: transport selected tcp";
pub const MARKER_TRANSPORT_SELECTED_QUIC: &str = "dsoftbusd: transport selected quic";
pub const AUTO_FALLBACK_MARKER_COUNT: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportMode {
    Auto,
    Tcp,
    Quic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    Tcp,
    Quic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuicProbe<'a> {
    Disabled,
    Candidate { expected_alpn: &'a str, offered_alpn: &'a str, cert_trusted: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use = "transport selection outcomes must be consumed to enforce fallback semantics"]
pub struct TransportSelectionOutcome {
    transport: TransportKind,
    markers: Vec<&'static str>,
}

impl TransportSelectionOutcome {
    fn new(transport: TransportKind, markers: Vec<&'static str>) -> Self {
        Self { transport, markers }
    }

    #[must_use]
    pub fn transport(&self) -> TransportKind {
        self.transport
    }

    #[must_use]
    pub fn markers(&self) -> &[&'static str] {
        &self.markers
    }
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
#[must_use = "transport selection errors must be handled to prevent silent downgrade"]
pub enum TransportSelectionError {
    #[error("quic reject: wrong alpn")]
    RejectQuicWrongAlpn,
    #[error("quic reject: invalid or untrusted cert")]
    RejectQuicInvalidOrUntrustedCert,
    #[error("quic reject: strict-mode downgrade denied")]
    RejectQuicStrictModeDowngrade,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuicValidationError {
    WrongAlpn,
    InvalidOrUntrustedCert,
    Unavailable,
}

fn validate_quic_probe(probe: QuicProbe<'_>) -> Result<(), QuicValidationError> {
    match probe {
        QuicProbe::Disabled => Err(QuicValidationError::Unavailable),
        QuicProbe::Candidate { expected_alpn, offered_alpn, cert_trusted } => {
            if expected_alpn != offered_alpn {
                return Err(QuicValidationError::WrongAlpn);
            }
            if !cert_trusted {
                return Err(QuicValidationError::InvalidOrUntrustedCert);
            }
            Ok(())
        }
    }
}

#[must_use]
pub fn quic_attempts_for_mode(mode: TransportMode) -> u8 {
    match mode {
        TransportMode::Tcp => 0,
        TransportMode::Quic | TransportMode::Auto => 1,
    }
}

#[must_use]
pub fn fallback_marker_budget(mode: TransportMode) -> usize {
    match mode {
        TransportMode::Auto => AUTO_FALLBACK_MARKER_COUNT,
        TransportMode::Tcp | TransportMode::Quic => 0,
    }
}

pub fn select_transport(
    mode: TransportMode,
    quic_probe: QuicProbe<'_>,
) -> Result<TransportSelectionOutcome, TransportSelectionError> {
    match mode {
        TransportMode::Tcp => Ok(TransportSelectionOutcome::new(
            TransportKind::Tcp,
            vec![MARKER_TRANSPORT_SELECTED_TCP],
        )),
        TransportMode::Quic => match validate_quic_probe(quic_probe) {
            Ok(()) => Ok(TransportSelectionOutcome::new(
                TransportKind::Quic,
                vec![MARKER_TRANSPORT_SELECTED_QUIC],
            )),
            Err(QuicValidationError::WrongAlpn) => {
                Err(TransportSelectionError::RejectQuicWrongAlpn)
            }
            Err(QuicValidationError::InvalidOrUntrustedCert) => {
                Err(TransportSelectionError::RejectQuicInvalidOrUntrustedCert)
            }
            Err(QuicValidationError::Unavailable) => {
                Err(TransportSelectionError::RejectQuicStrictModeDowngrade)
            }
        },
        TransportMode::Auto => match validate_quic_probe(quic_probe) {
            Ok(()) => Ok(TransportSelectionOutcome::new(
                TransportKind::Quic,
                vec![MARKER_TRANSPORT_SELECTED_QUIC],
            )),
            Err(QuicValidationError::WrongAlpn) => {
                Err(TransportSelectionError::RejectQuicWrongAlpn)
            }
            Err(QuicValidationError::InvalidOrUntrustedCert) => {
                Err(TransportSelectionError::RejectQuicInvalidOrUntrustedCert)
            }
            Err(QuicValidationError::Unavailable) => Ok(TransportSelectionOutcome::new(
                TransportKind::Tcp,
                vec![
                    MARKER_QUIC_OS_DISABLED_FALLBACK_TCP,
                    MARKER_TRANSPORT_SELECTED_TCP,
                    MARKER_SELFTEST_QUIC_FALLBACK_OK,
                ],
            )),
        },
    }
}
