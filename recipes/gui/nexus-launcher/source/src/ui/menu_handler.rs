// src/ui/menu_handler.rs
// Menu system management - extracted from bar_handler.rs

use crate::services::package_service::Package;
use crate::modes::desktop::{show_desktop_menu, DesktopMenuResult};
use crate::modes::mobile::{show_mobile_menu, MobileMenuResult};
use crate::config::settings::{Mode, mode};

use log::{debug, info};

/// Menu handler for managing start menu system
pub struct MenuHandler {
    start_menu_open: bool,
    suppress_start_open: bool,
}

impl MenuHandler {
    pub fn new() -> Self {
        MenuHandler {
            start_menu_open: false,
            suppress_start_open: false,
        }
    }

    /// Handle start menu opening logic
    pub fn handle_start_menu_click(&mut self, width: u32, height: u32, packages: &mut Vec<Package>) -> MenuResult {
        if self.start_menu_open {
            self.suppress_start_open = true;
            return MenuResult::None;
        }

        if !self.suppress_start_open {
            self.start_menu_open = true;
            debug!("Opening start menu");

            let result = match mode() {
                Mode::Desktop => {
                    match show_desktop_menu(width, height, packages) {
                        DesktopMenuResult::Launch(exec) => {
                            if !exec.trim().is_empty() {
                                MenuResult::Launch(exec)
                            } else {
                                MenuResult::None
                            }
                        }
                        DesktopMenuResult::Logout => MenuResult::Logout,
                        _ => MenuResult::None,
                    }
                }
                Mode::Mobile => {
                    match show_mobile_menu(width, height, packages) {
                        MobileMenuResult::Launch(exec) => {
                            if !exec.trim().is_empty() {
                                MenuResult::Launch(exec)
                            } else {
                                MenuResult::None
                            }
                        }
                        MobileMenuResult::Logout => MenuResult::Logout,
                        _ => MenuResult::None,
                    }
                }
            };

            self.start_menu_open = false;
            self.suppress_start_open = true;

            result
        } else {
            MenuResult::None
        }
    }

    /// Reset suppress flag when mouse button is pressed
    pub fn reset_suppress_on_click(&mut self) {
        self.suppress_start_open = false;
    }

    /// Check if start menu is currently open
    pub fn is_start_menu_open(&self) -> bool {
        self.start_menu_open
    }

    /// Set start menu open state (used for external control)
    pub fn set_start_menu_open(&mut self, open: bool) {
        self.start_menu_open = open;
    }

    /// Set suppress flag (used for external control)
    pub fn set_suppress_start_open(&mut self, suppress: bool) {
        self.suppress_start_open = suppress;
    }
}

/// Result of menu operations
#[derive(Debug, Clone)]
pub enum MenuResult {
    None,
    Launch(String),
    Logout,
}

impl MenuResult {
    /// Check if this result should cause the launcher to exit
    pub fn should_exit(&self) -> bool {
        matches!(self, MenuResult::Logout)
    }

    /// Extract launch command if available
    pub fn get_launch_command(&self) -> Option<String> {
        match self {
            MenuResult::Launch(cmd) => Some(cmd.clone()),
            _ => None,
        }
    }
}
