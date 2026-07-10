// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The DECLARATIVE window-presentation SSOT (RFC-0065 / Umbau #17).
//! windowd is a pure compositor SERVICE — it must NOT know "this surface is the
//! desktop / a chat window / an app". Each client surface DECLARES its intent
//! (`OP_SURFACE_INTENT`: style/level/mode/resizable); the shell environment
//! supplies a windowing POLICY (the shell profile). This module is the ONE
//! host-tested place that resolves `intent ⟂ policy` into the concrete
//! compositing properties windowd acts on — so there is NO per-window-type
//! branch anywhere else (no `WindowId::Desktop`, no scattered `is_desktop` /
//! `title_h` checks). This mirrors `wlr-layer-shell`: the client declares its
//! layer/role, the compositor honours it.
//!
//! Boundary: this is POLICY RESOLUTION (allowed in windowd's scene-assembly), not
//! rasterization (nexus-gfx) or chrome drawing (the `window` widget). It only
//! decides *what* to compose; *how* stays in nexus-gfx / gpud / the widget.
//!
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: host unit tests below (pure intent × policy → presentation)

use crate::window_scene::WindowRole;
use nexus_display_proto::client_surface as wire;

/// The environment's windowing POLICY — the `⟂` axis (the shell profile the
/// product selects). Policy can only ever TIGHTEN a surface's intent (drop
/// chrome / disable resize), never loosen it: an app cannot force chrome onto a
/// kiosk, and a kiosk cannot be talked into a title bar.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WindowingPolicy {
    /// Full desktop windowing: honour the app's declared intent as-is (chrome,
    /// resize, window levels).
    Desktop,
    /// Single-app / kiosk: the app owns the whole screen with NO chrome and NO
    /// resize (a launcher or a single-app OS — the user's explicit requirement:
    /// "eine app als launcher für ein single app os" gets no close/minimize).
    Kiosk,
    // Tablet / TV profiles land with the shell-profile work; until then a product
    // is either Desktop or Kiosk. `Desktop` is the default.
}

/// The resolved compositing properties for ONE surface — exactly what windowd
/// composes. Derived purely from declared intent `⟂` policy; carries no window
/// identity. windowd reads these instead of matching on a fixed window kind.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WindowPresentation {
    /// Z-BAND role: the desktop base (shell/greeter, composited at the bottom)
    /// vs. a floating window. Derived from the declared `level`.
    pub role: WindowRole,
    /// Whether windowd draws a title bar (chrome). Dropped for plain / desktop /
    /// fullscreen surfaces by intent, and ALWAYS dropped under a kiosk policy.
    pub has_chrome: bool,
    /// Whether the surface covers the whole display (the desktop base, or a
    /// fullscreen-mode window). Such a surface is composed edge-to-edge, no
    /// rounded corners / shadow.
    pub full_screen: bool,
    /// Whether the user may resize it: freeform intent, and only under a policy
    /// that permits it (never a kiosk, never a full-screen/desktop surface).
    pub resizable: bool,
}

