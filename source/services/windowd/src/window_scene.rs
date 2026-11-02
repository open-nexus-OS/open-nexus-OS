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
//! as dozens of inline `visible && !takeover` show checks that could
//! only be verified by booting. Moving the decision here makes it a `cargo test` failure
//! instead of a boot hunt. The one input beside the stack is `frozen` (RFC-0095 capture).

use alloc::vec::Vec;

/// A composable shell window (RFC-0065 multi-window).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WindowId {
    /// A cross-process app client window (ADR-0042 / TASK-0080D R1, by reference since
    /// RFC-0114): the body samples the app's own buffer. The index is the runtime's app-window
    /// SLOT — the slots grow with the windows (RFC-0114 §7); the stack orders them like any other
    /// window. On the wire a window is its key (`AppWindowSlot::key`), never this index.
    App(u16),
    /// The DESKTOP surface (RFC-0065 / Umbau #17): the shell (or greeter) running
    /// as its OWN app-host process, composited as the full-screen base layer
    /// BELOW all floating windows (see [`WindowRole::Desktop`]). Chromeless — it
    /// has no title bar / drag / resize / hover / close (the shell owns its whole
    /// surface), so the runtime's per-window chrome/interaction matches treat it
    /// as a no-op; only composition places it (at the bottom band).
    Desktop,
}

pub use crate::window_state::{WindowRole, WindowState};

/// The most windows one order lists: a frame composes at most `MAX_LAYERS` layers (a window
/// takes at least its body), so an order of more windows than that could not be drawn — the
/// topmost of them are the order, the ones below neither drawn nor hit (RFC-0114 §7: the limit
/// is the frame's layer budget, never a slot count).
pub const ORDER_MAX: usize = nexus_gfx::frame::MAX_LAYERS;

/// Whether a floating window should be composited this frame: `visible` and the
/// screen is not frozen for a capture (the frozen frame already shows it).
pub fn should_show(visible: bool, frozen: bool) -> bool {
    visible && !frozen
}

/// The windows to composite over the base layer, back-to-front (z ascending).
/// Hidden windows (and every window and overlay while the screen is frozen) are excluded.
#[cfg(test)] // documented z-order contract (composition_order fix), pinned by the host tests below
pub fn composition_order(windows: &[WindowState], frozen: bool) -> Vec<WindowId> {
    let mut visible: Vec<(WindowId, i32)> =
        windows.iter().filter(|w| w.showable(frozen)).map(|w| (w.id, w.order_key())).collect();
    visible.sort_by_key(|&(_, key)| key);
    visible.into_iter().map(|(id, _)| id).collect()
}

/// Anti-black-screen invariant: the compositor **always** draws a base layer
/// (wallpaper / shell) below the windows, so an *empty* [`composition_order`] is a
/// clean desktop — never a black frame. The runtime must honour this; the tests
/// below pin the contract.
#[cfg(test)] // anti-black-screen invariant, pinned by the host tests below
pub const BASE_ALWAYS_PRESENT: bool = true;

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
/// The windows grow (RFC-0114 §7: a slot is registered when the runtime makes it); the
/// per-frame queries stay alloc-free — fixed `[WindowId; ORDER_MAX]` arrays + a count, because
/// windowd's frame path allocates nothing.
pub struct WindowStack {
    entries: Vec<WindowState>,
    focused: Option<WindowId>,
    next_z: i16,
}

impl WindowStack {
    /// Register the managed windows, all hidden, stacked in the given order
    /// (later ids start on top once shown).
    pub fn new(ids: &[WindowId]) -> Self {
        let mut s = Self { entries: Vec::new(), focused: None, next_z: 0 };
        for &id in ids {
            s.register(id);
        }
        s
    }

    /// Register `id` hidden, on top of the stack once shown (a no-op for one registered).
    /// `false`: the stack could not grow.
    pub fn register(&mut self, id: WindowId) -> bool {
        if self.index_of(id).is_some() {
            return true;
        }
        if self.entries.try_reserve(1).is_err() {
            return false;
        }
        self.entries.push(match id {
            // The desktop base gets the DESKTOP z-band (always bottom); every other id is a
            // floating window.
            WindowId::Desktop => WindowState::desktop(id, false),
            _ => WindowState::floating(id, false, self.next_z),
        });
        self.next_z = self.next_z.saturating_add(1);
        if self.next_z >= Z_NORMALIZE_LIMIT {
            self.normalize_z();
        }
        true
    }

