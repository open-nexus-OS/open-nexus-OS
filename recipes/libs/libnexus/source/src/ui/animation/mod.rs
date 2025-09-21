use super::easing::Easing;
use std::time::Instant;

// Animation sub-modules
pub mod timer;
pub mod redox_timer;
pub mod types;

// Re-export types from types module
pub use types::{
    AnimationDirection as Direction,
    AnimationDuration,
    AnimationProgress,
    AnimationState,
    AnimationConfig,
    AnimationMetadata,
    AnimationResult,
    AnimationId,
    constants,
};


/// Animation timeline for smooth transitions
///
/// This provides a high-level animation API that can be used by any UI component
/// to create smooth, configurable animations with proper easing.
#[derive(Clone)]
pub struct Timeline {
    /// Current animation value (0.0 to 1.0)
    pub value: AnimationProgress,
    /// Animation direction
    pub direction: Direction,
    /// Duration in milliseconds
    pub duration_ms: AnimationDuration,
    /// Easing function to use
    pub easing: Easing,
    /// Animation state
    pub state: AnimationState,
    /// Start time of current animation
    start_time: Option<Instant>,
    /// Animation configuration
    config: AnimationConfig,
    /// Animation metadata
    metadata: Option<AnimationMetadata>,
}

impl Timeline {
    /// Create a new timeline with default values
    pub fn new() -> Self {
        Self {
            value: 0.0,
            direction: Direction::Out,
            duration_ms: constants::DEFAULT_ANIMATION_DURATION_MS,
            easing: Easing::CubicOut,
            state: AnimationState::Idle,
            start_time: None,
            config: AnimationConfig::default(),
            metadata: None,
        }
    }

    /// Create a new timeline with custom duration and easing
    pub fn with_config(duration_ms: AnimationDuration, easing: Easing) -> Self {
        Self {
            value: 0.0,
            direction: Direction::Out,
            duration_ms,
            easing,
            state: AnimationState::Idle,
            start_time: None,
            config: AnimationConfig {
                duration_ms,
                direction: Direction::Out,
                ..Default::default()
            },
            metadata: None,
        }
    }

    /// Start animation in the specified direction
    pub fn start(&mut self, direction: Direction) {
        self.direction = direction;
        self.config.direction = direction;
        self.state = AnimationState::Running;
        self.start_time = Some(Instant::now());

        if let Some(ref mut metadata) = self.metadata {
            metadata.mark_started();
        }
    }

    /// Stop animation and set to final state
    pub fn stop(&mut self) {
        self.state = AnimationState::Completed;
        self.value = match self.direction {
            Direction::In => 1.0,
            Direction::Out => 0.0,
        };
        self.start_time = None;

        if let Some(ref mut metadata) = self.metadata {
            metadata.mark_completed();
        }

        // Call completion callback if set
        if let Some(callback) = self.config.completion_callback.take() {
            callback();
        }
    }

    /// Update animation based on elapsed time
    /// Returns true if animation is still running
    pub fn update(&mut self) -> bool {
        if self.state != AnimationState::Running {
            return false;
        }

        let start_time = match self.start_time {
            Some(t) => t,
            None => {
                self.state = AnimationState::Completed;
                return false;
            }
        };

        let elapsed = start_time.elapsed();
        let elapsed_ms = elapsed.as_millis() as u32;

        if elapsed_ms >= self.duration_ms {
            // Animation complete
            self.stop();
            return false;
        }

        // Calculate progress (0.0 to 1.0)
        let progress = elapsed_ms as f32 / self.duration_ms as f32;

        // Apply easing
        let eased_progress = self.easing.apply(progress);

        // Set value based on direction
        self.value = match self.direction {
            Direction::In => eased_progress,
            Direction::Out => 1.0 - eased_progress,
        };

        // Call frame callback if set
        if let Some(ref mut callback) = self.config.frame_callback {
            callback(self.value);
        }

        true
    }

    /// Check if animation is currently running
    pub fn is_running(&self) -> bool {
        self.state == AnimationState::Running
    }

    /// Check if animation is in progress (not at start or end)
    pub fn is_animating(&self) -> bool {
        self.state == AnimationState::Running && self.value > 0.0 && self.value < 1.0
    }

