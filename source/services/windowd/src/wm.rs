// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Window manager for the retained-surface compositor (TASK-0064).
//! Pure logic — owns window geometry, visibility, z-order and the drag state
//! machine; no OS or rendering deps, so it is fully host-testable. The compositor
//! reads `chat_window().bounds` to place the chat layer's cached surface, and
//! routes pointer events here. Moving a window only changes its bounds → the
//! compositor re-blits the cached surface at the new position (no re-render).
//!
//! v1 manages a single window (the chat). The structure (WindowId, per-window
//! state) leaves room to grow without changing the contract.
//!
//! OWNERS: @ui
//! STATUS: TASK-0064 — window management v1
//! API_STABILITY: Unstable

/// Title-bar height in pixels (drag handle + title label).
pub(crate) const TITLE_BAR_H: i32 = crate::interaction::CHAT_TITLE_BAR_H as i32;
/// Width of the close-button zone at the right of the title bar.
pub(crate) const CLOSE_ZONE_W: i32 = crate::interaction::CHAT_CLOSE_ZONE_W as i32;

/// A display-space rectangle in window-manager coordinates (signed, so a window
/// dragged partly off-screen is representable before clamping).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WmRect {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) w: i32,
    pub(crate) h: i32,
}

impl WmRect {
    pub(crate) const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }
    pub(crate) fn right(self) -> i32 {
        self.x + self.w
    }
    pub(crate) fn bottom(self) -> i32 {
        self.y + self.h
    }
    pub(crate) fn contains(self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.right() && py >= self.y && py < self.bottom()
    }
}

/// Identifier for a managed window. v1 has only the chat window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WindowId {
    Chat,
}

/// A managed window.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Window {
    pub(crate) id: WindowId,
    pub(crate) title: &'static str,
    pub(crate) bounds: WmRect,
    pub(crate) default_bounds: WmRect,
    pub(crate) visible: bool,
    /// 0 = wallpaper, 1 = proof, 2 = sidebar, 3 = chat window (top).
    pub(crate) z_index: u32,
}

impl Window {
    /// Title-bar rect (drag handle), excluding the close-button zone.
    pub(crate) fn title_bar_rect(&self) -> WmRect {
        WmRect::new(
            self.bounds.x,
            self.bounds.y,
            (self.bounds.w - CLOSE_ZONE_W).max(0),
            TITLE_BAR_H,
        )
    }
    /// Close-button rect at the top-right of the title bar.
    pub(crate) fn close_rect(&self) -> WmRect {
        WmRect::new(self.bounds.right() - CLOSE_ZONE_W, self.bounds.y, CLOSE_ZONE_W, TITLE_BAR_H)
    }
    /// Content rect below the title bar (where the chat list renders).
    pub(crate) fn content_rect(&self) -> WmRect {
        WmRect::new(
            self.bounds.x,
            self.bounds.y + TITLE_BAR_H,
            self.bounds.w,
            (self.bounds.h - TITLE_BAR_H).max(0),
        )
    }
}

/// Active drag: which window, and the grab offset within it so the window
/// tracks the pointer without jumping.
#[derive(Clone, Copy, Debug)]
struct DragState {
    id: WindowId,
    grab_dx: i32,
    grab_dy: i32,
}

/// What a primary-button press resolved to (so the runtime can emit markers /
/// trigger side effects).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PointerAction {
    /// Began dragging a window by its title bar.
    DragStarted(WindowId),
    /// The close button was pressed; the window was closed.
    Closed(WindowId),
    /// Nothing window-related was hit (pass through to other UI).
    None,
}

/// Single-window (chat) manager. Grows to a list without changing the contract.
pub(crate) struct WindowManager {
    chat: Window,
    drag: Option<DragState>,
}

impl WindowManager {
    /// Create the manager with the chat window closed at `default_bounds`.
    pub(crate) fn new(default_bounds: WmRect) -> Self {
        Self {
            chat: Window {
                id: WindowId::Chat,
                title: "Chat",
                bounds: default_bounds,
                default_bounds,
                visible: false,
                z_index: 3,
            },
            drag: None,
        }
    }

    pub(crate) fn chat_window(&self) -> &Window {
        &self.chat
    }

