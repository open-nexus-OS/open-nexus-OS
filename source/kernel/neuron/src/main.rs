// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Temporary main entry point for standalone NEURON kernel binary.
//! This allows QEMU to load the kernel until proper userland services are implemented.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
#![deny(warnings)]
#![forbid(unsafe_op_in_unsafe_fn)]

#[cfg_attr(test, allow(unused_extern_crates))]
extern crate alloc;

// Import the neuron library
use neuron::boot;

/// Entry point for standalone kernel binary.
/// This simply calls the existing boot sequence from the neuron library.
#[cfg_attr(not(test), no_mangle)]
pub extern "C" fn _start() -> ! {
    // Call the existing boot sequence
    boot::_start()
}
