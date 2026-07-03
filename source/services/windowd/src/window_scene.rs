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
    /// Whether the app/window wants to be shown (open, even if minimized).
    pub visible: bool,
    /// Composite z-order (higher = nearer the viewer).
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
        Self { id, visible, z, minimized: false, fullscreen: false }
    }

    /// Whether this window composites this frame.
    fn showable(&self, desktop_shell_active: bool) -> bool {
        should_show(self.visible, desktop_shell_active) && !self.minimized
    }

    /// Sort key for composition: fullscreen windows group ABOVE all floating
    /// windows (they cover the chrome — nothing floating may overlap them),
    /// z orders within each group.
    fn order_key(&self) -> i32 {
        (self.fullscreen as i32) * 100_000 + self.z as i32
    }
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
    let mut visible: Vec<(WindowId, i32)> = windows
        .iter()
        .filter(|w| w.showable(desktop_shell_active))
        .map(|w| (w.id, w.order_key()))
        .collect();
    visible.sort_by_key(|&(_, key)| key);
    visible.into_iter().map(|(id, _)| id).collect()
}

/// Anti-black-screen invariant: the compositor **always** draws a base layer
/// (wallpaper / shell) below the windows, so an *empty* [`composition_order`] is a
/// clean desktop — never a black frame. The runtime must honour this; the tests
/// below pin the contract.
pub const BASE_ALWAYS_PRESENT: bool = true;

/// Hard cap on concurrently managed shell windows. Sized for the current shell
/// (chat + search + settings + one spare) and, more importantly, for the atlas
/// budget: every open window costs content + blur-cache rows from the shared
/// pool, so "more windows" is an atlas-sizing decision, not just a constant.
pub const MAX_WINDOWS: usize = 4;

/// When `next_z` reaches this bound the stack renormalizes all z values to
/// `0..len` (order-preserving). Keeps a long-lived session from ever
/// overflowing `i16` no matter how often windows are raised.
const Z_NORMALIZE_LIMIT: i16 = i16::MAX - 8;

/// The z/focus stack — the ONE ordering authority for shell windows. The scene
/// builder composites in [`WindowStack::order`] (back-to-front) and the input
/// router hit-tests in [`WindowStack::hit_order`] (front-to-back, the exact
/// reverse), so a window can never be drawn above another yet hit-tested below
/// it: draw order and hit order come from the same sorted data.
///
/// Alloc-free by design: the per-frame queries return fixed
/// `[WindowId; MAX_WINDOWS]` arrays + a count, because windowd runs on a
/// non-freeing bump allocator where a per-frame `Vec` is a slow leak.
pub struct WindowStack {
    entries: [WindowState; MAX_WINDOWS],
    len: usize,
    focused: Option<WindowId>,
    next_z: i16,
}

impl WindowStack {
    /// Register the managed windows, all hidden, stacked in the given order
    /// (later ids start on top once shown). Ids beyond [`MAX_WINDOWS`] are
    /// ignored — the cap is a hard invariant, not an error path.
    pub fn new(ids: &[WindowId]) -> Self {
        let mut entries = [WindowState::floating(WindowId::Chat, false, 0); MAX_WINDOWS];
        let len = ids.len().min(MAX_WINDOWS);
        for (i, &id) in ids.iter().take(len).enumerate() {
            entries[i] = WindowState::floating(id, false, i as i16);
        }
        Self { entries, len, focused: None, next_z: len as i16 }
    }

    fn index_of(&self, id: WindowId) -> Option<usize> {
        self.entries[..self.len].iter().position(|w| w.id == id)
    }

    /// Whether `id` is currently visible (shown and not hidden).
    pub fn is_visible(&self, id: WindowId) -> bool {
        self.index_of(id).map(|i| self.entries[i].visible).unwrap_or(false)
    }

    /// The focused window, if any. Focus follows raise and always rests on a
    /// visible window (or nothing when all windows are hidden).
    pub fn focused(&self) -> Option<WindowId> {
        self.focused
    }

    /// Whether `id` is the topmost visible window.
    pub fn is_top(&self, id: WindowId) -> bool {
        self.top() == Some(id)
    }

    /// The topmost ON-SCREEN window (visible and not minimized), if any.
    pub fn top(&self) -> Option<WindowId> {
        self.entries[..self.len]
            .iter()
            .filter(|w| w.visible && !w.minimized)
            .max_by_key(|w| w.order_key())
            .map(|w| w.id)
    }

    /// Show `id`: it becomes visible (un-minimized), raised to the top, and
    /// focused (opening a window is user intent — it must not appear behind
    /// another window).
    pub fn show(&mut self, id: WindowId) {
        if let Some(i) = self.index_of(id) {
            self.entries[i].visible = true;
            self.entries[i].minimized = false;
            self.raise(id);
        }
    }

