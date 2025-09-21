// src/types/state.rs
// Global state management for the launcher

use std::collections::BTreeMap;
use std::process::Child;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::Instant;
use orbclient::{Window, Renderer};
use crate::services::package_service::Package;

/// Global UI scale factor (atomic for thread safety)
pub static SCALE: AtomicIsize = AtomicIsize::new(1);

/// Z-order levels for window management (Orbital-ähnlich)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WindowZOrder {
    Back = 0,
    Normal = 1,
    Front = 2,
    AlwaysOnTop = 3,
}

/// Global launcher state
pub struct LauncherState {
    /// Running child processes
    pub children: Vec<(String, Child)>,
    
    /// All discovered packages
    pub packages: Vec<Package>,
    
    /// Packages organized by category
    pub category_packages: BTreeMap<String, Vec<Package>>,
    
    /// Start menu packages (categories + system actions)
    pub start_packages: Vec<Package>,
    
    /// Current time display
    pub time: String,
    
    /// Start menu state
    pub start_menu_open: bool,
    pub suppress_start_open: bool,
    
    /// Mouse state
    pub mouse_x: i32,
    pub mouse_y: i32,
    pub mouse_left: bool,
    pub last_mouse_left: bool,
    
    /// Selected item in taskbar
    pub selected: i32,
    
    /// Screen dimensions
    pub width: u32,
    pub height: u32,
    
    /// Event counter for debugging
    pub event_count: u32,
}

impl LauncherState {
    pub fn new(width: u32, height: u32) -> Self {
        LauncherState {
            children: Vec::new(),
            packages: Vec::new(),
            category_packages: BTreeMap::new(),
            start_packages: Vec::new(),
            time: String::new(),
            start_menu_open: false,
            suppress_start_open: false,
            mouse_x: -1,
            mouse_y: -1,
            mouse_left: false,
            last_mouse_left: false,
            selected: -1,
            width,
            height,
            event_count: 0,
        }
    }
    
    /// Update the current time display
    pub fn update_time(&mut self) {
        use libredox::data::TimeSpec;
        use libredox::flag;
        
        let time = libredox::call::clock_gettime(flag::CLOCK_REALTIME)
            .expect("launcher: failed to read time");

        let ts = time.tv_sec;
        let s = ts % 86400;
        let h = s / 3600;
        let m = s / 60 % 60;
        self.time = format!("{:>02}:{:>02}", h, m);
    }
    
    /// Increment event counter
    pub fn increment_event_count(&mut self) {
        self.event_count += 1;
    }
    
    /// Check if any child process is running with given exec string
    pub fn is_child_running(&self, exec: &str) -> bool {
        self.children.iter().any(|(child_exec, _)| child_exec == exec)
    }
    
    /// Add a new child process
    pub fn add_child(&mut self, exec: String, child: Child) {
        self.children.push((exec, child));
    }
    
    /// Remove finished child processes
    pub fn reap_children(&mut self) {
        let mut i = 0;
        while i < self.children.len() {
            let remove = match self.children[i].1.try_wait() {
                Ok(None) => false,
                Ok(Some(status)) => {
                    log::info!("{} ({}) exited with {}", 
                              self.children[i].0, 
                              self.children[i].1.id(), 
                              status);
                    true
                }
                Err(err) => {
                    log::error!("failed to wait for {} ({}): {}", 
                               self.children[i].0, 
                               self.children[i].1.id(), 
                               err);
                    true
                }
            };
            if remove { 
                self.children.remove(i); 
            } else { 
                i += 1; 
            }
        }
    }
}

/// Window management state
pub struct WindowState {
    /// Z-Buffer for window hierarchy
    pub zbuffer: Vec<(usize, WindowZOrder, usize)>,
    
    /// Window registry
    pub windows: BTreeMap<usize, Window>,
    
    /// Next available window ID
    pub next_window_id: usize,
    
    /// Panel visibility state
    pub panels_visible: bool,
    pub panels_fadeout_deadline: Option<Instant>,
}

impl WindowState {
    pub fn new() -> Self {
        WindowState {
            zbuffer: Vec::new(),
            windows: BTreeMap::new(),
            next_window_id: 1,
            panels_visible: false,
            panels_fadeout_deadline: None,
        }
    }
    
    /// Add a window to the Z-Buffer system
    pub fn add_window(&mut self, id: usize, window: Window, z_order: WindowZOrder, sub_order: usize) {
        self.windows.insert(id, window);
        self.zbuffer.push((id, z_order, sub_order));
        // Sort Z-Buffer (highest priority first)
        self.zbuffer.sort_by(|a, b| b.1.cmp(&a.1));
    }
    
    /// Get next available window ID
    pub fn get_next_window_id(&mut self) -> usize {
        let id = self.next_window_id;
        self.next_window_id += 1;
        id
    }
    
    /// Get window by ID
    pub fn get_window(&self, id: usize) -> Option<&Window> {
        self.windows.get(&id)
    }
    
    /// Get mutable window by ID
    pub fn get_window_mut(&mut self, id: usize) -> Option<&mut Window> {
        self.windows.get_mut(&id)
    }
    
    /// Hit-test a window at given coordinates
    pub fn hit_test_window(&self, window_id: usize, x: i32, y: i32) -> bool {
        if let Some(window) = self.windows.get(&window_id) {
            x >= 0 && y >= 0 && x < window.width() as i32 && y < window.height() as i32
        } else {
            false
        }
    }
    
    /// Get topmost window at given coordinates
    pub fn get_topmost_window_at(&self, x: i32, y: i32) -> Option<usize> {
        for &(window_id, _, _) in self.zbuffer.iter() {
            if self.hit_test_window(window_id, x, y) {
                return Some(window_id);
            }
        }
        None
    }
}