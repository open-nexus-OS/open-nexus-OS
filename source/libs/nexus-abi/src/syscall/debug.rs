// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Debug/observability — boot-mode knobs, verdict folding, markers, trace, Span, UART debug output
//! (Mechanical split out of the former lib.rs monolith — ADR-0051 hygiene
//! pass; behavior and syscall IDs unchanged.)

#[cfg(nexus_env = "os")]
use super::*;
/// True when the kernel resolved an INTERACTIVE boot (`SYSCALL_BOOT_MODE` → 1), so a U-mode service
/// should fold its boot markers into a `<service> N/N` verdict. Proof/unknown and host return
/// `false` (raw markers, keeping `verify-uart` deterministic). Lets every service share the kernel's
/// fw_cfg-derived mode without mapping fw_cfg itself — the keystone for per-service verdict folding.
#[must_use]
pub fn boot_should_fold_verdicts() -> bool {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_BOOT_MODE: usize = 45;
        let raw = unsafe { ecall0(SYSCALL_BOOT_MODE) };
        decode_syscall(raw).map(|v| v == 1).unwrap_or(false)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        false
    }
}

/// The fw_cfg-configured display mode (RFC-0074 / ADR-0050) as `(w, h)`, or `None` when
/// unknown/absent/host. The display server treats this as the AUTHORITATIVE mode it commands
/// onto the scanout — kernel-derived, so QEMU's transient GTK window size never latches wrong.
/// (ABI monolith reduction deferred to a separate task — this stays inline for now.)
#[must_use]
pub fn boot_display_mode() -> Option<(u32, u32)> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        // SYSCALL_BOOT_DISPLAY_MODE (50) → packed `w | (h << 16)`, 0 = unknown.
        let packed = decode_syscall(unsafe { ecall0(50) }).unwrap_or(0);
        let (w, h) = ((packed & 0xFFFF) as u32, (packed >> 16) as u32);
        (w > 0 && h > 0).then_some((w, h))
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        None
    }
}

// ── Per-process service verdict (alloc-free) ─────────────────────────────────────────────────
// In an interactive boot a service folds its routine boot markers into one `[ts] OK <service> N/N
// <ms>` grid line (the same form the kernel emits for `kself`). Fold mode is set once at service
// bootstrap from [`boot_should_fold_verdicts`]; proof boots never fold, so `verify-uart` still sees
// every raw marker. Counters only — no heap, no replay buffer. A FAILED marker is counted and
// printed live; routine markers are suppressed. The flush is paired with the tally so nothing is
// dropped without a verdict; after the flush, folding stops (later runtime markers print raw).
// Lives in nexus-abi (the universal dep) so any service can use it without a new dependency.
static SVC_FOLD: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
static SVC_TALLY: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
static SVC_FAILS: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
static SVC_FIRST_NS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
// Per-process opt-in for AUTO-folding `debug_println` markers. Only an ARMED process folds its
// debug_println lines into its verdict — so a folding-but-never-flushing process (init, the
// selftest observer) keeps printing raw and never loses a line. Services whose markers go through
// a custom funnel (e.g. keystored's emit_line → service_marker) need NOT arm.
static SVC_ARMED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
// Per-group EXPAND override (the configurable grid): when set, THIS process does NOT fold — it prints
// every marker raw and emits no verdict line, so you can focus on one group's full detail while every
// other group stays compactly folded. Set from config (`NEXUS_LOG_EXPAND=<group>`) at bootstrap.
// Nothing is hidden: folding is just the default view; expand recalls the raw stream per group.
static SVC_EXPAND: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
// Persistent interactive-fold policy: like SVC_FOLD but NEVER cleared at the `ready` flush, so
// post-`ready` runtime TRACE lines (IPC rx/tx, cap moves, audit echoes) can still fold in the
// interactive view even though the verdict is already emitted. Set once at bootstrap (set_verdict_fold).
static SVC_FOLD_MODE: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Opt THIS process out of folding (the configurable per-group EXPAND): all its markers print raw and
/// no verdict line is emitted, while every other group stays folded. Call at bootstrap when this
/// service is in the config's expand set. Overrides fold/arm.
pub fn set_verdict_expand(on: bool) {
    use core::sync::atomic::Ordering;
    SVC_EXPAND.store(on, Ordering::Relaxed);
}

/// Enable per-process verdict folding. Call once at service bootstrap with the kernel boot mode.
/// When enabling, also stamps the init-start time so the verdict's `<ms>` measures bootstrap→ready
/// (the real service init duration), not just the span between the first folded marker and flush.
pub fn set_verdict_fold(on: bool) {
    use core::sync::atomic::Ordering;
    SVC_FOLD.store(on, Ordering::Relaxed);
    // Persistent twin (survives the flush) so post-`ready` runtime traces keep folding via service_trace().
    SVC_FOLD_MODE.store(on, Ordering::Relaxed);
    if on {
        SVC_FIRST_NS.store(span_now_ns(), Ordering::Relaxed);
    }
}

