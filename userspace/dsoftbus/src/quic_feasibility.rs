// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//!
//! CONTEXT: Phase-D feasibility contract helpers for TASK-0023 blocked-gate evaluation.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Experimental
//! TEST_COVERAGE: `userspace/dsoftbus/tests/quic_feasibility_contract.rs`
//! ADR: docs/adr/0005-dsoftbus-architecture.md
//!
//! This module does not unlock OS QUIC by itself. It only validates whether
//! feasibility criteria are explicit, bounded, and deterministic.

#![forbid(unsafe_code)]

use thiserror::Error;

pub const MAX_FEASIBILITY_RETRANSMITS: u16 = 64;
pub const MAX_FEASIBILITY_INFLIGHT_PACKETS: u16 = 2048;
pub const MAX_FEASIBILITY_REORDER_PACKETS: u16 = 512;
pub const CURRENT_OS_QUIC_RUNTIME_IMPLEMENTATION: QuicOsRuntimeImplementation =
    QuicOsRuntimeImplementation::ImplementedAndWired;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuicOsRuntimeImplementation {
    ImplementedAndWired,
    Unimplemented,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuicRuntimeBoundary {
    IsolatedStdBoundary,
    UnisolatedStdRuntime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuicTimerDeterminism {
    DeterministicBounded,
    RuntimeDependent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuicEntropyReadiness {
    Ready,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use = "loss/retry budgets are decision-bearing for feasibility claims"]
pub struct LossRetryBudget {
    max_retransmits: u16,
    max_inflight_packets: u16,
    max_reorder_packets: u16,
}

impl LossRetryBudget {
    pub fn new(
        max_retransmits: u16,
        max_inflight_packets: u16,
        max_reorder_packets: u16,
    ) -> Option<Self> {
        if max_inflight_packets == 0 {
            return None;
        }
        Some(Self { max_retransmits, max_inflight_packets, max_reorder_packets })
    }

    #[must_use]
    pub fn max_retransmits(&self) -> u16 {
        self.max_retransmits
    }

    #[must_use]
    pub fn max_inflight_packets(&self) -> u16 {
        self.max_inflight_packets
    }

    #[must_use]
    pub fn max_reorder_packets(&self) -> u16 {
        self.max_reorder_packets
    }

    #[must_use]
    fn exceeds_phase_d_bounds(&self) -> bool {
        self.max_retransmits > MAX_FEASIBILITY_RETRANSMITS
            || self.max_inflight_packets > MAX_FEASIBILITY_INFLIGHT_PACKETS
            || self.max_reorder_packets > MAX_FEASIBILITY_REORDER_PACKETS
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use = "feasibility inputs are decision-bearing and must be consumed"]
pub struct QuicFeasibilityInput {
    os_runtime_implementation: QuicOsRuntimeImplementation,
    runtime_boundary: QuicRuntimeBoundary,
    timer_determinism: QuicTimerDeterminism,
    entropy_readiness: QuicEntropyReadiness,
    budget: LossRetryBudget,
}

impl QuicFeasibilityInput {
    #[must_use]
    pub fn new(
        os_runtime_implementation: QuicOsRuntimeImplementation,
        runtime_boundary: QuicRuntimeBoundary,
        timer_determinism: QuicTimerDeterminism,
        entropy_readiness: QuicEntropyReadiness,
        budget: LossRetryBudget,
    ) -> Self {
        Self { os_runtime_implementation, runtime_boundary, timer_determinism, entropy_readiness, budget }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use = "phase-d assessment must be reviewed before any gate status update"]
pub struct QuicFeasibilityAssessment {
    budget: LossRetryBudget,
}

impl QuicFeasibilityAssessment {
    fn new(budget: LossRetryBudget) -> Self {
        Self { budget }
    }

    #[must_use]
    pub fn requires_manual_unlock_review(&self) -> bool {
        true
    }

    #[must_use]
    pub fn budget(&self) -> LossRetryBudget {
        self.budget
    }
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
#[must_use = "feasibility rejects are security-critical and must not be ignored"]
pub enum QuicFeasibilityError {
    #[error("quic feasibility reject: os runtime implementation is missing")]
    RejectQuicFeasibilityImplementationMissing,
    #[error("quic feasibility reject: std runtime coupling is not isolated")]
    RejectQuicFeasibilityStdRuntimeCoupling,
    #[error("quic feasibility reject: timer model is not deterministic/bounded")]
    RejectQuicFeasibilityNonDeterministicTimers,
    #[error("quic feasibility reject: entropy prerequisites unavailable")]
    RejectQuicFeasibilityEntropyPrerequisites,
    #[error("quic feasibility reject: loss/retry budgets are unbounded")]
    RejectQuicFeasibilityUnboundedLossRetryBudget,
}

pub fn assess_quic_phase_d_feasibility(
    input: QuicFeasibilityInput,
) -> Result<QuicFeasibilityAssessment, QuicFeasibilityError> {
    if input.os_runtime_implementation != QuicOsRuntimeImplementation::ImplementedAndWired {
        return Err(QuicFeasibilityError::RejectQuicFeasibilityImplementationMissing);
    }
    if input.runtime_boundary != QuicRuntimeBoundary::IsolatedStdBoundary {
        return Err(QuicFeasibilityError::RejectQuicFeasibilityStdRuntimeCoupling);
    }
    if input.timer_determinism != QuicTimerDeterminism::DeterministicBounded {
        return Err(QuicFeasibilityError::RejectQuicFeasibilityNonDeterministicTimers);
    }
    if input.entropy_readiness != QuicEntropyReadiness::Ready {
        return Err(QuicFeasibilityError::RejectQuicFeasibilityEntropyPrerequisites);
    }
    if input.budget.exceeds_phase_d_bounds() {
        return Err(QuicFeasibilityError::RejectQuicFeasibilityUnboundedLossRetryBudget);
    }
    Ok(QuicFeasibilityAssessment::new(input.budget))
}
