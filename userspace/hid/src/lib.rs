// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: TASK-0252 transport-neutral USB-HID boot parser primitives.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: No direct tests (covered by 5 integration tests in `tests/input_v1_0_host/tests/hid_contract.rs`).
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

//! CONTEXT: TASK-0252 transport-neutral USB-HID boot parser primitives.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Stable for TASK-0252 host proof floor
//! TEST_COVERAGE: Integration coverage in `tests/input_v1_0_host/tests/hid_contract.rs`.
//! ADR: docs/rfcs/RFC-0052-input-v1_0a-host-hid-touch-keymaps-repeat-accel-contract.md

#![forbid(unsafe_code)]

mod error;
mod event;
mod keyboard;
mod mouse;
mod usage;

pub use error::HidError;
pub use event::{HidCode, HidEvent, HidEventKind, HidValue, TimestampNs};
pub use keyboard::BootKeyboardParser;
pub use mouse::BootMouseParser;
pub use usage::{KeyboardUsage, MouseButton, RelativeAxis};