/// Arm AUTO-folding of this process's `debug_println` markers into its verdict (call once at the
/// service's `os_entry`, for services whose markers are scattered `debug_println` rather than a
/// single funnel — gpud, windowd). A no-op effect in proof boots (folding still gated on the mode).
/// Pair with [`service_verdict_flush`] at the service's ready point.
pub fn service_verdict_arm() {
    use core::sync::atomic::Ordering;
    SVC_ARMED.store(true, Ordering::Relaxed);
}

// Only the `nexus_env = "os"` `debug_println` reads this (the host build's println never folds), so
// gate it to its sole caller to keep host clippy of nexus-abi consumers warning-free.
#[cfg(nexus_env = "os")]
#[inline]
fn svc_armed() -> bool {
    SVC_ARMED.load(core::sync::atomic::Ordering::Relaxed)
}

/// Tally one marker, auto-detecting failure from its text (contains `err`/`FAIL`/`denied`).
/// Returns `true` when the caller should SUPPRESS the line (routine marker folded). The one-call
/// convenience a service's marker funnel uses: `if nexus_abi::service_marker(msg) { return; }`.
#[must_use]
pub fn service_marker(line: &[u8]) -> bool {
    service_marker_tally(marker_is_failure(line))
}

/// Shared failure heuristic for service markers (a failure is counted + always printed live).
fn marker_is_failure(b: &[u8]) -> bool {
    fn has(h: &[u8], n: &[u8]) -> bool {
        n.len() <= h.len() && h.windows(n.len()).any(|w| w == n)
    }
    // RFC-0068: match `err` only as a TOKEN (`error`, ` err`, `err=`) — never as a bare substring,
    // which false-flagged routine lines like `audit emit deferred` / `interrupt` as failures and so
    // forced them to print raw instead of folding. The real failure markers all use one of these
    // token forms (`recv error`, `registry recv err`, `err={err}`).
    has(b, b"error") || has(b, b" err") || has(b, b"err=") || has(b, b"FAIL") || has(b, b"denied")
}

/// Tally one of this service's markers; returns `true` when the caller should SUPPRESS the line
/// (routine marker folded into the verdict). Only suppresses while folding; a `failed` marker is
/// counted as a failure and never suppressed.
#[must_use]
pub fn service_marker_tally(failed: bool) -> bool {
    use core::sync::atomic::Ordering;
    // Expanded group, or not folding (proof) → never suppress: print raw.
    if SVC_EXPAND.load(Ordering::Relaxed) || !SVC_FOLD.load(Ordering::Relaxed) {
        return false;
    }
    if SVC_FIRST_NS.load(Ordering::Relaxed) == 0 {
        SVC_FIRST_NS.store(span_now_ns(), Ordering::Relaxed);
    }
    SVC_TALLY.fetch_add(1, Ordering::Relaxed);
    if failed {
        SVC_FAILS.fetch_add(1, Ordering::Relaxed);
        return false;
    }
    true
}

/// Suppress one routine *runtime* TRACE line — post-`ready` plumbing a service emits forever (IPC
/// rx/tx ops, cap moves, audit echoes already journaled in logd). Returns `true` when the caller
/// should SUPPRESS the line. Unlike [`service_marker`] this is independent of the per-process verdict
/// (already flushed at `ready`): it just hides recall-only detail in the folded interactive view.
/// Always prints raw in proof boots (fold mode off) and when this group is expanded
/// (`NEXUS_LOG_EXPAND=<svc>` → [`set_verdict_expand`]); nothing is ever lost. The shared SSOT every
/// service's `emit_trace` funnel uses: `if nexus_abi::service_trace() { return; }`.
#[must_use]
pub fn service_trace() -> bool {
    use core::sync::atomic::Ordering;
    SVC_FOLD_MODE.load(Ordering::Relaxed) && !SVC_EXPAND.load(Ordering::Relaxed)
}

/// Combined funnel gate for a service's `emit_line`: PRE-`ready` markers fold into the per-process
/// verdict (tally), POST-`ready` routine markers fold into recall-only detail, and failures/proof
/// boots always print. Returns `true` when the caller should SUPPRESS the line. The drop-in upgrade
/// of [`service_marker`] for funnel services that also emit runtime markers after `ready`:
/// `if nexus_abi::service_line(msg.as_bytes()) { return; }`.
#[must_use]
pub fn service_line(line: &[u8]) -> bool {
    service_marker(line) || (!marker_is_failure(line) && service_trace())
}

