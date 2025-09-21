//! Animation system - now using libnexus animation engine
//! This provides a clean interface to the centralized animation system

// Re-export libnexus animation system
pub use libnexus::{
    Timeline,
    Direction,
    Easing,
    AnimationDuration,
    AnimationProgress,
    AnimationState,
    AnimationConfig,
    AnimationMetadata,
    AnimationResult,
    ui::constants,
};

// Compatibility wrapper for existing code
pub fn ease(easing: Easing, t: f32) -> f32 {
    easing.apply(t)
}
