// Simplified icon caching system for nexus-launcher
// This module provides a clean, easy-to-debug approach to icon caching

use std::collections::HashMap;
use orbclient::Color;
use orbimage::Image;
use orbimage::ResizeType;

/// Simple icon cache that stores icons by size
/// Much easier to debug than the complex resolution-based caching
pub struct SimpleIconCache {
    cache: HashMap<u32, Image>,
    base_icon: Option<Image>,
}

impl SimpleIconCache {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
            base_icon: None,
        }
    }

    /// Load an icon at a specific size
    /// This is much simpler than the complex DPI-aware caching
    pub fn get_icon(&mut self, size: u32) -> &Image {
        // Check if we already have this size cached
        if self.cache.contains_key(&size) {
            return self.cache.get(&size).unwrap();
        }

        // Load the base icon if we don't have it
        if self.base_icon.is_none() {
            self.base_icon = Some(self.load_base_icon());
        }

        // Resize the base icon to the requested size
        let base_icon = self.base_icon.as_ref().unwrap();
        let resized_icon = base_icon.resize(size, size, ResizeType::Lanczos3).unwrap();

        // Cache and return the resized icon
        self.cache.insert(size, resized_icon);
        self.cache.get(&size).unwrap()
    }

    /// Load the base icon (placeholder for now)
    fn load_base_icon(&self) -> Image {
        // Create a simple placeholder icon
        let mut pixels = Vec::new();
        for y in 0..32 {
            for x in 0..32 {
                let r = (x * 8) as u8;
                let g = (y * 8) as u8;
                let b = 128;
                let a = 255;
                pixels.push(Color::rgba(r, g, b, a));
            }
        }
        Image::from_data(32, 32, pixels.into()).unwrap()
    }

    /// Clear the cache (useful for debugging)
    pub fn clear(&mut self) {
        self.cache.clear();
        self.base_icon = None;
    }

    /// Get cache statistics (useful for debugging)
    pub fn stats(&self) -> (usize, Vec<u32>) {
        let sizes: Vec<u32> = self.cache.keys().cloned().collect();
        (self.cache.len(), sizes)
    }
}

impl Default for SimpleIconCache {
    fn default() -> Self {
        Self::new()
    }
}
