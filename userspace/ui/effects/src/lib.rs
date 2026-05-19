// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: CPU effects (blur/drop-shadow) with deterministic budgets for TASK-0059 / RFC-0058 Phase 6a.
//! OWNERS: @ui
//! STATUS: In Progress
//! API_STABILITY: Unstable
//! ADR: docs/rfcs/RFC-0058-ui-v3b-clip-scroll-effects-ime-contract.md

#![no_std]
#![deny(unsafe_code)]

extern crate alloc;

pub mod blur;
pub mod budget;
pub mod cache;
pub mod cursor_blink;
pub mod shadow;

// Re-export Phase 6a blur primitives
pub use blur::{blur_1d, blur_1x3_horizontal, blur_3x3, blur_separable};
pub use shadow::composite_drop_shadow;