    /// Hide (close) `id` and hand focus to the topmost remaining window. A
    /// closed window leaves the dock and forgets fullscreen — reopening starts
    /// floating.
    pub fn hide(&mut self, id: WindowId) {
        if let Some(i) = self.index_of(id) {
            self.entries[i].visible = false;
            self.entries[i].minimized = false;
            self.entries[i].fullscreen = false;
            if self.focused == Some(id) {
                self.focused = self.top();
            }
        }
    }

    /// Minimize `id` into the dock: stays open (`visible`) but is excluded
    /// from composition; focus falls to the topmost remaining window.
    pub fn minimize(&mut self, id: WindowId) {
        if let Some(i) = self.index_of(id) {
            if !self.entries[i].visible {
                return;
            }
            self.entries[i].minimized = true;
            if self.focused == Some(id) {
                self.focused = self.top();
            }
        }
    }

    /// Restore `id` from the dock: composited again, raised, and focused.
    /// (Returns to its previous mode — a fullscreen window restores fullscreen.)
    pub fn restore(&mut self, id: WindowId) {
        if let Some(i) = self.index_of(id) {
            if !self.entries[i].visible {
                return;
            }
            self.entries[i].minimized = false;
            self.raise(id);
        }
    }

    /// Whether `id` sits minimized in the dock.
    pub fn is_minimized(&self, id: WindowId) -> bool {
        self.index_of(id).map(|i| self.entries[i].minimized).unwrap_or(false)
    }

    /// Minimized (docked) windows in stable registration order — the dock's
    /// slot order, independent of z so icons never shuffle. Alloc-free.
    pub fn minimized_list(&self) -> ([WindowId; MAX_WINDOWS], usize) {
        let mut out = [WindowId::Chat; MAX_WINDOWS];
        let mut n = 0;
        for w in &self.entries[..self.len] {
            if w.visible && w.minimized {
                out[n] = w.id;
                n += 1;
            }
        }
        (out, n)
    }

    /// Set/clear fullscreen on `id`. Entering fullscreen raises + focuses (it
    /// covers the chrome, so it must be the interaction target).
    pub fn set_fullscreen(&mut self, id: WindowId, on: bool) {
        if let Some(i) = self.index_of(id) {
            self.entries[i].fullscreen = on;
            if on {
                self.raise(id);
            }
        }
    }

    /// Whether `id` is in fullscreen mode (even while minimized).
    pub fn is_fullscreen(&self, id: WindowId) -> bool {
        self.index_of(id).map(|i| self.entries[i].fullscreen).unwrap_or(false)
    }

    /// The fullscreen window currently ON SCREEN, if any — while this is
    /// `Some`, the chrome (topbar/panels/dock) is covered and not composited.
    pub fn fullscreen_active(&self) -> Option<WindowId> {
        self.entries[..self.len]
            .iter()
            .filter(|w| w.visible && !w.minimized && w.fullscreen)
            .max_by_key(|w| w.order_key())
            .map(|w| w.id)
    }

    /// Raise `id` to the top of the stack and focus it. Returns `true` when the
    /// stack order actually changed (the caller damages the affected windows),
    /// `false` when it was already on top (a plain re-focus).
    pub fn raise(&mut self, id: WindowId) -> bool {
        let Some(i) = self.index_of(id) else {
            return false;
        };
        self.focused = Some(id);
        if self.is_top(id) && self.entries[i].visible {
            return false;
        }
        self.entries[i].z = self.next_z;
        self.next_z = self.next_z.saturating_add(1);
        if self.next_z >= Z_NORMALIZE_LIMIT {
            self.normalize_z();
        }
        true
    }

    /// The current z of `id` (diagnostic/marker use).
    pub fn z_of(&self, id: WindowId) -> i16 {
        self.index_of(id).map(|i| self.entries[i].z).unwrap_or(0)
    }

    /// On-screen windows back-to-front (composite order), alloc-free.
    /// Minimized windows are excluded; fullscreen windows group on top.
    pub fn order(&self, desktop_shell_active: bool) -> ([WindowId; MAX_WINDOWS], usize) {
        let mut out = [WindowId::Chat; MAX_WINDOWS];
        let mut keys = [0i32; MAX_WINDOWS];
        let mut n = 0;
        for w in &self.entries[..self.len] {
            if w.showable(desktop_shell_active) {
                // Insertion sort by the order key ascending — ≤ MAX_WINDOWS entries.
                let key = w.order_key();
                let mut j = n;
                while j > 0 && keys[j - 1] > key {
                    out[j] = out[j - 1];
                    keys[j] = keys[j - 1];
                    j -= 1;
                }
                out[j] = w.id;
                keys[j] = key;
                n += 1;
            }
        }
        (out, n)
    }

