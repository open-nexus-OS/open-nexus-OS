// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The stock side of `KINIT: cpuN speed (loop_us=…)` (TASK-0330 P0): the same two instructions
//! the kernel times on each hart (`addi`, `bnez`, one million times), run on the stock system
//! once per CPU, so the ratio of the two times is the ratio of the clocks.
//!
//! A static program without a C library: it speaks the stock kernel's syscalls directly, so it
//! needs nothing installed on the board. Built with the repo's pinned toolchain for the same
//! instruction set the kernel is built for (the loop's two instructions encode identically):
//!
//! ```bash
//! rustc --edition 2021 --target riscv64imac-unknown-none-elf -C opt-level=2 -C panic=abort \
//!     -C relocation-model=static docs/board/measurements/2026-11-10-smp-baseline/hartspeed.rs \
//!     -o /tmp/hartspeed
//! adb push /tmp/hartspeed /tmp/hartspeed && adb shell /tmp/hartspeed
//! ```
//!
//! Output, one line per CPU the program may run on:
//! `cpu3 loop_us=612 min_us=604 iters=1000000 left=0` — `loop_us` is the first run (what the
//! kernel line states: one cold run), `min_us` the fastest of five (the clock without the
//! stock system's interrupts).

#![no_std]
#![no_main]

use core::arch::asm;

const ITERS: u64 = 1_000_000;
const RUNS: usize = 5;
const MAX_CPUS: u64 = 64;

const SYS_WRITE: usize = 64;
const SYS_EXIT_GROUP: usize = 94;
const SYS_CLOCK_GETTIME: usize = 113;
const SYS_SCHED_SETAFFINITY: usize = 122;
const CLOCK_MONOTONIC: usize = 1;

#[repr(C)]
struct Timespec {
    sec: i64,
    nsec: i64,
}

fn syscall3(nr: usize, a0: usize, a1: usize, a2: usize) -> isize {
    let ret: isize;
    // SAFETY: a Linux system call; the arguments are plain values or pointers to live locals.
    unsafe {
        asm!("ecall", in("a7") nr, inlateout("a0") a0 => ret, in("a1") a1, in("a2") a2);
    }
    ret
}

fn now_ns() -> u64 {
    let mut ts = Timespec { sec: 0, nsec: 0 };
    syscall3(SYS_CLOCK_GETTIME, CLOCK_MONOTONIC, &mut ts as *mut Timespec as usize, 0);
    ts.sec as u64 * 1_000_000_000 + ts.nsec as u64
}

/// The measured loop: the kernel's two instructions, `ITERS` times. Returns (ns, what is left).
#[inline(never)]
fn timed_loop() -> (u64, u64) {
    let mut left = ITERS;
    let t0 = now_ns();
    // SAFETY: a register-only countdown; `left` is the one register written.
    unsafe {
        asm!("2: addi {n}, {n}, -1", "bnez {n}, 2b", n = inout(reg) left, options(nomem, nostack));
    }
    (now_ns().saturating_sub(t0), left)
}

struct Line {
    buf: [u8; 96],
    len: usize,
}

impl Line {
    fn text(&mut self, s: &str) {
        for &b in s.as_bytes() {
            if self.len < self.buf.len() {
                self.buf[self.len] = b;
                self.len += 1;
            }
        }
    }

    fn number(&mut self, mut n: u64) {
        let mut digits = [0u8; 20];
        let mut at = digits.len();
        loop {
            at -= 1;
            digits[at] = b'0' + (n % 10) as u8;
            n /= 10;
            if n == 0 {
                break;
            }
        }
        for &d in &digits[at..] {
            if self.len < self.buf.len() {
                self.buf[self.len] = d;
                self.len += 1;
            }
        }
    }

    fn flush(&mut self) {
        syscall3(SYS_WRITE, 1, self.buf.as_ptr() as usize, self.len);
        self.len = 0;
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut line = Line { buf: [0; 96], len: 0 };
    for cpu in 0..MAX_CPUS {
        let mask: u64 = 1 << cpu;
        // Pin this thread to one CPU; a CPU the system does not have is refused and skipped.
        if syscall3(SYS_SCHED_SETAFFINITY, 0, 8, &mask as *const u64 as usize) < 0 {
            continue;
        }
        let (first, left) = timed_loop();
        let mut min = first;
        for _ in 1..RUNS {
            min = min.min(timed_loop().0);
        }
        line.text("cpu");
        line.number(cpu);
        line.text(" loop_us=");
        line.number(first / 1000);
        line.text(" min_us=");
        line.number(min / 1000);
        line.text(" iters=");
        line.number(ITERS);
        line.text(" left=");
        line.number(left);
        line.text("\n");
        line.flush();
    }
    syscall3(SYS_EXIT_GROUP, 0, 0, 0);
    loop {}
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    syscall3(SYS_EXIT_GROUP, 1, 0, 0);
    loop {}
}
