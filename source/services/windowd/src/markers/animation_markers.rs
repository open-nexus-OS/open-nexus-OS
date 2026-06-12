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
// ── TASK-0063 (UI v5b) markers ───────────────────────────────────────
pub const SELFTEST_UI_V5_SCENE_GRAPH_OK: &str = "SELFTEST: ui v5 scene graph ok";
pub const SELFTEST_UI_V5_VIRTUALIZE_OK: &str = "SELFTEST: ui v5 virtualize ok";
pub const SELFTEST_UI_V5_THEME_OK: &str = "SELFTEST: ui v5 theme ok";
pub const UI_VIRTUAL_LIST_ON: &str = "ui: virtual list on";
pub const VIRTUALIZE_MOUNT: &str = "virtualize: mount ok";
pub const VIRTUALIZE_RECYCLE: &str = "virtualize: recycle ok";
pub const VIRTUALIZE_LIVE_SCROLL_OK: &str = "virtualize: live scroll ok";
pub const VIRTUALIZE_PAGE_LOAD_OK: &str = "virtualize: page load ok";
pub const VIRTUALIZE_PREPEND_ANCHOR_OK: &str = "virtualize: prepend anchor ok";
pub const UITHEME_LOADED: &str = "uitheme: loaded (mode=light)";
pub const UITHEME_SWITCHED: &str = "uitheme: switched (to=dark)";
