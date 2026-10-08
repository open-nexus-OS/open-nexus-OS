// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Deterministic marker strings and postflight marker gating for `windowd`.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Marker literals and postflight gates covered by `ui_windowd_host` and `ui_v2a_host`
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

#![allow(dead_code)] // markers used in os-lite cfg, not in host clippy

use crate::error::{Result, WindowdError};
use crate::ids::SurfaceId;
use crate::server::PresentAck;

use alloc::format;
use alloc::string::String;

#[path = "markers/animation_markers.rs"]
pub mod animation_markers;

#[allow(unused_imports)]
pub use animation_markers::*;

/// The ready marker at the canonical 1280×800@120 baseline — the literal the
/// proof ladder asserts (`scripts/qemu-test.sh`). The OS boot prints
/// [`ready_marker`] for the mode it actually resolved; at the baseline the two
/// are byte-identical.
pub const READY_MARKER: &str = "windowd: ready (w=1280, h=800, hz=120)";

/// The ready marker for the RESOLVED visible mode (RFC-0074: the fw_cfg
/// display mode gpud reported), so a dev preset boot (TASK-0055D) reports its
/// real w/h instead of the baseline literal. The refresh rate is the pacer's
/// single constant (`VISIBLE_BOOTSTRAP_HZ` — windowd paces at 120 Hz for
/// every mode; presets naming another rate are rejected host-side).
pub fn ready_marker(w: u32, h: u32) -> alloc::string::String {
    alloc::format!("windowd: ready (w={w}, h={h}, hz={})", crate::server::VISIBLE_BOOTSTRAP_HZ)
}
pub const RUNTIME_INIT_START: &str = "windowd: runtime init start";
pub const RUNTIME_INIT_OK: &str = "windowd: runtime init ok";
pub const WALLPAPER_LOADED: &str = "windowd: wallpaper loaded (jpeg)";
pub const WALLPAPER_FALLBACK: &str = "windowd: wallpaper fallback solid";
pub const WALLPAPER_FAIL: &str = "windowd: wallpaper fail";
pub const SYSTEMUI_MARKER: &str = "windowd: systemui loaded (profile=desktop)";
pub const LAUNCHER_MARKER: &str = "launcher: first frame ok";
pub const SELFTEST_LAUNCHER_PRESENT_MARKER: &str = "SELFTEST: ui launcher present ok";
pub const SELFTEST_RESIZE_MARKER: &str = "SELFTEST: ui resize ok";
pub const DISPLAY_BOOTSTRAP_MARKER: &str = "display: bootstrap on";
/// The display-mode marker at the canonical baseline (ladder literal); the OS
/// boot prints [`display_mode_marker`] for the resolved mode.
pub const DISPLAY_MODE_MARKER: &str = "display: mode 1280x800 argb8888";

/// `display: mode <w>x<h> argb8888` for the RESOLVED visible mode (TASK-0055D);
/// byte-identical to [`DISPLAY_MODE_MARKER`] at 1280×800.
pub fn display_mode_marker(w: u32, h: u32) -> alloc::string::String {
    alloc::format!("display: mode {w}x{h} argb8888")
}
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
pub const SIDEBAR_OPEN_MARKER: &str = "windowd: sidebar open";
pub const SIDEBAR_CLOSE_MARKER: &str = "windowd: sidebar close";
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

// --- TASK-0057 / RFC-0056: UI v2b asset markers ---
/// Fired by windowd when the Mocu SVG cursor asset is successfully loaded.
pub const CURSOR_SVG_LOADED_MARKER: &str = "windowd: cursor svg loaded";
/// Fired by windowd when a text target (shaped glyphs) is visible on the proof surface.
pub const TEXT_TARGET_VISIBLE_MARKER: &str = "windowd: text target visible";
/// Fired by windowd when an SVG icon target is visible on the proof surface.
pub const ICON_TARGET_VISIBLE_MARKER: &str = "windowd: icon target visible";
/// Fired by windowd when the JPEG-sourced wallpaper background is visible.
pub const WALLPAPER_VISIBLE_MARKER: &str = "windowd: wallpaper visible";
/// Observer summary: all v2b asset targets verified.
pub const SELFTEST_UI_V2B_ASSETS_OK_MARKER: &str = "SELFTEST: ui v2b assets ok";

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

// --- TASK-0058 / RFC-0057: UI v3a layout engine markers ---
pub const LAYOUT_ENGINE_ON_MARKER: &str = "layout: engine on";
pub const TEXT_WRAPPING_ON_MARKER: &str = "text: wrapping on";

// --- TASK-0059 / RFC-0058: UI v3b scroll/effects markers (the legacy text-filter and IME
// lines retired 2026-10-08: typed text no longer reaches windowd's visible state) ---
pub const SCROLL_ON_MARKER: &str = "windowd: scroll on";
pub const LIVE_SCROLL_OK_MARKER: &str = "windowd: live scroll ok";
pub const EFFECTS_ON_MARKER: &str = "windowd: effects on";
pub const EFFECT_BLUR_OK_MARKER: &str = "windowd: effect blur ok";
pub const SELFTEST_UI_V3_EFFECT_OK_MARKER: &str = "SELFTEST: ui v3 effect ok";

// --- TASK-0074 / ADR-0068: modal semantics — windowd's ONE routing verb ---
/// `windowd: win modal on (id=N)` / `… off (id=N)`: the app-modal flag's edge.
pub fn win_modal_marker(surface_id: u32, on: bool) -> String {
    format!("windowd: win modal {} (id={surface_id})", if on { "on" } else { "off" })
}
/// A press on a window the gate refused (its owner's modal is `id`).
pub fn press_refused_marker(modal_surface_id: u32) -> String {
    format!("windowd: press refused (modal id={modal_surface_id})")
}
pub const SELFTEST_UI_V10_DIALOG_OK_MARKER: &str = "SELFTEST: ui v10 dialog ok";
pub const SELFTEST_UI_V10_LIVE_MODAL_OK_MARKER: &str = "SELFTEST: ui v10 live modal ok";

// --- TASK-0066: window tiling (halves, quarters, Fill, Return, arrangements) ---
/// `windowd: wm tile (zone=left-half id=app1)` — a window took a zone.
pub fn wm_tile_marker(zone: &str, id: &str) -> String {
    format!("windowd: wm tile (zone={zone} id={id})")
}
/// `windowd: wm return (id=app1)` — back to the pre-tile frame.
pub fn wm_return_marker(id: &str) -> String {
    format!("windowd: wm return (id={id})")
}
/// `windowd: wm arrange (kind=left-right n=2)` — an arrangement tiled `n` windows.
pub fn wm_arrange_marker(kind: &str, n: u8) -> String {
    format!("windowd: wm arrange (kind={kind} n={n})")
}
/// `windowd: wm tile deny (reason=not-resizable)` — a refused tile, by reason.
pub fn wm_tile_deny_marker(reason: &str) -> String {
    format!("windowd: wm tile deny (reason={reason})")
}
/// `windowd: wm chord (zone=left-half id=app1)` — a keyboard chord tiled the focused window.
pub fn wm_chord_marker(zone: &str, id: &str) -> String {
    format!("windowd: wm chord (zone={zone} id={id})")
}
/// The tiling round trip (a tile, then Return) happened on this boot.
pub const SELFTEST_UI_V7_TILE_OK_MARKER: &str = "SELFTEST: ui v7 tile ok";
