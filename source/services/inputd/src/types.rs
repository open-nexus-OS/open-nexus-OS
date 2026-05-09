// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Typed `inputd` dispatch records and IME hook events for TASK-0253.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 8 host contract tests in the `inputd` crate.
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

use keymaps::KeyOutput;
use touch::TouchEvent;
use windowd::InputDelivery;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImeHook {
    Show,
    Hide,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputDispatch {
    PointerMove {
        delivery: InputDelivery,
        x: i32,
        y: i32,
    },
    PointerDown {
        delivery: InputDelivery,
        x: i32,
        y: i32,
    },
    Keyboard {
        delivery: InputDelivery,
        key_code: u32,
        output: KeyOutput,
        repeated: bool,
    },
    Touch {
        delivery: InputDelivery,
        event: TouchEvent,
        x: i32,
        y: i32,
    },
    ImeHook(ImeHook),
}
