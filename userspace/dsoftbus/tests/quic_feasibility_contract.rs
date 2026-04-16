// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//!
//! CONTEXT: Behavior-first Phase-D feasibility contract proofs for TASK-0023.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Experimental
//! TEST_COVERAGE: Requirement-named feasibility reject paths + bounded positive criteria check.
//! ADR: docs/adr/0005-dsoftbus-architecture.md
//!
//! Target behavior: feasibility claims are rejected unless runtime boundary, timer determinism,
//! entropy readiness, and loss/retry boundedness are all explicit and valid.
//! Main break point: reporting feasibility without deterministic and bounded constraints.

#![cfg(nexus_env = "host")]

use dsoftbus::{
    assess_quic_phase_d_feasibility, LossRetryBudget, QuicEntropyReadiness, QuicFeasibilityError,
    QuicFeasibilityInput, QuicOsRuntimeImplementation, QuicRuntimeBoundary, QuicTimerDeterminism,
    CURRENT_OS_QUIC_RUNTIME_IMPLEMENTATION,
};

fn future_implemented_baseline_input() -> QuicFeasibilityInput {
    QuicFeasibilityInput::new(
        QuicOsRuntimeImplementation::ImplementedAndWired,
        QuicRuntimeBoundary::IsolatedStdBoundary,
        QuicTimerDeterminism::DeterministicBounded,
        QuicEntropyReadiness::Ready,
        LossRetryBudget::new(8, 256, 64).expect("bounded retry/inflight/reorder budget"),
    )
}

#[test]
fn test_current_os_runtime_state_is_feasibility_eligible() {
    let input = QuicFeasibilityInput::new(
        CURRENT_OS_QUIC_RUNTIME_IMPLEMENTATION,
        QuicRuntimeBoundary::IsolatedStdBoundary,
        QuicTimerDeterminism::DeterministicBounded,
        QuicEntropyReadiness::Ready,
        LossRetryBudget::new(8, 256, 64).expect("bounded budget"),
    );
    let assessment = assess_quic_phase_d_feasibility(input)
        .expect("current OS runtime state should be eligible");
    assert!(assessment.requires_manual_unlock_review());
}

#[test]
fn test_reject_quic_feasibility_os_runtime_unimplemented() {
    let input = QuicFeasibilityInput::new(
        QuicOsRuntimeImplementation::Unimplemented,
        QuicRuntimeBoundary::IsolatedStdBoundary,
        QuicTimerDeterminism::DeterministicBounded,
        QuicEntropyReadiness::Ready,
        LossRetryBudget::new(8, 256, 64).expect("bounded budget"),
    );
    let err = assess_quic_phase_d_feasibility(input)
        .expect_err("runtime marked unimplemented must reject QUIC feasibility unlock");
    assert_eq!(err, QuicFeasibilityError::RejectQuicFeasibilityImplementationMissing);
}

#[test]
fn test_reject_quic_feasibility_std_runtime_coupling_even_if_runtime_is_marked_implemented() {
    let input = QuicFeasibilityInput::new(
        QuicOsRuntimeImplementation::ImplementedAndWired,
        QuicRuntimeBoundary::UnisolatedStdRuntime,
        QuicTimerDeterminism::DeterministicBounded,
        QuicEntropyReadiness::Ready,
        LossRetryBudget::new(8, 256, 64).expect("bounded budget"),
    );
    let err = assess_quic_phase_d_feasibility(input)
        .expect_err("std-coupled runtime assumptions must reject feasibility");
    assert_eq!(err, QuicFeasibilityError::RejectQuicFeasibilityStdRuntimeCoupling);
}

#[test]
fn test_reject_quic_feasibility_non_deterministic_timer_assumptions() {
    let input = QuicFeasibilityInput::new(
        QuicOsRuntimeImplementation::ImplementedAndWired,
        QuicRuntimeBoundary::IsolatedStdBoundary,
        QuicTimerDeterminism::RuntimeDependent,
        QuicEntropyReadiness::Ready,
        LossRetryBudget::new(8, 256, 64).expect("bounded budget"),
    );
    let err = assess_quic_phase_d_feasibility(input)
        .expect_err("runtime-dependent timers must reject feasibility");
    assert_eq!(err, QuicFeasibilityError::RejectQuicFeasibilityNonDeterministicTimers);
}

#[test]
fn test_reject_quic_feasibility_entropy_prerequisites_unsatisfied() {
    let input = QuicFeasibilityInput::new(
        QuicOsRuntimeImplementation::ImplementedAndWired,
        QuicRuntimeBoundary::IsolatedStdBoundary,
        QuicTimerDeterminism::DeterministicBounded,
        QuicEntropyReadiness::Unavailable,
        LossRetryBudget::new(8, 256, 64).expect("bounded budget"),
    );
    let err = assess_quic_phase_d_feasibility(input)
        .expect_err("missing entropy prerequisites must reject feasibility");
    assert_eq!(err, QuicFeasibilityError::RejectQuicFeasibilityEntropyPrerequisites);
}

#[test]
fn test_reject_quic_feasibility_unbounded_loss_retry_budget() {
    let budget = LossRetryBudget::new(512, 16_384, 4_096)
        .expect("input budget object may exist but must fail feasibility bounds");
    let input = QuicFeasibilityInput::new(
        QuicOsRuntimeImplementation::ImplementedAndWired,
        QuicRuntimeBoundary::IsolatedStdBoundary,
        QuicTimerDeterminism::DeterministicBounded,
        QuicEntropyReadiness::Ready,
        budget,
    );
    let err = assess_quic_phase_d_feasibility(input)
        .expect_err("unbounded retry/inflight/reorder assumptions must reject");
    assert_eq!(err, QuicFeasibilityError::RejectQuicFeasibilityUnboundedLossRetryBudget);
}

#[test]
fn test_phase_d_feasibility_criteria_satisfied_requires_manual_unlock_review() {
    let assessment = assess_quic_phase_d_feasibility(future_implemented_baseline_input())
        .expect("bounded deterministic profile");

    assert!(assessment.requires_manual_unlock_review());
    assert_eq!(assessment.budget().max_retransmits(), 8);
    assert_eq!(assessment.budget().max_inflight_packets(), 256);
    assert_eq!(assessment.budget().max_reorder_packets(), 64);
}

#[test]
fn test_phase_d_feasibility_contract_types_are_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}

    assert_send_sync::<LossRetryBudget>();
    assert_send_sync::<QuicFeasibilityInput>();
}
