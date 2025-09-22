// src/core/window_manager.rs
// Central registry for orbclient::Window instances with a simple Z-order.

use orbclient::Renderer;
use std::collections::BTreeMap;
use std::time::Instant;
use orbclient::{Window, WindowFlag};

/// Z-order levels for window management
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WindowZOrder {
    Back = 0,
    Normal = 1,
    Front = 2,
    AlwaysOnTop = 3,
}

/// Thin manager around a set of windows + zbuffer ordering.
pub struct WindowManager {
    zbuffer: Vec<(usize, WindowZOrder, usize)>, // (id, z, suborder)
    windows: BTreeMap<usize, Window>,
    next_window_id: usize,

    // For overlays that need visibility decisions
    pub panels_visible: bool,
    pub panels_fadeout_deadline: Option<Instant>,
}

impl WindowManager {
    pub fn new() -> Self {
        Self {
            zbuffer: Vec::new(),
            windows: BTreeMap::new(),
            next_window_id: 1,
            panels_visible: false,
            panels_fadeout_deadline: None,
        }
    }

    /// Reserve a unique id.
    pub fn alloc_id(&mut self) -> usize {
        let id = self.next_window_id;
        self.next_window_id += 1;
        id
    }

    /// Insert an existing window under the given id and z-order.
    pub fn insert(&mut self, id: usize, window: Window, z: WindowZOrder, suborder: usize) {
        self.windows.insert(id, window);
        self.zbuffer.push((id, z, suborder));
        self.zbuffer.sort_by(|a, b| b.1.cmp(&a.1).then(a.2.cmp(&b.2)));
    }

    /// Convenience: create a window with flags and register it.
    pub fn create_window(
        &mut self,
        x: i32, y: i32, w: u32, h: u32, title: &str,
        flags: &[WindowFlag],
        z: WindowZOrder, suborder: usize,
    ) -> usize {
        let id = self.alloc_id();
        let window = Window::new_flags(x, y, w, h, title, flags)
            .expect("window_manager: failed to create window");
        self.insert(id, window, z, suborder);
        id
    }

    pub fn get(&self, id: usize) -> Option<&Window> {
        self.windows.get(&id)
    }
    pub fn get_mut(&mut self, id: usize) -> Option<&mut Window> {
        self.windows.get_mut(&id)
    }

    /// Iterate top-to-bottom over (id, window)
    pub fn iter_top_to_bottom(&self) -> impl Iterator<Item = (usize, &Window)> {
        self.zbuffer.iter().map(move |(id, _, _)| (*id, &self.windows[id]))
    }

    /// Hit test against a specific window id.
    pub fn hit_test(&self, id: usize, x: i32, y: i32) -> bool {
        if let Some(win) = self.windows.get(&id) {
            x >= 0 && y >= 0 && x < win.width() as i32 && y < win.height() as i32
        } else { false }
    }

    /// Find the topmost window under (x,y).
    pub fn topmost_at(&self, x: i32, y: i32) -> Option<usize> {
        for (id, win) in self.iter_top_to_bottom() {
            if x >= 0 && y >= 0 && x < win.width() as i32 && y < win.height() as i32 {
                return Some(id);
            }
        }
        None
    }
}
