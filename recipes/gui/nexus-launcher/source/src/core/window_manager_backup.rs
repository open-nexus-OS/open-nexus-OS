// core/window_manager.rs - Window management (placeholder)

// This will contain window management and Z-buffer logic extracted from main.rs
// For now, it's a placeholder to resolve compilation errors

use std::collections::BTreeMap;
use orbclient::{Window, WindowFlag, Renderer};

// Z-order for window stacking
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WindowZOrder {
    Back = 0,
    Normal = 1,
    Front = 2,
    AlwaysOnTop = 3,
}

pub struct WindowManager {
    windows: BTreeMap<usize, Window>,
    zbuffer: Vec<(usize, WindowZOrder, usize)>, // (window_id, z_order, sub_order)
    next_window_id: usize,
}

impl WindowManager {
    pub fn new() -> Self {
        Self {
            windows: BTreeMap::new(),
            zbuffer: Vec::new(),
            next_window_id: 1,
        }
    }

    pub fn create_window(&mut self, x: i32, y: i32, width: u32, height: u32, title: &str, flags: &[WindowFlag]) -> Result<usize, String> {
        let id = self.next_window_id;
        self.next_window_id += 1;

        let window = Window::new_flags(x, y, width, height, title, flags)
            .ok_or_else(|| format!("Failed to create window '{}'", title))?;

        self.windows.insert(id, window);
        Ok(id)
    }

    pub fn add_to_zbuffer(&mut self, window_id: usize, z_order: WindowZOrder, sub_order: usize) {
        self.zbuffer.push((window_id, z_order, sub_order));
        self.zbuffer.sort_by(|a, b| b.1.cmp(&a.1)); // Sort by Z-order (highest first)
    }

    pub fn get_window_mut(&mut self, id: usize) -> Option<&mut Window> {
        self.windows.get_mut(&id)
    }

    pub fn hit_test_window(&self, window_id: usize, x: i32, y: i32) -> bool {
        if let Some(window) = self.windows.get(&window_id) {
            x >= 0 && y >= 0 && x < window.width() as i32 && y < window.height() as i32
        } else {
            false
        }
    }

    pub fn get_topmost_window_at(&self, x: i32, y: i32) -> Option<usize> {
        for &(id, _, _) in self.zbuffer.iter() {
            if self.hit_test_window(id, x, y) {
                return Some(id);
            }
        }
        None
    }
}
