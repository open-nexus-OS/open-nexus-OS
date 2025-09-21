//! Screen insets for UI positioning
//! Provides utilities for handling screen margins and safe areas

/// Screen insets representing margins from screen edges
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Insets {
    /// Top inset (from top edge)
    pub top: u32,
    /// Bottom inset (from bottom edge)
    pub bottom: u32,
    /// Left inset (from left edge)
    pub left: u32,
    /// Right inset (from right edge)
    pub right: u32,
}

impl Insets {
    /// Create new insets with all values set to the same amount
    pub fn uniform(value: u32) -> Self {
        Self {
            top: value,
            bottom: value,
            left: value,
            right: value,
        }
    }

    /// Create new insets with separate horizontal and vertical values
    pub fn new(horizontal: u32, vertical: u32) -> Self {
        Self {
            top: vertical,
            bottom: vertical,
            left: horizontal,
            right: horizontal,
        }
    }

    /// Create new insets with individual values
    pub fn from_values(top: u32, bottom: u32, left: u32, right: u32) -> Self {
        Self {
            top,
            bottom,
            left,
            right,
        }
    }

    /// Create zero insets (no margins)
    pub fn zero() -> Self {
        Self {
            top: 0,
            bottom: 0,
            left: 0,
            right: 0,
        }
    }

    /// Get total horizontal inset (left + right)
    pub fn horizontal(&self) -> u32 {
        self.left + self.right
    }

    /// Get total vertical inset (top + bottom)
    pub fn vertical(&self) -> u32 {
        self.top + self.bottom
    }

    /// Check if any inset is non-zero
    pub fn has_insets(&self) -> bool {
        self.top > 0 || self.bottom > 0 || self.left > 0 || self.right > 0
    }

    /// Add another set of insets to this one
    pub fn add(&self, other: Insets) -> Insets {
        Insets {
            top: self.top + other.top,
            bottom: self.bottom + other.bottom,
            left: self.left + other.left,
            right: self.right + other.right,
        }
    }

    /// Subtract another set of insets from this one
    pub fn subtract(&self, other: Insets) -> Insets {
        Insets {
            top: self.top.saturating_sub(other.top),
            bottom: self.bottom.saturating_sub(other.bottom),
            left: self.left.saturating_sub(other.left),
            right: self.right.saturating_sub(other.right),
        }
    }
}

impl Default for Insets {
    fn default() -> Self {
        Self::zero()
    }
}

/// Utility functions for converting between different units
pub mod conversion {
    /// Convert density-independent pixels (dp) to physical pixels
    /// This is a simplified conversion - in a real implementation,
    /// this would use the actual device density
    pub fn dp_to_px(dp: u32) -> u32 {
        // Assume 1dp = 1px for now
        // In a real implementation, this would be:
        // dp * device_density_scale
        dp
    }

    /// Convert physical pixels to density-independent pixels
    pub fn px_to_dp(px: u32) -> u32 {
        // Assume 1px = 1dp for now
        px
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insets_creation() {
        let insets = Insets::uniform(10);
        assert_eq!(insets.top, 10);
        assert_eq!(insets.bottom, 10);
        assert_eq!(insets.left, 10);
        assert_eq!(insets.right, 10);
    }

    #[test]
    fn test_insets_horizontal_vertical() {
        let insets = Insets::new(20, 10);
        assert_eq!(insets.horizontal(), 40); // 20 + 20
        assert_eq!(insets.vertical(), 20);   // 10 + 10
    }

    #[test]
    fn test_insets_addition() {
        let insets1 = Insets::uniform(10);
        let insets2 = Insets::uniform(5);
        let result = insets1.add(insets2);

        assert_eq!(result.top, 15);
        assert_eq!(result.bottom, 15);
        assert_eq!(result.left, 15);
        assert_eq!(result.right, 15);
    }

    #[test]
    fn test_insets_subtraction() {
        let insets1 = Insets::uniform(10);
        let insets2 = Insets::uniform(3);
        let result = insets1.subtract(insets2);

        assert_eq!(result.top, 7);
        assert_eq!(result.bottom, 7);
        assert_eq!(result.left, 7);
        assert_eq!(result.right, 7);
    }
}
