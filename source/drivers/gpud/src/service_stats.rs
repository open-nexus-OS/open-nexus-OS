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

/// Present-phase split (present-cost triage): time spent ENQUEUEING the frame's
/// commands into the ring vs time spent in `ctrl_batch_end`'s single DRAIN
/// (which waits on the PRIOR batch's completion — pipelined presents). A
/// drain-dominated window = host-GL-bound (cut draw work per frame); an
/// enqueue-dominated window = guest-CPU-bound (cut command building). Written
/// by the virgl buildup present, read + reset per stats window here.
pub(crate) static PRESENT_ENQ_NS_SUM: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(0);
pub(crate) static PRESENT_DRAIN_NS_SUM: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(0);
pub(crate) static PRESENT_PHASE_COUNT: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(0);

/// Record one present's enqueue/drain durations (virgl buildup path).
#[cfg(feature = "virgl")]
pub(crate) fn note_present_phases(enq_ns: u64, drain_ns: u64) {
    use core::sync::atomic::Ordering;
    PRESENT_ENQ_NS_SUM.fetch_add(enq_ns, Ordering::Relaxed);
    PRESENT_DRAIN_NS_SUM.fetch_add(drain_ns, Ordering::Relaxed);
    PRESENT_PHASE_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Ring-entry telemetry: how many entries land per stats window and the most
/// expensive single enqueue (slot alloc + buffer write + doorbell). Localizes
/// whether the enqueue phase is many-cheap-entries (main-loop round trips) or
/// few-expensive ones (slot backpressure / a slow doorbell class).
pub(crate) static RING_ENTRY_COUNT: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(0);
pub(crate) static RING_ENTRY_MAX_NS: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(0);

pub(crate) fn note_ring_entry(ns: u64) {
    use core::sync::atomic::Ordering;
    RING_ENTRY_COUNT.fetch_add(1, Ordering::Relaxed);
    RING_ENTRY_MAX_NS.fetch_max(ns, Ordering::Relaxed);
}

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
    let mut l = DecLine::<160>::new();
    l.put(b"gpud: present us avg=");
    l.put_dec(avg_us);
    l.put(b" max=");
    l.put_dec(max_us);
    l.put(b" n=");
    l.put_dec(n);
    l.put(b" win_ms=");
    l.put_dec(win_ms);
    // Phase split (this window): average enqueue vs drain microseconds.
    {
        use core::sync::atomic::Ordering;
        let n = PRESENT_PHASE_COUNT.swap(0, Ordering::Relaxed).max(1);
        let enq = PRESENT_ENQ_NS_SUM.swap(0, Ordering::Relaxed) / n / 1000;
        let drain = PRESENT_DRAIN_NS_SUM.swap(0, Ordering::Relaxed) / n / 1000;
        l.put(b" enq_us=");
        l.put_dec(enq as u32);
        l.put(b" drain_us=");
        l.put_dec(drain as u32);
        let ents = RING_ENTRY_COUNT.swap(0, Ordering::Relaxed);
        let entmax = RING_ENTRY_MAX_NS.swap(0, Ordering::Relaxed) / 1000;
        l.put(b" ent=");
        l.put_dec(ents as u32);
        l.put(b" entmax_us=");
        l.put_dec(entmax as u32);
    }
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

/// Fold-immune diagnostic line: `<label> v0 v1 …` via the raw atomic
/// [`nexus_abi::debug_write`] syscall (bypasses verdict folding — these
/// fire only on device-reply anomalies and must never be swallowed).
/// Alloc-free: bounded stack buffer, decimal rendering.
pub(crate) fn raw_diag_line(label: &[u8], vals: &[u32]) {
    let mut buf = [0u8; 96];
    let mut p = 0usize;
    let put = |buf: &mut [u8; 96], p: &mut usize, s: &[u8]| {
        for &b in s {
            if *p < buf.len() {
                buf[*p] = b;
                *p += 1;
            }
        }
    };
    put(&mut buf, &mut p, label);
    for &v in vals {
        put(&mut buf, &mut p, b" ");
        let mut tmp = [0u8; 10];
        let mut n = 0;
        let mut v = v;
        loop {
            tmp[n] = b'0' + (v % 10) as u8;
            v /= 10;
            n += 1;
            if v == 0 {
                break;
            }
        }
        while n > 0 {
            n -= 1;
            put(&mut buf, &mut p, &tmp[n..=n]);
        }
    }
    put(&mut buf, &mut p, b"\n");
    let _ = nexus_abi::debug_write(&buf[..p]);
}
