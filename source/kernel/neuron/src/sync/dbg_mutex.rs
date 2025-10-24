// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Debug-only mutex wrapper with simple lockdep-style checks
//! OWNERS: @kernel-sync-team
//! PUBLIC API: DbgMutex::new(), DbgMutex::lock()
//! DEPENDS_ON: spin::Mutex, riscv time CSR (OS)
//! INVARIANTS: Detect double-lock; warn on long holds; debug-only
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

#![allow(dead_code)]

use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Minimal time source wrapper (10 MHz on QEMU virt).
#[inline(always)]
fn read_time() -> u64 {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        riscv::register::time::read() as u64
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        0
    }
}

/// A debug mutex that detects double-lock and long hold times.
pub struct DbgMutex<T> {
    inner: spin::Mutex<T>,
    held: AtomicBool,
    start_ticks: AtomicU64,
}

impl<T> DbgMutex<T> {
    pub const fn new(value: T) -> Self {
        Self {
            inner: spin::Mutex::new(value),
            held: AtomicBool::new(false),
            start_ticks: AtomicU64::new(0),
        }
    }

    /// Locks the mutex, performing debug checks.
    pub fn lock(&self) -> DbgMutexGuard<'_, T> {
        if self.held.swap(true, Ordering::SeqCst) {
            crate::uart::write_line("LOCKDEP: double-lock detected");
            panic!("lockdep: double-lock");
        }
        self.start_ticks.store(read_time(), Ordering::SeqCst);
        let guard = self.inner.lock();
        DbgMutexGuard { parent: self, guard }
    }
}

pub struct DbgMutexGuard<'a, T> {
    parent: &'a DbgMutex<T>,
    guard: spin::MutexGuard<'a, T>,
}

impl<'a, T> Deref for DbgMutexGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.guard
    }
}

impl<'a, T> DerefMut for DbgMutexGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.guard
    }
}

impl<'a, T> Drop for DbgMutexGuard<'a, T> {
    fn drop(&mut self) {
        let start = self.parent.start_ticks.load(Ordering::SeqCst);
        let end = read_time();
        let delta = end.wrapping_sub(start);
        // Warn on very long holds in debug builds (~>50ms on 10MHz)
        const LONG_HOLD_TICKS: u64 = 500_000;
        if delta > LONG_HOLD_TICKS {
            crate::uart::write_line("LOCKDEP: long hold");
        }
        self.parent.held.store(false, Ordering::SeqCst);
        // guard drops automatically
    }
}
