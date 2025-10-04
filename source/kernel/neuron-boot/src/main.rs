// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Boot wrapper for the NEURON kernel. Provides a minimal `_start` entry
//! point that performs the early machine setup before handing execution to
//! the kernel library via `neuron::kmain()`.
#![no_std]
#![no_main]

use neuron::kmain;

#[export_name = "_start"]
pub extern "C" fn start() -> ! {
    // SAFETY: Early boot runs before the Rust runtime. The kernel guarantees
    // that only a single core executes this path, so calling the raw
    // initialisation routine is sound here.
    unsafe { neuron::early_boot_init() };
    kmain()
}
