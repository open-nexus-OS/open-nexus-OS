// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: gpud no-alloc UART stat emitters (present stats window, present
//! deadline NACK marker, handoff timing) — split out of `service.rs`. gpud
//! runs on a non-freeing bump allocator, so every line here is formatted into
//! a stack buffer (no `format!`/heap on the present hot path).
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Lines proven via QEMU uart.log (`gpud: present us …`,
//!   `gpud: FAIL present deadline`, `gpud: timing handoff_to_ready_ms=`).

use nexus_abi::debug_write;

/// Fixed-capacity ASCII line builder (truncating, no alloc).
struct DecLine<const N: usize> {
    buf: [u8; N],
    len: usize,
}

impl<const N: usize> DecLine<N> {
    fn new() -> Self {
        Self { buf: [0u8; N], len: 0 }
    }

    fn put(&mut self, s: &[u8]) {
        for &b in s {
            if self.len < N {
                self.buf[self.len] = b;
                self.len += 1;
            }
        }
    }

    fn put_dec(&mut self, mut v: u32) {
        let mut tmp = [0u8; 10];
        let mut n = 0usize;
        loop {
            tmp[n] = b'0' + (v % 10) as u8;
            n += 1;
            v /= 10;
            if v == 0 {
                break;
            }
        }
        while n > 0 {
            n -= 1;
            if self.len < N {
                self.buf[self.len] = tmp[n];
                self.len += 1;
            }
        }
    }

    fn emit(&self) {
        let _ = debug_write(&self.buf[..self.len]);
    }
}

/// One-shot boot diagnostic: wall-clock of the framebuffer handoff →
/// display-ready processing (attach backing + GL scanout + first
/// textured/wallpaper present). This is the `tail_ms` the init boot table
/// attributes to the display chain; emitting it here localizes whether that
/// time is gpud's GL work vs gpud blocked waiting on present completion.
pub(crate) fn emit_handoff_timing(ms: u32) {
    let mut l = DecLine::<48>::new();
    l.put(b"gpud: timing handoff_to_ready_ms=");
    l.put_dec(ms);
    l.put(b"\n");
    l.emit();
}

/// P0.3 honest-present marker: `gpud: FAIL present deadline (cmd=N)` — N
/// completion waits ran into the `GPU_WAIT_DEADLINE_NS` net during ONE present,
/// so its commands were abandoned by the ring's degraded recovery and the frame
/// is (partially) lost. The present is NACKed; windowd requeues the damage.
#[cfg(all(feature = "os-lite", target_os = "none"))]
pub(crate) fn emit_present_deadline_fail(expired: u32) {
    let mut l = DecLine::<64>::new();
    l.put(b"gpud: FAIL present deadline (cmd=");
    l.put_dec(expired);
    l.put(b")\n");
    l.emit();
}

/// Present-stats window line. `win_ms` is the window's wall-clock: n presents
/// over win_ms → the actual present RATE (120 presents / ~1000ms = a healthy
/// 120Hz train; a much larger win_ms during continuous drag = the cadence is
/// starving upstream of the GPU). `irqw`/`dlx` report reactive-completion
/// health: waits woken by the GPU ring-buffer IRQ vs waits that ran into the
/// 500ms deadline net — a healthy boot has dlx=0; dlx climbing while irqw
/// stays flat = the IRQ path is wedged/unbound again.
pub(crate) fn emit_present_stats(avg_us: u32, max_us: u32, n: u32, win_ms: u32) {
    let mut l = DecLine::<128>::new();
    l.put(b"gpud: present us avg=");
    l.put_dec(avg_us);
    l.put(b" max=");
    l.put_dec(max_us);
    l.put(b" n=");
    l.put_dec(n);
    l.put(b" win_ms=");
    l.put_dec(win_ms);
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    {
        l.put(b" irqw=");
        l.put_dec(crate::backend::IRQ_WAKE_COUNT.load(core::sync::atomic::Ordering::Relaxed));
        l.put(b" dlx=");
        l.put_dec(
            crate::backend::IRQ_DEADLINE_EXPIRED_COUNT.load(core::sync::atomic::Ordering::Relaxed),
        );
    }
    l.put(b"\n");
    l.emit();
}
