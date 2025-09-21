//! Animation timer for consistent 60fps updates
//! This provides a dedicated timer system independent of the UI event loop

use std::time::{Duration, Instant};
use std::sync::{Arc, Mutex};
use std::thread;
use std::sync::atomic::{AtomicBool, Ordering};

/// Animation timer that runs at 60fps (16.67ms intervals)
/// This ensures smooth animations regardless of UI event loop performance
pub struct AnimationTimer {
    /// Whether the timer is currently running
    running: Arc<AtomicBool>,
    /// Callback function to call on each frame
    callback: Arc<Mutex<Option<Box<dyn FnMut() + Send + Sync>>>>,
}

impl AnimationTimer {
    /// Create a new animation timer
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            callback: Arc::new(Mutex::new(None)),
        }
    }

    /// Set the callback function to call on each frame
    pub fn set_callback<F>(&mut self, callback: F)
    where
        F: FnMut() + Send + Sync + 'static,
    {
        let mut cb = self.callback.lock().unwrap();
        *cb = Some(Box::new(callback));
    }

    /// Start the animation timer at 60fps
    pub fn start(&mut self) {
        if self.running.load(Ordering::Relaxed) {
            return; // Already running
        }

        self.running.store(true, Ordering::Relaxed);
        let running = Arc::clone(&self.running);
        let callback = Arc::clone(&self.callback);

        // Spawn timer thread
        thread::spawn(move || {
            const TARGET_FPS: u64 = 60;
            const FRAME_DURATION: Duration = Duration::from_nanos(1_000_000_000 / TARGET_FPS);

            let mut last_frame = Instant::now();

            while running.load(Ordering::Relaxed) {
                let now = Instant::now();
                let _frame_time = now.duration_since(last_frame);

                // Call the animation callback
                {
                    let mut cb = callback.lock().unwrap();
                    if let Some(ref mut callback_fn) = *cb {
                        callback_fn();
                    }
                }

                // Calculate sleep time to maintain 60fps
                let elapsed = Instant::now().duration_since(now);
                if elapsed < FRAME_DURATION {
                    let sleep_time = FRAME_DURATION - elapsed;
                    thread::sleep(sleep_time);
                }

                last_frame = Instant::now();
            }
        });
    }

    /// Stop the animation timer
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
    }

    /// Check if the timer is currently running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }
}

impl Default for AnimationTimer {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for AnimationTimer {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn test_timer_creation() {
        let timer = AnimationTimer::new();
        assert!(!timer.is_running());
    }

    #[test]
    fn test_timer_callback() {
        let mut timer = AnimationTimer::new();
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = Arc::clone(&counter);

        timer.set_callback(move || {
            counter_clone.fetch_add(1, Ordering::Relaxed);
        });

        timer.start();

        // Give it some time to run
        thread::sleep(Duration::from_millis(100));

        timer.stop();

        // Should have called the callback multiple times
        assert!(counter.load(Ordering::Relaxed) > 0);
    }
}
