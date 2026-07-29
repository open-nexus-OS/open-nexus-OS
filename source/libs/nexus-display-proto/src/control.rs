// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `OP_SURFACE_CONTROL` control kinds — the app→windowd window-action
//! vocabulary (minimize/close/mode/move + the launch-pending hint). Split out
//! of `client_surface.rs` (at its size baseline) when `CONTROL_WIN_MOVE`
//! joined; re-exported from there, so the wire path is unchanged.
//! OWNERS: @ui @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `client_surface` control encode/decode tests

/// Control kinds. 0/1/3 are RETIRED (RFC-0083 P5, values reserved forever):
/// theme, shell profile and accent are settingsd keys now, not compositor
/// controls — windowd answers an old sender like any unknown control.
pub const CONTROL_THEME: u8 = 0;
pub const CONTROL_SHELL_PROFILE: u8 = 1;
/// A shell-initiated app launch is pending: the compositor shows the wait
/// cursor (loading ring) until the fresh window's surface arrives (value
/// unused). Fire-and-forget hint — losing it only skips the ring.
pub const CONTROL_LAUNCH_PENDING: u8 = 2;
pub const CONTROL_THEME_ACCENT: u8 = 3;
/// App-chrome window controls (the app-icon dropdown of a client-chrome
/// window): windowd applies the action to the SENDING client's window.
/// value unused for minimize/close; for mode, value = `WIN_MODE_*`.
pub const CONTROL_WIN_MINIMIZE: u8 = 4;
pub const CONTROL_WIN_CLOSE: u8 = 5;
pub const CONTROL_WIN_MODE: u8 = 6;
/// Begin a pointer MOVE drag of the sender's window at the CURRENT cursor
/// (value = surface id) — how a CHROMELESS (`style: plain`) window is dragged:
/// its own chrome row is the grab strip and sends this on press; windowd
/// raises + tracks the pointer to release (same clamp/snap path as a WM title
/// drag). Fire-and-forget; ignored for fullscreen windows.
pub const CONTROL_WIN_MOVE: u8 = 7;
