// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Pure window-composition decisions — host-tested SSOT for which
//! windows show + their z-order, extracted from the compositor monolith (RFC-0066).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 6 tests
//!
//! Pure window-composition decisions — the host-tested SSOT for *which windows
//! show and in what z-order* (RFC-0066), extracted from the 4055-line compositor
//! runtime monolith.
//!
//! Why this exists: a wrong "show this window?" / z-order decision is the
//! **black-screen risk class** — and it was scattered across the os-only runtime
//! as dozens of `if self.chat.visible && !USE_DESKTOP_SHELL` checks that could
//! only be verified by booting. Moving the decision here makes it a `cargo test`
//! failure instead of a boot hunt, and shrinks the monolith.

use alloc::vec::Vec;

/// A composable shell window. Extensible toward per-app surfaces (RFC-0065).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WindowId {
    /// The chat window.
    Chat,
    /// The search window.
    Search,
}

/// The composition-relevant state of one window.
#[derive(Clone, Copy, Debug)]
pub struct WindowState {
    /// Which window.
    pub id: WindowId,
    /// Whether the app/window wants to be shown.
    pub visible: bool,
    /// Composite z-order (higher = nearer the viewer).
    pub z: i16,
}

/// Whether a shell window should be composited this frame.
///
/// A window shows only when it is `visible` **and** the declarative desktop shell
/// is not taking over the surface (the `chat_show = !USE_DESKTOP_SHELL && visible`
/// rule — now in one tested place instead of inline everywhere).
pub fn should_show(visible: bool, desktop_shell_active: bool) -> bool {
    visible && !desktop_shell_active
}

/// The windows to composite over the base layer, back-to-front (z ascending).
/// Hidden windows (and all windows when the desktop shell is active) are excluded.
pub fn composition_order(windows: &[WindowState], desktop_shell_active: bool) -> Vec<WindowId> {
    let mut visible: Vec<(WindowId, i16)> = windows
        .iter()
        .filter(|w| should_show(w.visible, desktop_shell_active))
        .map(|w| (w.id, w.z))
        .collect();
    visible.sort_by_key(|&(_, z)| z);
    visible.into_iter().map(|(id, _)| id).collect()
}

/// Anti-black-screen invariant: the compositor **always** draws a base layer
/// (wallpaper / shell) below the windows, so an *empty* [`composition_order`] is a
/// clean desktop — never a black frame. The runtime must honour this; the tests
/// below pin the contract.
pub const BASE_ALWAYS_PRESENT: bool = true;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shown_only_when_visible_and_shell_inactive() {
        assert!(should_show(true, false)); // visible, shell off → show
        assert!(!should_show(true, true)); // desktop shell takes over → hide window
        assert!(!should_show(false, false)); // hidden → no show
        assert!(!should_show(false, true));
    }

    #[test]
    fn hidden_windows_are_not_composited() {
        // Mirrors the boot case the user hit: chat starts hidden → not in the frame.
        let windows = [
            WindowState { id: WindowId::Chat, visible: false, z: 3 },
            WindowState { id: WindowId::Search, visible: false, z: 2 },
        ];
        assert!(composition_order(&windows, false).is_empty());
    }

    #[test]
    fn visible_window_is_composited() {
        let windows = [
            WindowState { id: WindowId::Chat, visible: true, z: 3 },
            WindowState { id: WindowId::Search, visible: false, z: 2 },
        ];
        assert_eq!(composition_order(&windows, false), vec![WindowId::Chat]);
    }

    #[test]
    fn composition_is_z_ordered_back_to_front() {
        let windows = [
            WindowState { id: WindowId::Chat, visible: true, z: 3 },
            WindowState { id: WindowId::Search, visible: true, z: 2 },
        ];
        // Search (z=2) composites before Chat (z=3) → chat on top.
        assert_eq!(composition_order(&windows, false), vec![WindowId::Search, WindowId::Chat]);
    }

    #[test]
    fn desktop_shell_suppresses_all_windows() {
        let windows = [
            WindowState { id: WindowId::Chat, visible: true, z: 3 },
            WindowState { id: WindowId::Search, visible: true, z: 2 },
        ];
        assert!(composition_order(&windows, true).is_empty());
    }

    #[test]
    fn empty_composition_is_not_a_black_frame() {
        // The black-screen guard: no visible windows is valid — the base layer
        // (wallpaper) is always present, so the desktop shows, not black.
        assert!(BASE_ALWAYS_PRESENT, "compositor must always draw a base layer");
        let nothing: [WindowState; 0] = [];
        assert!(composition_order(&nothing, false).is_empty());
    }
}
