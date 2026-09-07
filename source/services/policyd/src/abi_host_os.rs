// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the OS-lite `EvalHost` for `OP_ABI_EVAL` (RFC-0091 §5–§7):
//! monotonic clock (`nexus_abi::nsec`), the process-lifetime mode table
//! and learn collector (never persisted — every boot starts in Enforce),
//! and logd (scope `policyd.learn`, the same deterministic append the
//! audit path uses) as the learn sink. `OP_SET_ABI_MODE` (TASK-0028 P3)
//! writes `ABI_MODES`.
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: host logic in abi_eval.rs / abi_learn.rs tests; QEMU
//!   `SELFTEST: abi learn collected ok` (P3)
//! RFC: docs/rfcs/RFC-0091-policy-profile-v2-schema-wire-argument-matchers.md

use crate::abi_learn::{AbiMode, EvalHost, LearnState, ModeTable, LEARN_SCOPE};
use crate::os_lite::append_logd_deterministic;

static ABI_MODES: ModeTable = ModeTable::new();
static ABI_LEARN: LearnState = LearnState::new();

/// The OS host for `OP_ABI_EVAL`: monotonic clock, the mode table and
/// logd (scope `policyd.learn`) as the learn sink.
pub(crate) struct OsEvalHost;

impl EvalHost for OsEvalHost {
    fn now_ns(&self) -> u64 {
        nexus_abi::nsec().unwrap_or(0)
    }
    fn mode_of(&self, subject: u64) -> AbiMode {
        ABI_MODES.mode_of(subject)
    }
    fn learn_state(&self) -> &LearnState {
        &ABI_LEARN
    }
    fn emit_learn(&mut self, record: &[u8]) -> bool {
        append_logd_deterministic(LEARN_SCOPE.as_bytes(), record)
    }
}
