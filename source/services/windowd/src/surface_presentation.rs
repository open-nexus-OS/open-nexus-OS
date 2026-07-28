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
    #[allow(dead_code)]
    // declared policy vocabulary (intent ⟂ policy model); product wiring pending
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
    /// OVERLAY surfaces dock to the bottom display edge (RFC-0075 Phase 2:
    /// the OSK band) — full display width, WM-owned height, above all
    /// floating windows, never focused.
    pub docked_bottom: bool,
}

impl WindowPresentation {
    /// Resolve the declared intent (`style`/`level`/`mode`/`resizable`, as carried
    /// on `OP_SURFACE_INTENT`) against the environment `policy` into the concrete
    /// compositing properties. The ONE SSOT — no other windowd code re-derives
    /// "is this the desktop / does it have chrome".
    #[must_use]
    pub fn resolve(
        style: u8,
        level: u8,
        mode: u8,
        resizable: bool,
        policy: WindowingPolicy,
    ) -> Self {
        let is_desktop = level == wire::WIN_LEVEL_DESKTOP;
        let is_overlay = level == wire::WIN_LEVEL_OVERLAY;
        let is_fullscreen = mode == wire::WIN_MODE_FULLSCREEN;
        // The desktop base is inherently full-screen; a fullscreen-mode window
        // covers the display too.
        let full_screen = is_desktop || is_fullscreen;

        // Z-band from the declared level (wlr-layer-shell style): desktop → base
        // band, everything else → the floating window band. (OVERLAY bands above
        // — a follow-up when the overlay role exists; today it floats.)
        let role = if is_desktop {
            WindowRole::Desktop
        } else if is_overlay {
            WindowRole::Overlay
        } else {
            WindowRole::Window
        };

        // Chrome = intent ⟂ policy. The app drops it by declaring `plain`, or
        // implicitly for a desktop/fullscreen surface; a kiosk policy drops it
        // unconditionally (single-app OS = no window controls).
        let intent_chromeless = style == wire::WIN_STYLE_PLAIN || full_screen || is_overlay;
        let has_chrome = !intent_chromeless && !matches!(policy, WindowingPolicy::Kiosk);

        // Resize = intent ⟂ policy. Only a freeform, non-fullscreen, non-desktop
        // surface under a resize-permitting policy is user-resizable.
        let resizable =
            resizable && !full_screen && !is_overlay && !matches!(policy, WindowingPolicy::Kiosk);

        Self { role, has_chrome, full_screen, resizable, docked_bottom: is_overlay }
    }
}

/// Where a floating window's frame may sit, given the work area.
///
/// A new window inherits its slot's CASCADE origin — picked so several windows
/// never stack pixel-exactly, and picked before anyone knows how big the window
/// will be. Once a window can ask the compositor for a real default frame
/// (three quarters of the display), that origin puts a large frame partly off
/// screen: the file manager's 960-wide window at cascade x=300 lost its whole
/// properties pane past the right edge. Clamping keeps the cascade useful
/// without letting it push content out of view.
///
/// A frame larger than the work area pins to the origin (`saturating_sub` ⇒ 0),
/// which is the only sensible answer when nothing fits.
#[must_use]
pub fn clamp_frame_origin(
    (x, y): (i32, i32),
    (w, h): (u32, u32),
    (area_w, area_h): (u32, u32),
) -> (i32, i32) {
    let max_x = i64::from(area_w.saturating_sub(w)).min(i64::from(i32::MAX)) as i32;
    let max_y = i64::from(area_h.saturating_sub(h)).min(i64::from(i32::MAX)) as i32;
    (x.clamp(0, max_x), y.clamp(0, max_y))
}

#[cfg(test)]
mod frame_tests {
    use super::clamp_frame_origin;

    #[test]
    fn a_frame_that_fits_keeps_its_cascade_origin() {
        assert_eq!(clamp_frame_origin((300, 140), (400, 300), (1280, 760)), (300, 140));
    }

    /// The defect. The cascade steps 64px per slot (300, 364, 428, …), so the
    /// FIRST window's 960-wide default frame just fits a 1280 display (300+960
    /// = 1260) and the SECOND one hangs 44px off the right edge — which is how
    /// the file manager lost the right side of its window on the second launch.
    /// The clamp slides it back to 320 instead of letting it run off.
    #[test]
    fn a_cascaded_wide_frame_slides_back_onto_the_display() {
        assert_eq!(clamp_frame_origin((300, 140), (960, 570), (1280, 760)), (300, 140));
        assert_eq!(clamp_frame_origin((364, 196), (960, 570), (1280, 760)), (320, 190));
    }

    #[test]
    fn a_frame_larger_than_the_work_area_pins_to_the_origin() {
        assert_eq!(clamp_frame_origin((300, 140), (1600, 900), (1280, 760)), (0, 0));
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

// `ShellWindow` only exists in the OS build; the POLICY above is host-tested
// either way (`frame_tests`), which is the half worth proving.
#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
impl crate::compositor::shell_window::ShellWindow {
    /// `set_frame` at the window's CURRENT origin, slid back onto `area` by
    /// [`clamp_frame_origin`]. Lives here, with the placement policy, rather
    /// than in the legacy `shell_window` module that is being retired.
    pub(crate) fn set_frame_clamped(&mut self, w: u32, h: u32, area: (u32, u32)) {
        let (x, y) = clamp_frame_origin((self.x, self.y), (w, h), area);
        self.set_frame(x, y, w, h);
    }
}
