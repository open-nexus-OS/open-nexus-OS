// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: The per-window compositing STATE + Z-BAND role (moved out of
//! `window_scene.rs`, structure gate): [`WindowRole`] bands the compositor
//! order (desktop < floating < overlay < fullscreen-within-band), and
//! [`WindowState`] carries the visibility/z/minimize bits the
//! [`crate::window_scene::WindowStack`] orders by.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: exercised by the `window_scene` stack/order host tests.

use crate::window_scene::{should_show, WindowId};

/// The compositor Z-BAND a window belongs to (RFC-0065 multi-window). Bands are
/// strictly ordered — a window can never leave its band via z-raise — so the
/// desktop surface stays under every floating window and a fullscreen window
/// stays over them. Within a band, `z` orders.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WindowRole {
    /// The DESKTOP surface: the shell (or greeter) as the base layer, composited
    /// BELOW all floating windows, chromeless + full-screen. It replaces the
    /// in-process shell / wallpaper as the always-present base
    /// ([`BASE_ALWAYS_PRESENT`]) — an app-host owns it (RFC-0065), windowd only
    /// composes it at the bottom band. Never suppressed by `desktop_shell_active`
    /// (it IS the desktop).
    Desktop,
    /// An OVERLAY surface (RFC-0075 Phase 2: the OSK; later: candidate strip,
    /// system banners): composited ABOVE every floating window, chromeless,
    /// never focus-stealing (shown via [`WindowStack::show_unfocused`]).
    Overlay,
    /// A normal floating window (chat/search/settings/app client): chrome, z
    /// within the window band, subject to the shell-takeover show rule.
    Window,
}

/// The composition-relevant state of one window.
#[derive(Clone, Copy, Debug)]
pub struct WindowState {
    /// Which window.
    pub id: WindowId,
    /// The Z-BAND this window lives in (desktop base vs. floating window).
    pub role: WindowRole,
    /// Whether the app/window wants to be shown (open, even if minimized).
    pub visible: bool,
    /// Composite z-order within the band (higher = nearer the viewer).
    pub z: i16,
    /// Minimized: still OPEN (lives in the dock) but not composited.
    /// Orthogonal to `visible` — closed = `visible: false`, minimized =
    /// `visible: true, minimized: true` (restore brings it straight back).
    pub minimized: bool,
    /// Fullscreen: composites ABOVE the chrome (the "□" toggle / a later
    /// top-edge snap). Survives minimize; cleared on close.
    pub fullscreen: bool,
}

impl WindowState {
    /// A floating (non-minimized, non-fullscreen) window state.
    pub fn floating(id: WindowId, visible: bool, z: i16) -> Self {
        Self { id, role: WindowRole::Window, visible, z, minimized: false, fullscreen: false }
    }

    /// A DESKTOP-band window (the shell / greeter background surface): composited
    /// below all floating windows, never fullscreen-banded (it is already the
    /// full-screen base).
    pub fn desktop(id: WindowId, visible: bool) -> Self {
        Self { id, role: WindowRole::Desktop, visible, z: 0, minimized: false, fullscreen: false }
    }

    /// Whether this window composites this frame.
    pub(crate) fn showable(&self, desktop_shell_active: bool) -> bool {
        match self.role {
            // The desktop surface is the base layer — it shows whenever visible,
            // never suppressed by the shell-takeover rule (it IS the shell).
            WindowRole::Desktop => self.visible && !self.minimized,
            WindowRole::Window => {
                should_show(self.visible, desktop_shell_active) && !self.minimized
            }
            // Overlays ride ABOVE the shell-takeover rule (the OSK must show
            // over the greeter and the desktop shell alike).
            WindowRole::Overlay => self.visible && !self.minimized,
        }
    }

    /// Sort key for composition. Strictly banded: DESKTOP below everything, then
    /// floating windows, then FULLSCREEN above all (covers the chrome — nothing
    /// floating may overlap it). `z` orders within a band.
    pub(crate) fn order_key(&self) -> i32 {
        let role_band = match self.role {
            WindowRole::Desktop => -1_000_000,
            WindowRole::Window => 0,
            WindowRole::Overlay => 1_000_000,
        };
        role_band + (self.fullscreen as i32) * 100_000 + self.z as i32
    }
}
