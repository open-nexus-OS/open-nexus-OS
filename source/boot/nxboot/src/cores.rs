// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The cores' idle policy before the OS owns idle (TASK-0260B P3). The K1's power
//! unit lets a core power itself down on `wfi`; the firmware chain the board ships leaves
//! that on, and the vendor OS plans around it with its own idle driver. Our kernel parks
//! its secondary harts in plain `wfi` — a parked core then vanishes from the bus, and the
//! next access that concerns it (measured: the boot hart reading a parked hart's PLIC
//! context) is answered by nothing; the hart dies without a trap and the board resets.
//! The mainline firmware clears the power-down bits of every core at cold boot; this
//! loader does the same, from the tree's APMU syscon (RFC-0098: the base from the tree,
//! the register map from the SoC's documented firmware). Markers:
//! `nxboot: cores awake (apmu=0x… cores=8 was=0x… now=0x…)` / `cores none (<why>)`.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the board (a boot that passes the PLIC context probe of every hart)

extern crate alloc;

use alloc::format;

use crate::arch;
use crate::platform::Tree;

/// Per-core idle configuration registers inside the APMU window (mainline OpenSBI,
/// `spacemit/common.h`: `PMU_AP_CORE{0..7}_IDLE_CFG`).
const CORE_IDLE_CFG: [usize; 8] = [0x124, 0x128, 0x160, 0x164, 0x304, 0x308, 0x30c, 0x310];
/// Gate the core clock (bit 0), power the core off (bit 1), mask its nIRQ/nFIQ (bits 3, 4)
/// on `wfi` — `PMU_AP_IDLE_PWRDOWN_K1_MASK`.
const PWRDOWN_MASK: u32 = (1 << 0) | (1 << 1) | (1 << 3) | (1 << 4);

/// Clears every core's power-down-on-`wfi` bits; nothing on a tree without the APMU.
pub fn keep_awake(t: &Tree) {
    let Some(node) = t.fdt.find_compatible(&["spacemit,k1-syscon-apmu"]).next() else {
        arch::uart_puts("nxboot: cores none (no apmu)\n");
        return;
    };
    let Some(reg) = node.reg(0).ok().flatten() else {
        arch::uart_puts("nxboot: cores none (no window)\n");
        return;
    };
    let (base, len) = (reg.addr as usize, reg.size as usize);
    if CORE_IDLE_CFG.iter().any(|&off| off + 4 > len) {
        arch::uart_puts(&format!("nxboot: cores none (window 0x{len:x} too small)\n"));
        return;
    }
    let (mut was, mut now) = (0u32, 0u32);
    for &off in &CORE_IDLE_CFG {
        let v = arch::mmio_read32(base + off);
        was |= v & PWRDOWN_MASK;
        arch::mmio_write32(base + off, v & !PWRDOWN_MASK);
        now |= arch::mmio_read32(base + off) & PWRDOWN_MASK;
    }
    let n = CORE_IDLE_CFG.len();
    if now == 0 {
        arch::uart_puts(&format!(
            "nxboot: cores awake (apmu=0x{base:x} cores={n} was=0x{was:x})\n"
        ));
    } else {
        arch::uart_puts(&format!("nxboot: cores FAIL (apmu=0x{base:x} still=0x{now:x})\n"));
    }
}