    /// Get current animation value (0.0 to 1.0)
    pub fn value(&self) -> AnimationProgress {
        self.value
    }

    /// Set animation to immediate state (no animation)
    pub fn set_immediate(&mut self, open: bool) {
        self.state = AnimationState::Completed;
        self.value = if open { 1.0 } else { 0.0 };
        self.direction = if open { Direction::In } else { Direction::Out };
        self.start_time = None;
    }

    /// Set duration in milliseconds
    pub fn set_duration(&mut self, duration_ms: AnimationDuration) {
        self.duration_ms = duration_ms;
        self.config.duration_ms = duration_ms;
    }

    /// Set easing function
    pub fn set_easing(&mut self, easing: Easing) {
        self.easing = easing;
    }
}

impl Default for Timeline {
    fn default() -> Self {
        Self::new()
    }
}

/// Animation manager for handling multiple animations
///
/// This provides a centralized way to manage animations across the application,
/// similar to how KDE/GNOME handle animations in their compositors.
pub struct AnimationManager {
    /// All active animations
    animations: Vec<Timeline>,
    /// Animation timer for 60fps updates
    timer: super::animation::timer::AnimationTimer,
    /// Next animation ID
    next_id: AnimationId,
    /// Whether the manager is running
    running: bool,
}

impl AnimationManager {
    /// Create a new animation manager
    pub fn new() -> Self {
        let manager = Self {
            animations: Vec::new(),
            timer: super::animation::timer::AnimationTimer::new(),
            next_id: 0,
            running: false,
        };

        // Timer callback will be set up when start() is called

        manager
    }

    /// Add a new animation timeline
    pub fn add_timeline(&mut self, mut timeline: Timeline) -> AnimationId {
        let id = self.next_id;
        self.next_id += 1;

        // Add metadata to timeline
        timeline.metadata = Some(AnimationMetadata::new(id, format!("animation_{}", id)));

        self.animations.push(timeline);
        id
    }

    /// Get mutable reference to animation by ID
    pub fn get_mut(&mut self, id: AnimationId) -> Option<&mut Timeline> {
        self.animations.iter_mut().find(|anim| {
            anim.metadata.as_ref().map(|m| m.id == id).unwrap_or(false)
        })
    }

    /// Start the animation manager (starts the timer)
    pub fn start(&mut self) {
        if !self.running {
            // For now, we'll use manual updates instead of the timer callback
            // This avoids unsafe code and complex synchronization
            self.running = true;
        }
    }

    /// Stop the animation manager (stops the timer)
    pub fn stop(&mut self) {
        if self.running {
            self.timer.stop();
            self.running = false;
        }
    }

    /// Update all animations (called manually from main loop)
    /// Returns true if any animation is still running
    pub fn update_all(&mut self) -> bool {
        let mut any_running = false;
        for animation in &mut self.animations {
            if animation.update() {
                any_running = true;
            }
        }
        // Remove completed animations
        self.animations.retain(|animation| animation.is_running());
        any_running
    }

    /// Check if any animation is currently running
    pub fn has_running_animations(&self) -> bool {
        self.animations.iter().any(|a| a.is_running())
    }

    /// Get number of active animations
    pub fn animation_count(&self) -> usize {
        self.animations.len()
    }

    /// Check if the animation manager is running
    pub fn is_running(&self) -> bool {
        self.running
    }
}

impl Default for AnimationManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_timeline_creation() {
        let timeline = Timeline::new();
        assert_eq!(timeline.value, 0.0);
        assert_eq!(timeline.direction, Direction::Out);
        assert!(!timeline.running);
    }

    #[test]
    fn test_timeline_immediate_set() {
        let mut timeline = Timeline::new();
        timeline.set_immediate(true);
        assert_eq!(timeline.value, 1.0);
        assert_eq!(timeline.direction, Direction::In);
        assert!(!timeline.running);
    }

    #[test]
    fn test_animation_manager() {
        let mut manager = AnimationManager::new();
        let timeline = Timeline::new();
        let id = manager.add_timeline(timeline);

        assert_eq!(manager.animation_count(), 1);
        assert!(manager.get_mut(id).is_some());
    }
}