/// Emit this service's verdict as one atomic grid line, then stop folding (later runtime markers
/// print raw). No-op when not folding or nothing was tallied. Pairs with [`service_marker_tally`]
/// so folded markers are never lost without a verdict.
pub fn service_verdict_flush(service: &str) {
    use core::sync::atomic::Ordering;
    // Expanded group prints raw detail (no verdict line); proof boots don't fold either.
    if SVC_EXPAND.load(Ordering::Relaxed) || !SVC_FOLD.load(Ordering::Relaxed) {
        return;
    }
    let total = SVC_TALLY.load(Ordering::Relaxed);
    if total != 0 {
        let fails = SVC_FAILS.load(Ordering::Relaxed);
        let first = SVC_FIRST_NS.load(Ordering::Relaxed);
        let now = span_now_ns();
        // RFC-0068: the verdict math (passed/total, ms, OK/WARN-slow/ERROR with the soft-real-time
        // SLOW budget) lives once in nexus-event; this is just the per-process atomic feeder. `first
        // == 0` is the unset sentinel (a real marker's nsec is never 0 here).
        let started_at = (first != 0).then_some(first);
        let v = nexus_event::verdict_from(total, fails, started_at, now);
        svc_emit_verdict_line(now, service, v);
    }
    SVC_FOLD.store(false, Ordering::Relaxed);
    SVC_ARMED.store(false, Ordering::Relaxed);
}

#[cfg(all(target_arch = "riscv64", target_os = "none"))]
fn svc_emit_verdict_line(now: u64, service: &str, v: nexus_event::Verdict) {
    // RFC-0068: the grid-line FORMAT is the shared SSOT in nexus-event (same renderer the kernel
    // GROUP flush uses) — this just owns the fixed buffer + the one atomic console write.
    let mut buf = [0u8; 96];
    let n = nexus_event::render_verdict_line(&mut buf, now, service, v);
    let _ = debug_write(&buf[..n]);
}
#[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
fn svc_emit_verdict_line(_now: u64, _service: &str, _v: nexus_event::Verdict) {}

/// Current monotonic nanoseconds, or 0 where unavailable (host). Internal to [`Span`].
#[cfg(nexus_env = "os")]
fn span_now_ns() -> u64 {
    nsec().unwrap_or(0)
}
#[cfg(not(nexus_env = "os"))]
fn span_now_ns() -> u64 {
    0
}

/// A monotonic timing span over the kernel clock, for boot/section instrumentation (the
/// signpost primitive). Reading is one cheap syscall; on host (no clock) it degrades to zero
/// duration so the same instrumentation compiles and runs in host tests.
pub struct Span {
    start_ns: u64,
}

impl Span {
    /// Begins a span at the current monotonic time.
    pub fn begin() -> Self {
        Self { start_ns: span_now_ns() }
    }

    /// Nanoseconds elapsed since [`begin`](Self::begin).
    pub fn elapsed_ns(&self) -> u64 {
        span_now_ns().saturating_sub(self.start_ns)
    }

    /// Whole milliseconds elapsed since [`begin`](Self::begin).
    pub fn elapsed_ms(&self) -> u64 {
        self.elapsed_ns() / 1_000_000
    }

    /// Whole microseconds elapsed since [`begin`](Self::begin).
    pub fn elapsed_us(&self) -> u64 {
        self.elapsed_ns() / 1_000
    }
}

/// Writes a single byte to the kernel UART from userspace for debugging.
#[cfg(nexus_env = "os")]
pub fn debug_putc(byte: u8) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        const SYSCALL_DEBUG_PUTC: usize = 16;
        let raw = unsafe { ecall1(SYSCALL_DEBUG_PUTC, byte as usize) };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        let _ = byte;
        Err(AbiError::Unsupported)
    }
}

/// Writes a byte slice to the kernel UART for debugging. The whole slice is emitted
/// atomically by the kernel under the UART lock (one syscall), so it cannot interleave
/// mid-slice with the kernel or another process.
#[cfg(nexus_env = "os")]
pub fn debug_write(bytes: &[u8]) -> SysResult<()> {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        if bytes.is_empty() {
            return Ok(());
        }
        const SYSCALL_DEBUG_WRITE: usize = 44;
        let raw = unsafe { ecall2(SYSCALL_DEBUG_WRITE, bytes.as_ptr() as usize, bytes.len()) };
        decode_syscall(raw).map(|_| ())
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        for &b in bytes {
            debug_putc(b)?;
        }
        Ok(())
    }
}