    pub(crate) fn chat_visible(&self) -> bool {
        self.chat.visible
    }

    fn window_mut(&mut self, id: WindowId) -> &mut Window {
        match id {
            WindowId::Chat => &mut self.chat,
        }
    }

    /// Open a window (idempotent — if already open it stays open / focused).
    pub(crate) fn open(&mut self, id: WindowId) {
        self.window_mut(id).visible = true;
    }

    /// Close a window (idempotent). Cancels any drag of it.
    pub(crate) fn close(&mut self, id: WindowId) {
        self.window_mut(id).visible = false;
        if matches!(self.drag, Some(d) if d.id == id) {
            self.drag = None;
        }
    }

    /// Toggle a window open/closed. Returns the new visibility.
    pub(crate) fn toggle(&mut self, id: WindowId) -> bool {
        let now = !self.window_mut(id).visible;
        self.window_mut(id).visible = now;
        if !now && matches!(self.drag, Some(d) if d.id == id) {
            self.drag = None;
        }
        now
    }

    /// Which visible window's title bar is at (x, y)?
    pub(crate) fn hit_test_title_bar(&self, x: i32, y: i32) -> Option<WindowId> {
        (self.chat.visible && self.chat.title_bar_rect().contains(x, y)).then_some(WindowId::Chat)
    }

    /// Which visible window's close button is at (x, y)?
    pub(crate) fn hit_test_close(&self, x: i32, y: i32) -> Option<WindowId> {
        (self.chat.visible && self.chat.close_rect().contains(x, y)).then_some(WindowId::Chat)
    }

    /// Whether (x, y) is inside any visible window (content or chrome).
    pub(crate) fn hit_test_window(&self, x: i32, y: i32) -> Option<WindowId> {
        (self.chat.visible && self.chat.bounds.contains(x, y)).then_some(WindowId::Chat)
    }

    pub(crate) fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Primary-button press at (x, y). Close button wins over the title bar;
    /// the title bar starts a drag. Anything else returns `None` (pass through).
    pub(crate) fn on_pointer_down(&mut self, x: i32, y: i32) -> PointerAction {
        if let Some(id) = self.hit_test_close(x, y) {
            self.close(id);
            return PointerAction::Closed(id);
        }
        if let Some(id) = self.hit_test_title_bar(x, y) {
            let b = self.window_mut(id).bounds;
            self.drag = Some(DragState { id, grab_dx: x - b.x, grab_dy: y - b.y });
            return PointerAction::DragStarted(id);
        }
        PointerAction::None
    }

    /// Pointer move at (x, y). If dragging, move the window so the grab point
    /// stays under the pointer, clamped to the display. Returns true if a window
    /// moved (so the runtime damages old+new regions).
    pub(crate) fn on_pointer_move(
        &mut self,
        x: i32,
        y: i32,
        display_w: i32,
        display_h: i32,
    ) -> bool {
        let Some(drag) = self.drag else {
            return false;
        };
        let win = self.window_mut(drag.id);
        let new_x = x - drag.grab_dx;
        let new_y = y - drag.grab_dy;
        let clamped_x = new_x.clamp(0, (display_w - win.bounds.w).max(0));
        let clamped_y = new_y.clamp(0, (display_h - win.bounds.h).max(0));
        if clamped_x == win.bounds.x && clamped_y == win.bounds.y {
            return false;
        }
        win.bounds.x = clamped_x;
        win.bounds.y = clamped_y;
        true
    }