impl WindowPresentation {
    /// Resolve the declared intent (`style`/`level`/`mode`/`resizable`, as carried
    /// on `OP_SURFACE_INTENT`) against the environment `policy` into the concrete
    /// compositing properties. The ONE SSOT — no other windowd code re-derives
    /// "is this the desktop / does it have chrome".
    #[must_use]
    pub fn resolve(style: u8, level: u8, mode: u8, resizable: bool, policy: WindowingPolicy) -> Self {
        let is_desktop = level == wire::WIN_LEVEL_DESKTOP;
        let is_fullscreen = mode == wire::WIN_MODE_FULLSCREEN;
        // The desktop base is inherently full-screen; a fullscreen-mode window
        // covers the display too.
        let full_screen = is_desktop || is_fullscreen;

        // Z-band from the declared level (wlr-layer-shell style): desktop → base
        // band, everything else → the floating window band. (OVERLAY bands above
        // — a follow-up when the overlay role exists; today it floats.)
        let role = if is_desktop { WindowRole::Desktop } else { WindowRole::Window };

        // Chrome = intent ⟂ policy. The app drops it by declaring `plain`, or
        // implicitly for a desktop/fullscreen surface; a kiosk policy drops it
        // unconditionally (single-app OS = no window controls).
        let intent_chromeless = style == wire::WIN_STYLE_PLAIN || full_screen;
        let has_chrome = !intent_chromeless && !matches!(policy, WindowingPolicy::Kiosk);

        // Resize = intent ⟂ policy. Only a freeform, non-fullscreen, non-desktop
        // surface under a resize-permitting policy is user-resizable.
        let resizable = resizable && !full_screen && !matches!(policy, WindowingPolicy::Kiosk);

        Self { role, has_chrome, full_screen, resizable }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_level_resolves_to_the_base_band_chromeless_fullscreen() {
        // The shell/greeter declare `Window { style: plain, level: desktop }` —
        // the desktop LEVEL alone implies full-screen (mode stays `auto`;
        // `mode: fullscreen` is a floating-window intent, e.g. a kiosk app).
        let p = WindowPresentation::resolve(
            wire::WIN_STYLE_PLAIN,
            wire::WIN_LEVEL_DESKTOP,
            wire::WIN_MODE_AUTO,
            false,
            WindowingPolicy::Desktop,
        );
        assert_eq!(p.role, WindowRole::Desktop);
        assert!(!p.has_chrome, "the desktop base has no title bar");
        assert!(p.full_screen);
        assert!(!p.resizable);
        // Declaring BOTH (legacy pages did) resolves identically — the combo
        // stays valid, just redundant.
        let both = WindowPresentation::resolve(
            wire::WIN_STYLE_PLAIN,
            wire::WIN_LEVEL_DESKTOP,
            wire::WIN_MODE_FULLSCREEN,
            false,
            WindowingPolicy::Desktop,
        );
        assert_eq!(both, p);
    }

    #[test]
    fn normal_titlebar_window_floats_with_chrome_and_resize() {
        // The counter declares `Window { style: titlebar, level: normal }`.
        let p = WindowPresentation::resolve(
            wire::WIN_STYLE_TITLEBAR,
            wire::WIN_LEVEL_NORMAL,
            wire::WIN_MODE_AUTO,
            true,
            WindowingPolicy::Desktop,
        );
        assert_eq!(p.role, WindowRole::Window);
        assert!(p.has_chrome);
        assert!(!p.full_screen);
        assert!(p.resizable);
    }

    #[test]
    fn plain_style_drops_chrome_but_still_floats() {
        let p = WindowPresentation::resolve(
            wire::WIN_STYLE_PLAIN,
            wire::WIN_LEVEL_NORMAL,
            wire::WIN_MODE_AUTO,
            true,
            WindowingPolicy::Desktop,
        );
        assert_eq!(p.role, WindowRole::Window);
        assert!(!p.has_chrome, "plain surfaces have no title bar");
        assert!(!p.full_screen);
    }

    #[test]
    fn fullscreen_mode_covers_display_and_drops_chrome_and_resize() {
        let p = WindowPresentation::resolve(
            wire::WIN_STYLE_TITLEBAR,
            wire::WIN_LEVEL_NORMAL,
            wire::WIN_MODE_FULLSCREEN,
            true,
            WindowingPolicy::Desktop,
        );
        assert!(p.full_screen);
        assert!(!p.has_chrome, "a fullscreen window covers the chrome");
        assert!(!p.resizable, "a fullscreen window is not user-resizable");
        assert_eq!(p.role, WindowRole::Window, "fullscreen is a MODE, not the desktop LEVEL");
    }

    #[test]
    fn kiosk_policy_forces_chromeless_and_non_resizable_regardless_of_intent() {
        // A single-app-OS launcher: even a titlebar/normal/resizable intent gets
        // NO window controls and NO resize under kiosk (intent ⟂ policy: policy
        // can only tighten).
        let p = WindowPresentation::resolve(
            wire::WIN_STYLE_TITLEBAR,
            wire::WIN_LEVEL_NORMAL,
            wire::WIN_MODE_AUTO,
            true,
            WindowingPolicy::Kiosk,
        );
        assert!(!p.has_chrome, "kiosk = no close/minimize (user's single-app-OS requirement)");
        assert!(!p.resizable, "kiosk = no resize");
        assert_eq!(p.role, WindowRole::Window);
    }
}