/// Writes a line (with trailing '\n') to the kernel UART for debugging. The content and the
/// newline go out in a single atomic [`debug_write`] (via a bounded stack buffer) so a line
/// is never split across two console writes; very long lines fall back to content + newline.
#[cfg(nexus_env = "os")]
pub fn debug_println(s: &str) -> SysResult<()> {
    // Verdict folding: an ARMED service folds its routine `debug_println` lines in interactive boots.
    // `service_line` folds BOTH phases: pre-`ready` markers tally into the `<service> N/N` verdict,
    // and post-`ready` runtime traces (IPC/present/chain echoes) fold into recall-only detail
    // (`NEXUS_LOG_EXPAND=<svc>`). FAIL lines and proof boots always print; only armed processes fold,
    // so init/the observer keep printing raw and never lose a line.
    if svc_armed() && service_line(s.as_bytes()) {
        return Ok(());
    }
    // RFC-0068: in interactive boots, give the raw (unfolded) marker the SAME `[    S.uuuuuu]  ` grid
    // timestamp as the verdict lines, so the `build/logs` boot timeline shows WHEN each marker fired
    // (this is how the gpud/windowd/hidrawd display+input chain becomes legible). Proof boots stay
    // unprefixed — `verify-uart` + the nexus-evidence canonicalizer see the bare line (its `[ts=…ms]`
    // parser only triggers on a `[ts=` prefix, but keeping proof output bare avoids any ambiguity).
    if boot_should_fold_verdicts() {
        let now = nsec().unwrap_or(0);
        let mut buf = [0u8; 640];
        let n = nexus_event::render_marker_ts(&mut buf, now, s);
        return debug_write(&buf[..n]);
    }
    const LINE_CAP: usize = 512;
    let bytes = s.as_bytes();
    if bytes.len() < LINE_CAP {
        let mut buf = [0u8; LINE_CAP];
        buf[..bytes.len()].copy_from_slice(bytes);
        buf[bytes.len()] = b'\n';
        debug_write(&buf[..bytes.len() + 1])
    } else {
        debug_write(bytes)?;
        debug_write(b"\n")
    }
}

/// Write the grid TIMESTAMP PREFIX `[    S.uuuuuu]  ` to the console (RFC-0068), for emitters that
/// build a marker line from `debug_putc` fragments (e.g. `abilitymgr`) rather than one `debug_println`.
/// Call it once at the START of the marker line. No-op in proof boots (keeps the evidence line bare).
#[cfg(nexus_env = "os")]
pub fn debug_ts_prefix() {
    if !boot_should_fold_verdicts() {
        return;
    }
    let now = nsec().unwrap_or(0);
    let mut buf = [0u8; 24];
    let n = nexus_event::render_ts_prefix(&mut buf, now);
    let _ = debug_write(&buf[..n]);
}

/// Emit a routine bring-up/runtime trace line that FOLDS in the interactive grid view (recall with
/// `NEXUS_LOG_EXPAND=<svc>`) and prints raw in proof boots (so `verify-uart` still sees it). The
/// shared funnel — via [`service_trace`] — for services that emit scattered `debug_println` markers
/// (gpud, dsoftbusd) rather than routing through a single `emit_line`. Unlike [`debug_trace`] (gated
/// on an explicit verbosity flag) this follows the per-process verdict-fold policy. NEVER use it for
/// SELFTEST markers or failures — those must always reach the observer + verify-uart.
#[cfg(nexus_env = "os")]
pub fn trace_line(s: &str) -> SysResult<()> {
    // Failure safety net: a marker the heuristic flags (err/FAIL/denied) is never folded, even if a
    // caller routes it here by mistake. Routine lines fold in interactive, raw in proof.
    if !marker_is_failure(s.as_bytes()) && service_trace() {
        return Ok(());
    }
    debug_println(s)
}

/// Runtime gate for developer trace breadcrumbs (see [`debug_trace`]). Off by default so
/// high-frequency dev lines stay silent in a normal boot; flipped on by the boot-time
/// verbosity knob for a focused debug session.
#[cfg(nexus_env = "os")]
static DEBUG_TRACE_ON: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Enable or disable developer trace breadcrumbs emitted via [`debug_trace`].
#[cfg(nexus_env = "os")]
pub fn set_debug_trace(on: bool) {
    DEBUG_TRACE_ON.store(on, core::sync::atomic::Ordering::Relaxed);
}

/// Developer trace breadcrumb. Routes to [`debug_println`] only when tracing is enabled, so
/// these lines are silent by default but one runtime flag away — never use it for markers or
/// errors, which must always emit.
#[cfg(nexus_env = "os")]
pub fn debug_trace(s: &str) -> SysResult<()> {
    if DEBUG_TRACE_ON.load(core::sync::atomic::Ordering::Relaxed) {
        debug_println(s)
    } else {
        Ok(())
    }
}