    /// End any drag. Returns the window that was being dragged, if any.
    pub(crate) fn on_pointer_up(&mut self) -> Option<WindowId> {
        self.drag.take().map(|d| d.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DISPLAY_W: i32 = 1280;
    const DISPLAY_H: i32 = 800;
    fn default_bounds() -> WmRect {
        WmRect::new(890, 96, 366, 600)
    }
    fn wm() -> WindowManager {
        WindowManager::new(default_bounds())
    }

    #[test]
    fn starts_closed_and_toggles() {
        let mut m = wm();
        assert!(!m.chat_visible());
        assert!(m.toggle(WindowId::Chat));
        assert!(m.chat_visible());
        assert!(!m.toggle(WindowId::Chat));
        assert!(!m.chat_visible());
    }

    #[test]
    fn open_close_idempotent() {
        let mut m = wm();
        m.open(WindowId::Chat);
        m.open(WindowId::Chat);
        assert!(m.chat_visible());
        m.close(WindowId::Chat);
        m.close(WindowId::Chat);
        assert!(!m.chat_visible());
    }

    #[test]
    fn hit_tests_only_when_visible_and_match_rects() {
        let mut m = wm();
        let b = default_bounds();
        // Closed → no hits.
        assert_eq!(m.hit_test_title_bar(b.x + 1, b.y + 1), None);
        m.open(WindowId::Chat);
        // Title-bar top-left inside; just past the title-bar bottom is outside.
        assert_eq!(m.hit_test_title_bar(b.x, b.y), Some(WindowId::Chat));
        assert_eq!(m.hit_test_title_bar(b.x, b.y + TITLE_BAR_H), None);
        // Close zone is the right CLOSE_ZONE_W of the title bar, not the title bar.
        let close_x = b.right() - CLOSE_ZONE_W + 1;
        assert_eq!(m.hit_test_close(close_x, b.y + 1), Some(WindowId::Chat));
        assert_eq!(m.hit_test_title_bar(close_x, b.y + 1), None);
    }

    #[test]
    fn close_button_press_closes() {
        let mut m = wm();
        m.open(WindowId::Chat);
        let b = default_bounds();
        let action = m.on_pointer_down(b.right() - 10, b.y + 10);
        assert_eq!(action, PointerAction::Closed(WindowId::Chat));
        assert!(!m.chat_visible());
    }

    #[test]
    fn drag_moves_window_by_delta() {
        let mut m = wm();
        m.open(WindowId::Chat);
        let b = default_bounds();
        // Grab the title bar at (+20,+10) inside the window.
        let gx = b.x + 20;
        let gy = b.y + 10;
        assert_eq!(m.on_pointer_down(gx, gy), PointerAction::DragStarted(WindowId::Chat));
        assert!(m.is_dragging());
        // Move pointer by (-100, +50): window follows, grab offset preserved.
        assert!(m.on_pointer_move(gx - 100, gy + 50, DISPLAY_W, DISPLAY_H));
        assert_eq!(m.chat_window().bounds.x, b.x - 100);
        assert_eq!(m.chat_window().bounds.y, b.y + 50);
        assert_eq!(m.on_pointer_up(), Some(WindowId::Chat));
        assert!(!m.is_dragging());
    }

    #[test]
    fn drag_clamps_to_display() {
        let mut m = wm();
        m.open(WindowId::Chat);
        let b = default_bounds();
        m.on_pointer_down(b.x + 5, b.y + 5);
        // Drag far past the top-left corner → clamps to (0,0).
        m.on_pointer_move(-500, -500, DISPLAY_W, DISPLAY_H);
        assert_eq!(m.chat_window().bounds.x, 0);
        assert_eq!(m.chat_window().bounds.y, 0);
        // Drag far past bottom-right → clamps so the window stays fully on-screen.
        m.on_pointer_move(99999, 99999, DISPLAY_W, DISPLAY_H);
        assert_eq!(m.chat_window().bounds.x, DISPLAY_W - b.w);
        assert_eq!(m.chat_window().bounds.y, DISPLAY_H - b.h);
        assert!(m.chat_window().bounds.right() <= DISPLAY_W);
        assert!(m.chat_window().bounds.bottom() <= DISPLAY_H);
    }

    #[test]
    fn no_drag_without_title_bar_grab() {
        let mut m = wm();
        m.open(WindowId::Chat);
        let b = default_bounds();
        // Press in the content area (below the title bar) → no drag.
        assert_eq!(m.on_pointer_down(b.x + 10, b.y + TITLE_BAR_H + 10), PointerAction::None);
        assert!(!m.is_dragging());
        assert!(!m.on_pointer_move(b.x - 50, b.y, DISPLAY_W, DISPLAY_H));
    }

    #[test]
    fn content_rect_is_below_title_bar() {
        let m = wm();
        let c = m.chat_window().content_rect();
        let b = default_bounds();
        assert_eq!(c.y, b.y + TITLE_BAR_H);
        assert_eq!(c.h, b.h - TITLE_BAR_H);
    }
}
