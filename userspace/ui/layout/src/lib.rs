// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
//!
//! CONTEXT: Layout engine for TASK-0058 / RFC-0057.
//! OWNERS: @ui
//! STATUS: Done
//! API_STABILITY: Unstable
//! TEST_COVERAGE: engine_tests (8) + tests/ui_v3a_host/
//! ADR: docs/rfcs/RFC-0057-ui-v3a-layout-engine-pretext-contract.md

#![deny(unsafe_code)]
#![allow(clippy::too_many_arguments)]

extern crate alloc;

pub mod engine;
#[cfg(test)]
mod engine_tests;
pub mod error;

pub use engine::{
    compute_scroll_damage, LayoutBox, LayoutEngine, LayoutResult, ScrollDamage,
};
pub use error::LayoutError;
