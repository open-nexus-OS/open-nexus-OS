// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The boot LED (TASK-0260B P3): on a board without a serial adapter, the user LED is
//! the one channel the boot's earliest phase has. The tree names it (`/chosen/nexus,boot-led`,
//! a bank and line of a GPIO block, and the pad's mux register with the value the stock system
//! runs it with). The loader brings the block up through the SoC glue (its clocks, RFC-0106's
//! loader clause), muxes the pad, sets the line's direction and lights it for two seconds — "the
//! loader is alive" — so the channel itself is proven before the kernel's ladder begins (one pulse per
//! milestone, `neuron::hal::boot_led`). Register model: mainline v6.16 `gpio-spacemit-k1`
//! (banks at 0x0/0x4/0x8/0x100; GSDR 0x54 sets direction out, GPSR 0x18 sets, GPCR 0x24
//! clears). Diagnostics only: nothing here stops the boot.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the tree side in nexus-fdt's goldens; the pulses on the desk board (a person)

extern crate alloc;

use alloc::format;

use nexus_soc::{bring_up, BringUpError, Providers};

use crate::arch;
use crate::platform::Tree;
use crate::probe::ArchBus;

const BANK_BASE: [usize; 4] = [0x0, 0x4, 0x8, 0x100];
const GSDR: usize = 0x54;
const GPSR: usize = 0x18;
const GPCR: usize = 0x24;
const LOADER_ON_MS: u64 = 2000;
const GAP_MS: u64 = 700;

fn wait_ms(hz: u64, ms: u64) {
    let ticks = hz / 1000 * ms;
    let start = arch::time_ticks();
    while arch::time_ticks().wrapping_sub(start) < ticks {
        core::hint::spin_loop();
    }
}

fn none(why: &str) {
    arch::uart_puts(&format!("nxboot: boot led none ({why})\n"));
}

/// The LED the tree names, brought up and pulsed twice.
pub fn prepare(t: &Tree) {
    let Some(chosen) = t.fdt.chosen().ok() else { return none("no chosen") };
    let Some(led) = chosen.boot_led() else { return none("no led") };
    let Some(reg) = led.gpio.reg(0).ok().flatten() else { return none("no gpio window") };
    let (Some(&bank), true) = (BANK_BASE.get(led.bank as usize), led.line < 32) else {
        return none("bank or line");
    };
    let hz = t.fdt.cpus().ok().map(|c| u64::from(c.timebase_hz)).unwrap_or(0);
    if hz == 0 {
        return none("no timebase");
    }
    let providers = Providers::from_tree(&t.fdt, |node| {
        node.reg(0).ok().flatten().and_then(|reg| usize::try_from(reg.addr).ok())
    });
    if let Err(error) = bring_up(led.gpio, &providers, &ArchBus::UNTRANSLATED) {
        return none(match error {
            BringUpError::Plan(_) => "soc glue unsupported",
            BringUpError::Fault(_) => "soc glue failed",
        });
    }
    // The pad's register lies behind the pad controller, which has clocks and a reset of its
    // own (RFC-0106): that node first, then the measured value.
    if let Some((pad, value)) = chosen.boot_led_pad() {
        if let Some(pinctrl) = t.fdt.find_compatible(&["spacemit,k1-pinctrl"]).next() {
            if let Err(error) = bring_up(pinctrl, &providers, &ArchBus::UNTRANSLATED) {
                return none(match error {
                    BringUpError::Plan(_) => "pad glue unsupported",
                    BringUpError::Fault(_) => "pad glue failed",
                });
            }
        }
        arch::mmio_write32(pad as usize, value);
    }
    let base = reg.addr as usize + bank;
    let bit = 1u32 << led.line;
    arch::mmio_write32(base + GSDR, bit);
    // The loader's signal: on for two seconds — long enough to be seen after a reset — then a
    // pause before the kernel's first group.
    arch::mmio_write32(base + GPSR, bit);
    wait_ms(hz, LOADER_ON_MS);
    arch::mmio_write32(base + GPCR, bit);
    wait_ms(hz, GAP_MS);
    arch::uart_puts(&format!(
        "nxboot: boot led ok (gpio=0x{:x} bank={} line={})\n",
        reg.addr, led.bank, led.line
    ));
}
