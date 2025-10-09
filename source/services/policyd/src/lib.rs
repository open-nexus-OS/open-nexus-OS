// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Minimal policyd stub daemon.
//!
//! The stub announces readiness, attempts a best-effort registration with
//! `samgr` (when available), then idles in a low-CPU loop. Policy RPCs are not
//! implemented yet and would return NotImplemented once wired.

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

/// Runs the daemon main loop.
pub fn service_main_loop(notifier: ReadyNotifier) -> Result<(), ()> {
    let _ = try_register_with_samgr();
    notifier.notify();
    println!("policyd: ready");
    idle()
}

fn idle() -> Result<(), ()> {
    let tick = std::time::Duration::from_millis(100);
    loop {
        std::thread::park_timeout(tick);
        std::thread::yield_now();
    }
}

/// Best-effort registration with `samgr`. No-op if client not yet available.
fn try_register_with_samgr() -> Result<(), ()> {
    Ok(())
}
