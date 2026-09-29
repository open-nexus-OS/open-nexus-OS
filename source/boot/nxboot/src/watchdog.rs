// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The SoC watchdog, stopped by the loader (TASK-0260B P3). The vendor chain's U-Boot
//! starts the K1's watchdog at its init, feeds it while it runs and stops it before it boots the
//! kernel; the flash vehicle is that U-Boot, and its `reboot` IS the watchdog. Our chain never
//! runs U-Boot, so nothing stops a watchdog that is counting when nxboot starts — the kernel
//! then dies mid-line, seconds in, with no trap and no message (the first board boots with the
//! OS trace). The tree names the watchdog; the loader turns it off through the vendor driver's
//! register model (`spacemit_wdt.c` in the vendor's U-Boot: a write to `ENABLE` at +0xb8 needs
//! the two unlock words first, 0xbaba at +0xb0 and 0xeb10 at +0xb4) and reads the register back.
//! Diagnostics-grade honesty: the console says what happened; nothing here stops the boot.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the desk board (the kernel's ladder runs past its former stopping point)

extern crate alloc;

use alloc::format;

use crate::arch;
use crate::platform::Tree;

const WFAR: usize = 0xb0;
const WSAR: usize = 0xb4;
const ENABLE: usize = 0xb8;
const UNLOCK_FIRST: u32 = 0xbaba;
const UNLOCK_SECOND: u32 = 0xeb10;

/// Stops the watchdog the tree names, if any.
pub fn stop(t: &Tree) {
    let Some(node) = t.fdt.find_compatible(&["spacemit,k1x-wdt"]).next() else {
        arch::uart_puts("nxboot: watchdog none (no node)\n");
        return;
    };
    let Some(reg) = node.reg(0).ok().flatten() else {
        arch::uart_puts("nxboot: watchdog none (no window)\n");
        return;
    };
    let base = reg.addr as usize;
    let was = arch::mmio_read32(base + ENABLE);
    arch::mmio_write32(base + WFAR, UNLOCK_FIRST);
    arch::mmio_write32(base + WSAR, UNLOCK_SECOND);
    arch::mmio_write32(base + ENABLE, 0);
    let now = arch::mmio_read32(base + ENABLE);
    if now == 0 {
        arch::uart_puts(&format!("nxboot: watchdog off (0x{base:x} was=0x{was:x})\n"));
    } else {
        arch::uart_puts(&format!("nxboot: watchdog FAIL (0x{base:x} enable=0x{now:x})\n"));
    }
}
