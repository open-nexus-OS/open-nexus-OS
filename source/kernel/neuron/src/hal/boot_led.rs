// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The boot LED (TASK-0260B P3): on a board without a serial adapter, the user LED is
//! the one channel the kernel's earliest phase has — the boot trace (RFC-0107) starts only when
//! the block owner runs, and the board's reset scrubs the console ring in RAM. At milestone `k`
//! the kernel pulses the LED, then pauses: the last group a person sees is the last milestone
//! the kernel reached. Encoding: `k / 5` long pulses (600 ms), then `k % 5` short pulses
//! (120 ms), then a 700 ms pause — long counts five, short counts one. A fatal stop (panic)
//! flickers the LED forever (60 ms on/off), unmistakable next to any group.
//! Milestones: 1 platform from the tree · 2 high half · 3 kmain, handoff captured · 4 trap
//! runtime installed · 5 secondary harts gated · 6 kernel selftests begin · 7 child task ran,
//! exited, was waited · 8 init's address space created · 9 init's segments copied · 10 init
//! spawned · 11 runtime begins. The tree names the LED (`/chosen/nexus,boot-led`, a bank and
//! line of a GPIO block; RFC-0098: never an address in code); the loader brought the block up,
//! muxed the pad and set the direction, so the kernel only sets and clears the line
//! (mainline v6.16 `gpio-spacemit-k1`: banks at 0x0/0x4/0x8/0x100, GPSR 0x18, GPCR 0x24).
//! Without a LED in the tree (QEMU) every milestone is a no-op. Pure timing: busy waits on
//! `time`, before the timer exists; ~25 s over the whole ladder, on the board only.
//! OWNERS: @kernel-team @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the tree side in nexus-fdt's goldens; the pulses on the desk board (a person)

use core::sync::atomic::{AtomicUsize, Ordering};

/// The GPIO block's window (0 = no LED) and its length: the runtime kernel table maps it
/// like the console's (`mm::kernel_layout`) — the boot table's gigabyte leaves covered it by
/// accident, and the first pulse after the switch faulted on an unmapped window.
static BASE: AtomicUsize = AtomicUsize::new(0);
static LEN: AtomicUsize = AtomicUsize::new(0);
/// The bank's register base inside the window and the line's bit.
static BANK_OFF: AtomicUsize = AtomicUsize::new(0);
static BIT: AtomicUsize = AtomicUsize::new(0);

const BANK_BASE: [usize; 4] = [0x0, 0x4, 0x8, 0x100];
const GPSR: usize = 0x18;
const GPCR: usize = 0x24;
/// A short pulse counts one, a long pulse counts five.
const SHORT_MS: u64 = 120;
const LONG_MS: u64 = 600;
const OFF_MS: u64 = 200;
const GAP_MS: u64 = 700;
const FLICKER_MS: u64 = 60;

/// Reads the LED the tree names; nothing without one.
pub fn init(bytes: Option<&[u8]>) {
    let Some(bytes) = bytes else { return };
    let Ok(fdt) = nexus_fdt::Fdt::new(bytes) else { return };
    let Some(led) = fdt.chosen().ok().and_then(|c| c.boot_led()) else { return };
    let Some(reg) = led.gpio.reg(0).ok().flatten() else { return };
    let (Some(&bank), true) = (BANK_BASE.get(led.bank as usize), led.line < 32) else { return };
    BANK_OFF.store(bank, Ordering::Relaxed);
    BIT.store(1usize << led.line, Ordering::Relaxed);
    LEN.store(reg.size as usize, Ordering::Relaxed);
    BASE.store(reg.addr as usize, Ordering::Release);
}

/// The GPIO block's window `(base, len)` for the kernel table, `None` without a LED.
pub fn window() -> Option<(usize, usize)> {
    let base = BASE.load(Ordering::Acquire);
    (base != 0).then(|| (base, LEN.load(Ordering::Relaxed).max(0x1000)))
}

/// Whether the tree named a LED and the timebase the pulses need is known.
pub fn present() -> bool {
    BASE.load(Ordering::Acquire) != 0 && super::platform::timebase_hz() != 0
}

fn write(reg: usize, value: u32) {
    let base = BASE.load(Ordering::Acquire);
    if base == 0 {
        return;
    }
    let addr = crate::phys::phys_to_virt(base + BANK_OFF.load(Ordering::Relaxed) + reg);
    // SAFETY: a register of the GPIO window the tree names, mapped in both phases.
    unsafe { core::ptr::write_volatile(addr as *mut u32, value) };
}

fn wait_ms(ms: u64) {
    let ticks = super::platform::timebase_hz() / 1000 * ms;
    let start = crate::arch::riscv::read_time();
    while crate::arch::riscv::read_time().wrapping_sub(start) < ticks {
        core::hint::spin_loop();
    }
}

fn pulse(bit: u32, on_ms: u64) {
    write(GPSR, bit);
    wait_ms(on_ms);
    write(GPCR, bit);
    wait_ms(OFF_MS);
}

/// Milestone `k`: `k / 5` long pulses, `k % 5` short ones, then a pause. A no-op without a LED.
/// Every milestone also stamps the console with the time since the counter's zero — on the
/// board the one clock the ring keeps, so a boot cut short by a reset reads as a duration.
pub fn milestone(k: u32) {
    let hz = super::platform::timebase_hz();
    if hz != 0 {
        let ms = crate::arch::riscv::read_time() / (hz / 1000);
        log_info!(target: "boot", "KINIT: milestone {} at {} ms (uart stalls={})", k, ms,
            super::platform::uart_stalls());
    }
    if !present() {
        return;
    }
    let bit = BIT.load(Ordering::Relaxed) as u32;
    for _ in 0..k / 5 {
        pulse(bit, LONG_MS);
    }
    for _ in 0..k % 5 {
        pulse(bit, SHORT_MS);
    }
    wait_ms(GAP_MS);
}

/// A fatal stop: the LED flickers until the board is reset. Returns at once without a LED,
/// so the caller's park (WFI) is what a QEMU panic still does.
pub fn fatal() {
    if !present() {
        return;
    }
    let bit = BIT.load(Ordering::Relaxed) as u32;
    loop {
        write(GPSR, bit);
        wait_ms(FLICKER_MS);
        write(GPCR, bit);
        wait_ms(FLICKER_MS);
    }
}
