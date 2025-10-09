// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Minimal keystored stub daemon.
//!
//! The stub prints a readiness marker, performs a best-effort registration
//! with `samgr` (when available), then idles in a low-CPU loop. Requests are
//! not handled yet and would return NotImplemented once wired.

#![forbid(unsafe_code)]

/// Notifies the init process that the daemon has completed its boot sequence.
pub struct ReadyNotifier(Box<dyn FnOnce() + Send>);

impl ReadyNotifier {
    /// Creates a notifier from the provided closure.
    pub fn new<F>(func: F) -> Self
    where
        F: FnOnce() + Send + 'static,
    {
        Self(Box::new(func))
    }

    /// Signals readiness to the caller.
    pub fn notify(self) {
        (self.0)();
    }
}

#[derive(Debug)]
pub struct Error;

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "keystored error")
    }
}

impl std::error::Error for Error {}

/// Runs the daemon main loop.
pub fn service_main_loop(notifier: ReadyNotifier) -> Result<(), Error> {
    let _ = try_register_with_samgr();
    notifier.notify();
    println!("keystored: ready");
    idle()
}

fn idle() -> Result<(), Error> {
    let tick = std::time::Duration::from_millis(100);
    loop {
        // TODO: Once IPC endpoint is assigned, block on recv with timeout and
        // reply NotImplemented. For now, avoid CPU busy-spinning.
        std::thread::park_timeout(tick);
        std::thread::yield_now();
    }
}

/// Best-effort registration with `samgr`. No-op if client not yet available.
fn try_register_with_samgr() -> Result<(), Error> {
    // Placeholder: wire to `samgr` client when available.
    Ok(())
}
