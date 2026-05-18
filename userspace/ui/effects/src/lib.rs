// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: CPU effects (blur/drop-shadow) with deterministic budgets for TASK-0059 / RFC-0058.
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
