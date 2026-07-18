// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Host proof suite for TASK-0075 (DSL v0.1a): parser corpus with
//! stable diagnostic codes, fmt idempotence, build determinism (byte-compare),
//! IR goldens, loader-side validation rejects.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! TEST_COVERAGE: this crate IS the coverage

// reason: test harness — a failed parse/lower of a fixture must panic loudly to
// fail the proof, not be silently propagated.
#![allow(clippy::expect_used, clippy::unwrap_used)]

/// Compiles a fixture end-to-end and returns the canonical `.nxir` bytes.
///
/// # Panics
/// On any parse/check/lower failure — fixtures in this suite must be valid.
#[must_use]
pub fn compile(source: &str) -> Vec<u8> {
    let file = nexus_dsl_core::parse_file(source).expect("fixture parses");
    let (model, diags) = nexus_dsl_core::check_file(&file);
    assert!(!nexus_dsl_core::has_errors(&diags), "fixture check errors: {diags:?}");
    let canonical = nexus_dsl_core::format_file(&file);
    nexus_dsl_core::lower_file(&file, &model, &canonical).expect("fixture lowers").nxir
}

/// The shared proof-surface fixture (text, icon, keyed list, interactive
/// control, overlay-ish card) — later interpreter/OS tasks mount this same
/// structure instead of inventing a separate demo (TASK-0075 DoD).
pub const PROOF_SURFACE: &str = include_str!("../fixtures/proof_surface.nx");
