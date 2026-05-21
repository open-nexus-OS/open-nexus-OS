// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: CPU effects (blur/drop-shadow/9-slice/kawase) with deterministic budgets and render cache for TASK-0059 / RFC-0058.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 98 tests (tests/ui_v4_host/ — 21+22+23+8+7+15+2)
//! ADR: docs/rfcs/RFC-0058-ui-v3b-clip-scroll-effects-ime-contract.md

#![no_std]
#![deny(unsafe_code)]

extern crate alloc;

pub mod blur;
pub mod budget;
pub mod cache;
pub mod cursor_blink;
pub mod shadow;

pub use blur::{blur_1d, blur_1x3_horizontal, blur_3x3, blur_separable, dual_kawase_blur};
pub use budget::EffectBudget;
pub use cache::{EffectCache, RenderCache, ShadowArena, ShadowCache, TextCache};
pub use shadow::{
    composite_drop_shadow, composite_nine_slice_shadow, DropShadowParams, NineSliceCompositeParams,
    NineSliceShadow,
};
