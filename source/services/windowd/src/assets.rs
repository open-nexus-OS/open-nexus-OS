// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

mod generated {
    include!(concat!(env!("OUT_DIR"), "/windowd_generated_assets.rs"));
}

/// Embedded Mocu cursor SVG, normalized from `resources/cursors/mocu/src/svg/default.svg`.
pub const CURSOR_LEFT_PTR_SVG: &str = generated::MOCU_CURSOR_LEFT_PTR_SVG;
pub const CURSOR_HOTSPOT_X: i32 = generated::MOCU_CURSOR_HOTSPOT_X;
pub const CURSOR_HOTSPOT_Y: i32 = generated::MOCU_CURSOR_HOTSPOT_Y;

/// Inter-rendered proof text overlay, generated from
/// `resources/fonts/inter/docs/font-files/InterVariable.ttf`.
pub const PROOF_TEXT_WIDTH: u32 = generated::PROOF_TEXT_WIDTH;
pub const PROOF_TEXT_HEIGHT: u32 = generated::PROOF_TEXT_HEIGHT;
pub const PROOF_TEXT_BGRA: &[u8] = generated::PROOF_TEXT_BGRA;
