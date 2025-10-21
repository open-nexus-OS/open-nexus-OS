// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Minimal init process responsible for launching core services and emitting
//! deterministic UART markers for the OS test harness.

#![forbid(unsafe_code)]
#![deny(clippy::all, missing_docs)]
#![allow(unexpected_cfgs)]

use nexus_init::touch_schemas;
use nexus_init::{service_main_loop, ReadyNotifier};

/// Entrypoint for the init binary. Delegates to the selected backend and keeps
/// the process alive once service bootstrapping finishes.
fn main() -> ! {
    #[cfg(not(all(nexus_env = "os", feature = "os-lite")))]
    touch_schemas();
    if let Err(err) = service_main_loop(ReadyNotifier::new(|| ())) {
        eprintln!("init: fatal error: {err}");
    }
    loop {
        core::hint::spin_loop();
    }
}
