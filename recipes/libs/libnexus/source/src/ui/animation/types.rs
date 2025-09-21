//! Animation types and constants
//! Common types used throughout the animation system

use std::time::Duration;

/// Animation duration in milliseconds
pub type AnimationDuration = u32;

/// Animation progress value (0.0 to 1.0)
pub type AnimationProgress = f32;

/// Frame delta time in milliseconds
pub type FrameDelta = u32;

/// Animation ID for tracking multiple animations
pub type AnimationId = usize;

/// Animation constants
pub mod constants {
    /// Target frames per second for animations
    pub const TARGET_FPS: u32 = 60;

    /// Frame duration in milliseconds for 60fps
    pub const FRAME_DURATION_MS: u32 = 1000 / TARGET_FPS;

    /// Default animation duration in milliseconds
    pub const DEFAULT_ANIMATION_DURATION_MS: u32 = 250;

    /// Minimum animation duration in milliseconds
    pub const MIN_ANIMATION_DURATION_MS: u32 = 16; // ~1 frame

    /// Maximum animation duration in milliseconds
    pub const MAX_ANIMATION_DURATION_MS: u32 = 5000; // 5 seconds
}

/// Animation state
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AnimationState {
    /// Animation is not running
    Idle,
    /// Animation is currently running
    Running,
    /// Animation is paused
    Paused,
    /// Animation has completed
    Completed,
}

/// Animation direction
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AnimationDirection {
    /// Animation is opening/expanding (0.0 -> 1.0)
    In,
    /// Animation is closing/collapsing (1.0 -> 0.0)
    Out,
}

/// Animation completion callback
pub type AnimationCallback = Box<dyn FnOnce() + Send + Sync>;

/// Animation frame callback (called each frame during animation)
pub type FrameCallback = Box<dyn FnMut(AnimationProgress) + Send + Sync>;

/// Animation configuration
pub struct AnimationConfig {
    /// Animation duration in milliseconds
    pub duration_ms: AnimationDuration,
    /// Animation direction
    pub direction: AnimationDirection,
    /// Whether to loop the animation
    pub loop_animation: bool,
    /// Callback to call when animation completes
    pub completion_callback: Option<AnimationCallback>,
    /// Callback to call each frame
    pub frame_callback: Option<FrameCallback>,
}

impl Clone for AnimationConfig {
    fn clone(&self) -> Self {
        Self {
            duration_ms: self.duration_ms,
            direction: self.direction,
            loop_animation: self.loop_animation,
            completion_callback: None, // Callbacks can't be cloned
            frame_callback: None,      // Callbacks can't be cloned
        }
    }
}

impl Default for AnimationConfig {
    fn default() -> Self {
        Self {
            duration_ms: constants::DEFAULT_ANIMATION_DURATION_MS,
            direction: AnimationDirection::Out,
            loop_animation: false,
            completion_callback: None,
            frame_callback: None,
        }
    }
}

/// Animation result
#[derive(Debug, Clone, PartialEq)]
pub enum AnimationResult {
    /// Animation completed successfully
    Completed,
    /// Animation was cancelled
    Cancelled,
    /// Animation encountered an error
    Error(String),
}

/// Animation metadata
#[derive(Debug, Clone)]
pub struct AnimationMetadata {
    /// Unique animation ID
    pub id: AnimationId,
    /// Animation name (for debugging)
    pub name: String,
    /// When the animation was created
    pub created_at: std::time::Instant,
    /// When the animation started
    pub started_at: Option<std::time::Instant>,
    /// When the animation completed
    pub completed_at: Option<std::time::Instant>,
}

impl AnimationMetadata {
    /// Create new animation metadata
    pub fn new(id: AnimationId, name: String) -> Self {
        Self {
            id,
            name,
            created_at: std::time::Instant::now(),
            started_at: None,
            completed_at: None,
        }
    }

    /// Mark animation as started
    pub fn mark_started(&mut self) {
        self.started_at = Some(std::time::Instant::now());
    }

    /// Mark animation as completed
    pub fn mark_completed(&mut self) {
        self.completed_at = Some(std::time::Instant::now());
    }

    /// Get total animation duration
    pub fn total_duration(&self) -> Option<Duration> {
        self.started_at.and_then(|start| {
            self.completed_at.map(|end| end.duration_since(start))
        })
    }
}
