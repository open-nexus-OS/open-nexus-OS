// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_op_in_unsafe_fn)]
#![deny(warnings)]

//! NEURON microkernel crate.
//!
//! The crate bundles architecture specific early boot code, a tiny
//! hardware abstraction layer for the RISC-V `virt` machine and core
//! kernel subsystems such as scheduling, capability handling and the
//! syscall dispatcher.  All modules are intentionally compact and
//! thoroughly documented to make host-first testing feasible.

extern crate alloc;

pub mod arch;
pub mod boot;
pub mod cap;
pub mod hal;
pub mod ipc;
pub mod kmain;
pub mod mm;
pub mod sched;
pub mod syscall;
pub mod trap;
pub mod uart;

/// Kernel banner printed during boot on the first UART.
pub const BANNER: &str = "NEURON";

#[cfg(test)]
mod tests {
    use super::ipc::header::MessageHeader;
    use static_assertions::const_assert_eq;

    #[test]
    fn header_layout() {
        const_assert_eq!(core::mem::size_of::<MessageHeader>(), 16);
    }
}
