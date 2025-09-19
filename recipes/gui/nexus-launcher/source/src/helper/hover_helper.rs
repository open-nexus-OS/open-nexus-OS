// Simplified hover system for nexus-launcher
// This module provides a clean, easy-to-debug approach to hover effects

use orbclient::{Color, Window, Renderer};

/// Simple hover state tracking
/// Much easier to debug than the complex point-in calculations
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HoverState {
    pub is_hovering: bool,
    pub last_hovered: Option<(i32, i32, i32, i32)>, // (x, y, w, h)
}

impl HoverState {
    pub fn new() -> Self {
        Self {
            is_hovering: false,
            last_hovered: None,
        }
    }

    /// Check if mouse is hovering over a rectangle
    /// Simplified version that's easier to debug
    pub fn check_hover(&mut self, mouse_x: i32, mouse_y: i32, x: i32, y: i32, w: i32, h: i32) -> bool {
        let is_hovering = mouse_x >= x && mouse_x < x + w && mouse_y >= y && mouse_y < y + h;

        // Only update state if it changed (prevents unnecessary redraws)
        if is_hovering != self.is_hovering {
            self.is_hovering = is_hovering;
            self.last_hovered = if is_hovering { Some((x, y, w, h)) } else { None };
        }

        is_hovering
    }

    /// Draw hover effect if currently hovering
    /// Simplified version that's easier to debug
    pub fn draw_hover_effect(&self, window: &mut Window, large: bool) {
        if let Some((x, y, w, h)) = self.last_hovered {
            let color = if large {
                Color::rgba(255, 255, 255, 28)
            } else {
                Color::rgba(0, 0, 0, 22)
            };

            // Draw simple rectangle instead of complex rounded rect
            window.rect(x, y, w as u32, h as u32, color);
        }
    }

    /// Clear hover state
    pub fn clear(&mut self) {
        self.is_hovering = false;
        self.last_hovered = None;
    }
}

impl Default for HoverState {
    fn default() -> Self {
        Self::new()
    }
}

/// Simple hover manager for multiple elements
pub struct HoverManager {
    elements: Vec<HoverState>,
}

impl HoverManager {
    pub fn new() -> Self {
        Self {
            elements: Vec::new(),
        }
    }

    /// Add a new hover element
    pub fn add_element(&mut self) -> usize {
        self.elements.push(HoverState::new());
        self.elements.len() - 1
    }

    /// Check hover for a specific element
    pub fn check_hover(&mut self, index: usize, mouse_x: i32, mouse_y: i32, x: i32, y: i32, w: i32, h: i32) -> bool {
        if let Some(element) = self.elements.get_mut(index) {
            element.check_hover(mouse_x, mouse_y, x, y, w, h)
        } else {
            false
        }
    }

    /// Draw all hover effects
    pub fn draw_hover_effects(&self, window: &mut Window, large: bool) {
        for element in &self.elements {
            element.draw_hover_effect(window, large);
        }
    }

    /// Clear all hover states
    pub fn clear_all(&mut self) {
        for element in &mut self.elements {
            element.clear();
        }
    }
}

impl Default for HoverManager {
    fn default() -> Self {
        Self::new()
    }
}
