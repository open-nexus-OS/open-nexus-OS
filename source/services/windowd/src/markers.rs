// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Deterministic marker strings and postflight marker gating for `windowd`.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Marker literals and postflight gates covered by `ui_windowd_host` and `ui_v2a_host`
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

use crate::error::{Result, WindowdError};
use crate::ids::SurfaceId;
use crate::server::PresentAck;
use alloc::format;
use alloc::string::String;

pub const READY_MARKER: &str = "windowd: ready (w=64, h=48, hz=60)";
pub const SYSTEMUI_MARKER: &str = "windowd: systemui loaded (profile=desktop)";
pub const LAUNCHER_MARKER: &str = "launcher: first frame ok";
pub const SELFTEST_LAUNCHER_PRESENT_MARKER: &str = "SELFTEST: ui launcher present ok";
pub const SELFTEST_RESIZE_MARKER: &str = "SELFTEST: ui resize ok";
pub const DISPLAY_BOOTSTRAP_MARKER: &str = "display: bootstrap on";
pub const DISPLAY_MODE_MARKER: &str = "display: mode 1280x800 argb8888";
pub const DISPLAY_FIRST_SCANOUT_MARKER: &str = "display: first scanout ok";
pub const SELFTEST_DISPLAY_BOOTSTRAP_VISIBLE_MARKER: &str = "SELFTEST: display bootstrap guest ok";
pub const VISIBLE_BACKEND_MARKER: &str = "windowd: backend=visible";
pub const COMPOSE_READY_MARKER: &str = "windowd: compose ready";
pub const PRESENT_QUEUED_MARKER: &str = "windowd: present queued";
pub const PRESENT_COALESCED_MARKER: &str = "windowd: present coalesced";
pub const PRESENT_VISIBLE_MARKER: &str = "windowd: present visible ok";
pub const SYSTEMUI_FIRST_FRAME_VISIBLE_MARKER: &str = "systemui: first frame visible";
pub const FAIL_COMPOSE_EVIDENCE_MARKER: &str = "windowd: fail compose-evidence";
pub const FAIL_PRESENT_STALL_MARKER: &str = "windowd: fail present-stall";
pub const SELFTEST_UI_VISIBLE_PRESENT_MARKER: &str = "SELFTEST: ui visible present ok";
pub const PRESENT_SCHEDULER_ON_MARKER: &str = "windowd: present scheduler on";
pub const INPUT_ON_MARKER: &str = "windowd: input on";
pub const LAUNCHER_CLICK_OK_MARKER: &str = "launcher: click ok";
pub const SELFTEST_UI_V2_PRESENT_OK_MARKER: &str = "SELFTEST: ui v2 present ok";
pub const SELFTEST_UI_V2_INPUT_OK_MARKER: &str = "SELFTEST: ui v2 input ok";
pub const INPUT_VISIBLE_ON_MARKER: &str = "windowd: input visible on";
pub const FULL_WINDOW_VISIBLE_MARKER: &str = "windowd: full-window color visible";
pub const CURSOR_MOVE_VISIBLE_MARKER: &str = "windowd: cursor move visible";
pub const HOVER_VISIBLE_MARKER: &str = "windowd: hover visible";
pub const FOCUS_VISIBLE_MARKER: &str = "windowd: focus visible";
pub const LAUNCHER_CLICK_VISIBLE_OK_MARKER: &str = "launcher: click visible ok";
pub const KEYBOARD_VISIBLE_MARKER: &str = "windowd: keyboard visible";
pub const SELFTEST_UI_VISIBLE_INPUT_OK_MARKER: &str = "SELFTEST: ui visible input ok";
pub const WHEEL_VISIBLE_MARKER: &str = "windowd: wheel visible";
pub const SELFTEST_UI_VISIBLE_WHEEL_OK_MARKER: &str = "SELFTEST: ui visible wheel ok";
pub const INTERACTIVE_SCENE_READY_MARKER: &str = "windowd: interactive scene ready";
pub const INTERACTIVE_CLICK_TARGET_READY_MARKER: &str = "windowd: interactive click target ready";
pub const INTERACTIVE_KEYBOARD_TARGET_READY_MARKER: &str =
    "windowd: interactive keyboard target ready";
pub const INTERACTIVE_FULL_MARKERS_MARKER: &str = "windowd: interactive full markers on";
pub const PRESENT_FASTPATH_MARKER: &str = "windowd: present fastpath on";
pub const POINTER_COALESCE_OK_MARKER: &str = "windowd: pointer coalesce ok";
pub const NO_DAMAGE_SKIP_OK_MARKER: &str = "windowd: no-damage skip ok";
pub const IDLE_FASTPATH_OK_MARKER: &str = "windowd: idle fastpath ok";
pub const CLICK_LATENCY_OK_MARKER: &str = "windowd: click latency ok";
pub const KEYBOARD_LATENCY_OK_MARKER: &str = "windowd: keyboard latency ok";

pub fn present_marker(ack: PresentAck) -> String {
    format!("windowd: present ok (seq={} dmg={})", ack.seq.raw(), ack.damage_rects)
}

pub fn focus_marker(surface: SurfaceId) -> String {
    format!("windowd: focus -> {}", surface.raw())
}

pub fn damage_rects_marker(rects: u16) -> String {
    format!("windowd: damage rects={rects}")
}

pub fn marker_postflight_ready(evidence: Option<PresentAck>) -> Result<PresentAck> {
    evidence.ok_or(WindowdError::MarkerBeforePresentState)
}
