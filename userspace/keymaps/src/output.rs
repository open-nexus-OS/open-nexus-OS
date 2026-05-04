// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Text and action outputs emitted by deterministic base keymap resolution.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: No direct tests (covered by 4 integration tests in `tests/input_v1_0_host/tests/keymaps_contract.rs`).
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    Enter,
    Escape,
    Backspace,
    Tab,
    ImeSwitch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyOutput {
    Text(char),
    Action(KeyAction),
}
