// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! QEMU hop markers for animation integration chain verification.
//! Format: `hop:<from>→<to>: <event>` for chain debugging.

pub const UIRUNTIME_ON: &str = "uiruntime: on";
pub const UIRUNTIME_BATCH_COMMIT_OK: &str = "uiruntime: batch commit ok";
pub const UIANIM_TIMELINE_ON: &str = "uianim: timeline on";
pub const UIANIM_SPRING_CONVERGE_OK: &str = "uianim: spring converge ok";
pub const WINDOWD_IMPLICIT_TRANSITIONS_ON: &str = "windowd: implicit transitions on";
pub const WINDOWD_LIVE_TRANSITION_OK: &str = "windowd: live transition ok";
pub const SELFTEST_GPU_CURSOR_MOVE_OK: &str = "SELFTEST: gpu cursor move ok";
pub const SELFTEST_GPU_SCANOUT_FLIP_OK: &str = "SELFTEST: gpu scanout flip ok";
pub const SELFTEST_UI_V5_TRANSITION_OK: &str = "SELFTEST: ui v5 transition ok";