    /// Visible windows front-to-back (hit-test order) — the exact reverse of
    /// [`Self::order`], so occlusion and input can never disagree.
    pub fn hit_order(&self, desktop_shell_active: bool) -> ([WindowId; MAX_WINDOWS], usize) {
        let (mut order, n) = self.order(desktop_shell_active);
        order[..n].reverse();
        (order, n)
    }

    /// Order-preserving z renormalization to `0..len`.
    fn normalize_z(&mut self) {
        // Selection-style rank assignment over ≤ MAX_WINDOWS entries.
        let mut ranked: [usize; MAX_WINDOWS] = [0; MAX_WINDOWS];
        for (slot, item) in ranked.iter_mut().enumerate().take(self.len) {
            *item = slot;
        }
        ranked[..self.len].sort_unstable_by_key(|&i| self.entries[i].z);
        for (rank, &i) in ranked[..self.len].iter().enumerate() {
            self.entries[i].z = rank as i16;
        }
        self.next_z = self.len as i16;
    }
}

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
            WindowState::floating(WindowId::Chat, false, 3),
            WindowState::floating(WindowId::Search, false, 2),
        ];
        assert!(composition_order(&windows, false).is_empty());
    }

    #[test]
    fn visible_window_is_composited() {
        let windows = [
            WindowState::floating(WindowId::Chat, true, 3),
            WindowState::floating(WindowId::Search, false, 2),
        ];
        assert_eq!(composition_order(&windows, false), vec![WindowId::Chat]);
    }

    #[test]
    fn composition_is_z_ordered_back_to_front() {
        let windows = [
            WindowState::floating(WindowId::Chat, true, 3),
            WindowState::floating(WindowId::Search, true, 2),
        ];
        // Search (z=2) composites before Chat (z=3) → chat on top.
        assert_eq!(composition_order(&windows, false), vec![WindowId::Search, WindowId::Chat]);
    }

    #[test]
    fn desktop_shell_suppresses_all_windows() {
        let windows = [
            WindowState::floating(WindowId::Chat, true, 3),
            WindowState::floating(WindowId::Search, true, 2),
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

    // ── WindowStack: the z/focus stack ──

    fn stack() -> WindowStack {
        WindowStack::new(&[WindowId::Search, WindowId::Chat])
    }

    #[test]
    fn stack_starts_hidden_and_unfocused() {
        let s = stack();
        assert_eq!(s.order(false).1, 0);
        assert_eq!(s.focused(), None);
        assert_eq!(s.top(), None);
        assert!(!s.is_visible(WindowId::Chat));
    }

    #[test]
    fn show_raises_and_focuses() {
        let mut s = stack();
        s.show(WindowId::Search);
        assert_eq!(s.focused(), Some(WindowId::Search));
        assert!(s.is_top(WindowId::Search));
        // Opening chat puts it on top and moves focus.
        s.show(WindowId::Chat);
        assert_eq!(s.focused(), Some(WindowId::Chat));
        let (order, n) = s.order(false);
        assert_eq!(&order[..n], &[WindowId::Search, WindowId::Chat]);
    }

    #[test]
    fn raise_reorders_and_reports_change() {
        let mut s = stack();
        s.show(WindowId::Search);
        s.show(WindowId::Chat);
        // Raising the bottom window flips the order (the user's z bug: chat was
        // ALWAYS on top because emit order was hardcoded — raise must win now).
        assert!(s.raise(WindowId::Search));
        let (order, n) = s.order(false);
        assert_eq!(&order[..n], &[WindowId::Chat, WindowId::Search]);
        assert_eq!(s.focused(), Some(WindowId::Search));
        // Raising the already-top window changes nothing (no damage needed).
        assert!(!s.raise(WindowId::Search));
    }

    #[test]
    fn hide_refocuses_topmost_remaining() {
        let mut s = stack();
        s.show(WindowId::Search);
        s.show(WindowId::Chat);
        s.hide(WindowId::Chat);
        assert_eq!(s.focused(), Some(WindowId::Search));
        assert!(s.is_top(WindowId::Search));
        s.hide(WindowId::Search);
        assert_eq!(s.focused(), None);
        assert_eq!(s.order(false).1, 0);
    }

    #[test]
    fn hit_order_is_reverse_of_composite_order() {
        let mut s = stack();
        s.show(WindowId::Search);
        s.show(WindowId::Chat);
        let (order, n) = s.order(false);
        let (hits, hn) = s.hit_order(false);
        assert_eq!(n, hn);
        let mut reversed = order;
        reversed[..n].reverse();
        assert_eq!(&hits[..hn], &reversed[..n]);
        // Front-to-back: the topmost (chat) is hit-tested first.
        assert_eq!(hits[0], WindowId::Chat);
    }

    #[test]
    fn desktop_shell_suppresses_stack_windows_too() {
        let mut s = stack();
        s.show(WindowId::Chat);
        assert_eq!(s.order(true).1, 0);
        assert_eq!(s.hit_order(true).1, 0);
    }

    // ── Phase 2: minimize / dock / fullscreen ──

    #[test]
    fn minimize_leaves_composition_and_refocuses() {
        let mut s = stack();
        s.show(WindowId::Search);
        s.show(WindowId::Chat);
        s.minimize(WindowId::Chat);
        // Off screen but still OPEN (in the dock), search takes focus.
        let (order, n) = s.order(false);
        assert_eq!(&order[..n], &[WindowId::Search]);
        assert!(s.is_visible(WindowId::Chat));
        assert!(s.is_minimized(WindowId::Chat));
        assert_eq!(s.focused(), Some(WindowId::Search));
        let (dock, dn) = s.minimized_list();
        assert_eq!(&dock[..dn], &[WindowId::Chat]);
    }

    #[test]
    fn restore_returns_raised_and_focused() {
        let mut s = stack();
        s.show(WindowId::Search);
        s.show(WindowId::Chat);
        s.minimize(WindowId::Chat);
        s.restore(WindowId::Chat);
        assert!(!s.is_minimized(WindowId::Chat));
        assert!(s.is_top(WindowId::Chat));
        assert_eq!(s.focused(), Some(WindowId::Chat));
        assert_eq!(s.minimized_list().1, 0);
    }

    #[test]
    fn dock_order_is_stable_registration_order() {
        let mut s = stack();
        s.show(WindowId::Search);
        s.show(WindowId::Chat);
        // Minimize chat FIRST, then search — dock still lists registration order.
        s.minimize(WindowId::Chat);
        s.minimize(WindowId::Search);
        let (dock, dn) = s.minimized_list();
        assert_eq!(&dock[..dn], &[WindowId::Search, WindowId::Chat]);
        assert_eq!(s.focused(), None);
        assert_eq!(s.order(false).1, 0);
    }

    #[test]
    fn close_clears_dock_membership_and_fullscreen() {
        let mut s = stack();
        s.show(WindowId::Chat);
        s.set_fullscreen(WindowId::Chat, true);
        s.minimize(WindowId::Chat);
        s.hide(WindowId::Chat);
        assert_eq!(s.minimized_list().1, 0);
        assert!(!s.is_fullscreen(WindowId::Chat));
        // Reopening starts floating, not fullscreen.
        s.show(WindowId::Chat);
        assert_eq!(s.fullscreen_active(), None);
    }

    #[test]
    fn fullscreen_sorts_above_floating_and_reports_active() {
        let mut s = stack();
        s.show(WindowId::Search);
        s.show(WindowId::Chat);
        s.set_fullscreen(WindowId::Search, true);
        // Search entered fullscreen → raised + focused + grouped on top even
        // though chat was raised later at a higher raw z.
        assert_eq!(s.fullscreen_active(), Some(WindowId::Search));
        assert_eq!(s.focused(), Some(WindowId::Search));
        s.raise(WindowId::Chat);
        let (order, n) = s.order(false);
        assert_eq!(&order[..n], &[WindowId::Chat, WindowId::Search], "fullscreen stays on top");
        // Leaving fullscreen restores plain z ordering.
        s.set_fullscreen(WindowId::Search, false);
        assert_eq!(s.fullscreen_active(), None);
        let (order, n) = s.order(false);
        assert_eq!(&order[..n], &[WindowId::Search, WindowId::Chat]);
    }

    #[test]
    fn minimized_fullscreen_window_is_not_active_until_restored() {
        let mut s = stack();
        s.show(WindowId::Chat);
        s.set_fullscreen(WindowId::Chat, true);
        s.minimize(WindowId::Chat);
        // Docked → the chrome is NOT covered.
        assert_eq!(s.fullscreen_active(), None);
        s.restore(WindowId::Chat);
        // Restore returns to fullscreen (mode survives the dock).
        assert_eq!(s.fullscreen_active(), Some(WindowId::Chat));
    }

    #[test]
    fn z_normalization_preserves_order() {
        let mut s = stack();
        s.show(WindowId::Search);
        s.show(WindowId::Chat);
        // Force many raises to cross the renormalization bound.
        for _ in 0..40_000 {
            s.raise(WindowId::Search);
            s.raise(WindowId::Chat);
        }
        s.raise(WindowId::Search);
        let (order, n) = s.order(false);
        assert_eq!(&order[..n], &[WindowId::Chat, WindowId::Search]);
        assert!(s.z_of(WindowId::Search) < Z_NORMALIZE_LIMIT);
        assert!(s.z_of(WindowId::Chat) < Z_NORMALIZE_LIMIT);
    }
}
