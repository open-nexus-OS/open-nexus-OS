//! Positioning utilities for UI components
//! Provides functions for calculating positions and layouts

use super::insets::Insets;

/// Screen dimensions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenDimensions {
    /// Screen width in pixels
    pub width: u32,
    /// Screen height in pixels
    pub height: u32,
}

impl ScreenDimensions {
    /// Create new screen dimensions
    pub fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    /// Get available width after applying insets
    pub fn available_width(&self, insets: &Insets) -> u32 {
        self.width.saturating_sub(insets.horizontal())
    }

    /// Get available height after applying insets
    pub fn available_height(&self, insets: &Insets) -> u32 {
        self.height.saturating_sub(insets.vertical())
    }

    /// Get available area after applying insets
    pub fn available_area(&self, insets: &Insets) -> u32 {
        self.available_width(insets) * self.available_height(insets)
    }
}

/// Rectangle for UI positioning
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    /// X position (left edge)
    pub x: i32,
    /// Y position (top edge)
    pub y: i32,
    /// Width
    pub width: u32,
    /// Height
    pub height: u32,
}

impl Rect {
    /// Create new rectangle
    pub fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self { x, y, width, height }
    }

    /// Create rectangle from top-left and bottom-right points
    pub fn from_points(top_left: (i32, i32), bottom_right: (i32, i32)) -> Self {
        let width = (bottom_right.0 - top_left.0).max(0) as u32;
        let height = (bottom_right.1 - top_left.1).max(0) as u32;
        Self {
            x: top_left.0,
            y: top_left.1,
            width,
            height,
        }
    }

    /// Get right edge position
    pub fn right(&self) -> i32 {
        self.x + self.width as i32
    }

    /// Get bottom edge position
    pub fn bottom(&self) -> i32 {
        self.y + self.height as i32
    }

    /// Get center point
    pub fn center(&self) -> (i32, i32) {
        (
            self.x + self.width as i32 / 2,
            self.y + self.height as i32 / 2,
        )
    }

    /// Check if point is inside rectangle
    pub fn contains(&self, point: (i32, i32)) -> bool {
        point.0 >= self.x
            && point.0 < self.right()
            && point.1 >= self.y
            && point.1 < self.bottom()
    }

    /// Check if rectangle intersects with another
    pub fn intersects(&self, other: &Rect) -> bool {
        self.x < other.right()
            && self.right() > other.x
            && self.y < other.bottom()
            && self.bottom() > other.y
    }

    /// Expand rectangle by given insets
    pub fn expand(&self, insets: &Insets) -> Rect {
        Rect {
            x: self.x - insets.left as i32,
            y: self.y - insets.top as i32,
            width: self.width + insets.horizontal(),
            height: self.height + insets.vertical(),
        }
    }

    /// Contract rectangle by given insets
    pub fn contract(&self, insets: &Insets) -> Rect {
        Rect {
            x: self.x + insets.left as i32,
            y: self.y + insets.top as i32,
            width: self.width.saturating_sub(insets.horizontal()),
            height: self.height.saturating_sub(insets.vertical()),
        }
    }
}

/// Alignment types for positioning
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HorizontalAlignment {
    /// Align to the left
    Left,
    /// Align to the center
    Center,
    /// Align to the right
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerticalAlignment {
    /// Align to the top
    Top,
    /// Align to the center
    Center,
    /// Align to the bottom
    Bottom,
}

/// Position a rectangle within a container using alignment
pub fn align_rect(
    content: &Rect,
    container: &Rect,
    horizontal: HorizontalAlignment,
    vertical: VerticalAlignment,
) -> Rect {
    let x = match horizontal {
        HorizontalAlignment::Left => container.x,
        HorizontalAlignment::Center => {
            container.x + (container.width.saturating_sub(content.width)) as i32 / 2
        }
        HorizontalAlignment::Right => {
            container.right() - content.width as i32
        }
    };

    let y = match vertical {
        VerticalAlignment::Top => container.y,
        VerticalAlignment::Center => {
            container.y + (container.height.saturating_sub(content.height)) as i32 / 2
        }
        VerticalAlignment::Bottom => {
            container.bottom() - content.height as i32
        }
    };

    Rect::new(x, y, content.width, content.height)
}

/// Calculate button layout within a container
pub fn calculate_button_layout(
    container_width: u32,
    button_count: usize,
    button_spacing: u32,
    button_size: u32,
) -> Vec<Rect> {
    if button_count == 0 {
        return Vec::new();
    }

    let total_button_width = button_count as u32 * button_size;
    let total_spacing = (button_count.saturating_sub(1)) as u32 * button_spacing;
    let total_width = total_button_width + total_spacing;

    let start_x = if total_width < container_width {
        (container_width - total_width) as i32 / 2
    } else {
        0
    };

    let mut buttons = Vec::new();
    for i in 0..button_count {
        let x = start_x + i as i32 * (button_size + button_spacing) as i32;
        buttons.push(Rect::new(x, 0, button_size, button_size));
    }

    buttons
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rect_creation() {
        let rect = Rect::new(10, 20, 100, 50);
        assert_eq!(rect.x, 10);
        assert_eq!(rect.y, 20);
        assert_eq!(rect.width, 100);
        assert_eq!(rect.height, 50);
    }

    #[test]
    fn test_rect_edges() {
        let rect = Rect::new(10, 20, 100, 50);
        assert_eq!(rect.right(), 110);
        assert_eq!(rect.bottom(), 70);
        assert_eq!(rect.center(), (60, 45));
    }

    #[test]
    fn test_rect_contains() {
        let rect = Rect::new(10, 20, 100, 50);
        assert!(rect.contains((50, 40)));
        assert!(!rect.contains((5, 40)));
        assert!(!rect.contains((150, 40)));
    }

    #[test]
    fn test_button_layout() {
        let buttons = calculate_button_layout(300, 3, 10, 80);
        assert_eq!(buttons.len(), 3);

        // First button should be centered
        let expected_start = (300 - (3 * 80 + 2 * 10)) / 2;
        assert_eq!(buttons[0].x, expected_start as i32);
        assert_eq!(buttons[1].x, (expected_start + 90) as i32);
        assert_eq!(buttons[2].x, (expected_start + 180) as i32);
    }
}
