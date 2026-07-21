// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: TASK-0146 / RFC-0075 deterministic IME composition core (dead keys,
//! compose tables, preedit/commit outcomes). Hosted by `imed`; no I/O, no IPC.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Stable for RFC-0075 Phase 0
//! TEST_COVERAGE: Unit + integration tests in `tests/compose_contract.rs`.
//! RFC: docs/rfcs/RFC-0075-ime-v2-text-focus-composition-delivery.md

#![cfg_attr(all(nexus_env = "os", target_os = "none"), no_std)]
#![forbid(unsafe_code)]

mod compose;
mod outcome;

pub use compose::{Composer, COMPOSE_PENDING_MAX};
pub use outcome::{Commit, ImeAction, ImeKey, ImeOutcome, Preedit, PREEDIT_MAX_BYTES};