    fn index_of(&self, id: WindowId) -> Option<usize> {
        self.entries.iter().position(|w| w.id == id)
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
        self.entries
            .iter()
            .filter(|w| w.visible && !w.minimized)
            .max_by_key(|w| w.order_key())
            .map(|w| w.id)
    }

    /// Re-bands a registered window (create-time: the surface's DECLARED
    /// intent resolves its role — e.g. the OSK's `level: overlay`).
    // Called only from the OS-gated compositor runtime (`app_surface`,
    // `text_input`); cfg-gate instead of a blanket allow.
    #[cfg(nexus_env = "os")]
    pub fn set_role(&mut self, id: WindowId, role: WindowRole) {
        if let Some(i) = self.index_of(id) {
            self.entries[i].role = role;
        }
    }

    /// Show `id` WITHOUT moving focus (RFC-0075 OSK contract: the on-screen
    /// keyboard must never steal the focused text field's window focus).
    #[cfg(nexus_env = "os")]
    pub fn show_unfocused(&mut self, id: WindowId) {
        if let Some(i) = self.index_of(id) {
            self.entries[i].visible = true;
            self.entries[i].minimized = false;
        }
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

    /// Minimized windows in registration order (RFC-0086: the SHELL renders them; the
    /// minimize chord restores the first, TASK-0333 6d) — the first `ORDER_MAX`.
    pub fn minimized_list(&self) -> ([WindowId; ORDER_MAX], usize) {
        let mut out = [WindowId::App(0); ORDER_MAX];
        let mut n = 0;
        for w in self.entries.iter().filter(|w| w.visible && w.minimized).take(ORDER_MAX) {
            out[n] = w.id;
            n += 1;
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
        self.entries
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

    /// On-screen windows back-to-front (composite order), alloc-free: the topmost
    /// [`ORDER_MAX`] of them. Minimized windows are excluded; fullscreen windows group on top —
    /// and while one is on screen, FLOATING windows are excluded entirely (they are fully
    /// covered: compositing them would be wasted work, and only the wallpaper base remains
    /// beneath the fullscreen surface).
    pub fn order(&self, frozen: bool) -> ([WindowId; ORDER_MAX], usize) {
        let fullscreen_on_screen = self.entries.iter().any(|w| w.showable(frozen) && w.fullscreen);
        let mut out = [WindowId::App(0); ORDER_MAX];
        let mut keys = [0i32; ORDER_MAX];
        let drawn =
            |w: &&WindowState| w.showable(frozen) && (!fullscreen_on_screen || w.fullscreen);
        // The desktop base, when drawn, is first and never gives way.
        let base = self.entries.iter().filter(drawn).any(|w| w.id == WindowId::Desktop);
        if base {
            out[0] = WindowId::Desktop;
        }
        let first = usize::from(base);
        let mut n = first;
        for w in self.entries.iter().filter(drawn).filter(|w| w.id != WindowId::Desktop) {
            // Insertion by the order key ascending; when full, the bottom-most gives way to one
            // above it (the order keeps the topmost).
            let key = w.order_key();
            if n == ORDER_MAX {
                if keys[first] >= key {
                    continue;
                }
                out.copy_within(first + 1..n, first);
                keys.copy_within(first + 1..n, first);
                n -= 1;
            }
            let mut j = n;
            while j > first && keys[j - 1] > key {
                out[j] = out[j - 1];
                keys[j] = keys[j - 1];
                j -= 1;
            }
            out[j] = w.id;
            keys[j] = key;
            n += 1;
        }
        (out, n)
    }

    /// Visible windows front-to-back (hit-test order) — the exact reverse of
    /// [`Self::order`], so occlusion and input can never disagree.
    pub fn hit_order(&self, frozen: bool) -> ([WindowId; ORDER_MAX], usize) {
        let (mut order, n) = self.order(frozen);
        order[..n].reverse();
        (order, n)
    }

    /// Order-preserving z renormalization to `0..len` (rare: a long session's raises).
    fn normalize_z(&mut self) {
        let mut ranked: Vec<usize> = (0..self.entries.len()).collect();
        ranked.sort_unstable_by_key(|&i| self.entries[i].z);
        for (rank, &i) in ranked.iter().enumerate() {
            self.entries[i].z = rank as i16;
        }
        self.next_z = self.entries.len() as i16;
    }
}

#[cfg(test)]
#[path = "window_scene_tests.rs"]
mod tests;
