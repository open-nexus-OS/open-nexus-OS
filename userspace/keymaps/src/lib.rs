// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: TASK-0252 shared deterministic base keymap authority for input and IME follow-ups.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: No direct tests (covered by 4 integration tests in `tests/input_v1_0_host/tests/keymaps_contract.rs`).
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

//! CONTEXT: TASK-0252 shared deterministic base keymap authority for input and IME follow-ups.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Stable for TASK-0252 host proof floor
//! TEST_COVERAGE: Integration coverage in `tests/input_v1_0_host/tests/keymaps_contract.rs`.
//! ADR: docs/rfcs/RFC-0052-input-v1_0a-host-hid-touch-keymaps-repeat-accel-contract.md

#![forbid(unsafe_code)]

mod error;
mod layout;
mod modifiers;
mod output;
mod table;

pub use error::KeymapError;
pub use layout::{Keymap, LayoutId};
pub use modifiers::Modifiers;
pub use output::{KeyAction, KeyOutput};
