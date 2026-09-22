// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The ONE unsafe-bearing module of socd: volatile 32-bit access to the mapped
//! provider windows. Addresses are absolute mapped VAs (`Provider.base + offset`),
//! every window came from init's grant of a tree node's `reg`.

#![allow(unsafe_code)]

use nexus_hal::Bus;

pub struct MmioBus;

impl Bus for MmioBus {
    fn read(&self, addr: usize) -> u32 {
        // SAFETY: `addr` lies inside a window `mmio_map_auto` returned for a
        // granted device capability; a 4-byte aligned volatile read.
        unsafe { core::ptr::read_volatile(addr as *const u32) }
    }
    fn write(&self, addr: usize, value: u32) {
        // SAFETY: as above, a volatile write.
        unsafe { core::ptr::write_volatile(addr as *mut u32, value) }
    }
}
