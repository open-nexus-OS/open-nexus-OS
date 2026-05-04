// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Deterministic modifier state used by shared base keymap resolution.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: No direct tests (covered by 4 integration tests in `tests/input_v1_0_host/tests/keymaps_contract.rs`).
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    shift: bool,
    control: bool,
    alt_gr: bool,
}

impl Modifiers {
    #[must_use]
    pub const fn with_shift(mut self) -> Self {
        self.shift = true;
        self
    }

    #[must_use]
    pub const fn with_control(mut self) -> Self {
        self.control = true;
        self
    }

    #[must_use]
    pub const fn with_alt_gr(mut self) -> Self {
        self.alt_gr = true;
        self
    }

    #[must_use]
    pub const fn shift(self) -> bool {
        self.shift
    }

    #[must_use]
    pub const fn control(self) -> bool {
        self.control
    }

    #[must_use]
    pub const fn alt_gr(self) -> bool {
        self.alt_gr
    }
}
