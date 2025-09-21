//! Redox OS kompatibler Animation-Timer
//!
//! Dieser Timer läuft in einem eigenen Thread und ist unabhängig vom Event-Loop.
//! Er bietet 30fps für QEMU-Performance und ist einfach zu implementieren.

use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const FPS: u64 = 30; // 30fps für QEMU-Performance
const FRAME_DURATION: Duration = Duration::from_millis(1000 / FPS);

/// Redox-kompatibler Animation-Timer
///
/// Läuft in eigenem Thread und ist unabhängig vom Event-Loop.
/// Bietet kontinuierliche 30fps Updates für Animationen.
pub struct RedoxAnimationTimer {
    /// Timer läuft
    is_running: Arc<AtomicBool>,
    /// Callback-Funktion für jeden Frame
    callback: Arc<Mutex<Option<Box<dyn Fn() + Send + 'static>>>>,
    /// Thread-Handle
    thread_handle: Option<thread::JoinHandle<()>>,
}

impl RedoxAnimationTimer {
    /// Erstelle neuen Timer
    pub fn new() -> Self {
        Self {
            is_running: Arc::new(AtomicBool::new(false)),
            callback: Arc::new(Mutex::new(None)),
            thread_handle: None,
        }
    }

    /// Setze Callback-Funktion für jeden Frame
    pub fn set_callback<F>(&mut self, callback: F)
    where
        F: Fn() + Send + 'static,
    {
        *self.callback.lock().unwrap() = Some(Box::new(callback));
    }

    /// Starte Timer (30fps)
    pub fn start(&mut self) {
        if self.is_running.load(Ordering::SeqCst) {
            return;
        }

        self.is_running.store(true, Ordering::SeqCst);
        let is_running_clone = Arc::clone(&self.is_running);
        let callback_clone = Arc::clone(&self.callback);

        let handle = thread::spawn(move || {
            let mut last_frame_time = Instant::now();

            while is_running_clone.load(Ordering::SeqCst) {
                let now = Instant::now();
                let elapsed = now.duration_since(last_frame_time);

                if elapsed >= FRAME_DURATION {
                    // Callback aufrufen
                    if let Some(callback_fn) = callback_clone.lock().unwrap().as_ref() {
                        callback_fn();
                    }
                    last_frame_time = now;
                } else {
                    // Sleep für verbleibende Zeit
                    thread::sleep(FRAME_DURATION - elapsed);
                }
            }
        });

        self.thread_handle = Some(handle);
    }

    /// Stoppe Timer
    pub fn stop(&mut self) {
        if self.is_running.load(Ordering::SeqCst) {
            self.is_running.store(false, Ordering::SeqCst);
            if let Some(handle) = self.thread_handle.take() {
                let _ = handle.join(); // Ignoriere Fehler beim Join
            }
        }
    }

    /// Prüfe ob Timer läuft
    pub fn is_running(&self) -> bool {
        self.is_running.load(Ordering::SeqCst)
    }
}

impl Default for RedoxAnimationTimer {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for RedoxAnimationTimer {
    fn drop(&mut self) {
        self.stop();
    }
}
