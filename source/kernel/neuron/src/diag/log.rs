// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Minimal structured logging with severity levels
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests (exercised by QEMU smoke + kernel selftests)
//! PUBLIC API: log_* macros, emit(level,target,args)
//! DEPENDS_ON: uart::KernelUart
//! INVARIANTS: Debug/Trace only in debug builds; single-line emission
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

use core::fmt::{Arguments, Write};
use core::sync::atomic::{AtomicU32, AtomicU8, AtomicU64, Ordering};

/// Logging severity used by the kernel. Mirrors `nexus_log::Level` (userspace) so the
/// kernel and userspace facades share one policy vocabulary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Level {
    Error = 1,
    Warn = 2,
    Info = 3,
    Debug = 4,
    Trace = 5,
}

impl Level {
    const fn tag(self) -> &'static str {
        match self {
            Level::Error => "ERROR",
            Level::Warn => "WARN",
            Level::Info => "INFO",
            Level::Debug => "DEBUG",
            Level::Trace => "TRACE",
        }
    }
}

/// Runtime verbosity floor. Default `Info` keeps the kernel quiet by default:
/// Error/Warn/Info emit, Debug/Trace do not — in every build, including debug. A debug
/// session raises this (or narrows the topic mask) at runtime instead of recompiling.
static MAX_LEVEL: AtomicU8 = AtomicU8::new(Level::Info as u8);

/// Per-topic allow mask. Default all-on; narrow it to focus a debug session on one area
/// (e.g. only `cap` or `vm` trace) without drowning in every other topic.
static TOPIC_MASK: AtomicU32 = AtomicU32::new(u32::MAX);

// Topic bits. Bit 0 is the catch-all for un-tagged targets and is always allowed when the
// mask is full. Keep in sync with the userspace topic vocabulary where they overlap.
const TOPIC_GENERAL: u32 = 1 << 0;
const TOPIC_CAP: u32 = 1 << 1;
const TOPIC_VM: u32 = 1 << 2;
const TOPIC_SCHED: u32 = 1 << 3;

/// Set the runtime verbosity floor (lines at this level or more severe emit).
/// The runtime control surface for the boot-time verbosity knob; wired to the kernel
/// cmdline / a debug syscall in a later step of this track.
#[allow(dead_code)]
pub fn set_max_level(level: Level) {
    MAX_LEVEL.store(level as u8, Ordering::Relaxed);
}

/// Set the per-topic allow mask (a bitmask of `TOPIC_*`). See [`set_max_level`].
#[allow(dead_code)]
pub fn set_topic_mask(bits: u32) {
    TOPIC_MASK.store(bits, Ordering::Relaxed);
}

// ── Verdict aggregation (alloc-free) ─────────────────────────────────────────────────────────
//
// In interactive boots the kernel folds a subsystem's routine markers into ONE grid verdict
// (`[ts] OK kself N/N <ms>`) instead of printing every line; a guaranteed flush emits the verdict,
// and FAIL/ERROR/WARN always print live (a problem is never hidden). Proof boots do NOT fold
// (`boot_mode::fold_verdicts()` is false there) so `verify-uart` still sees every raw marker.
// State is plain atomics — NO heap/Vec/replay-buffer (the kernel's UART+alloc constraint). This
// first slice folds only the `selftest` topic; more subsystems adopt the same pattern next.

/// Monotonic nanoseconds from the RISC-V `time` CSR (QEMU virt timebase = 10 MHz → 100 ns/tick).
/// Host builds have no CSR; return 0 (verdict timing is a no-op off-target).
#[inline]
fn now_ns() -> u64 {
    #[cfg(all(target_arch = "riscv64", target_os = "none"))]
    {
        (riscv::register::time::read() as u64).wrapping_mul(100)
    }
    #[cfg(not(all(target_arch = "riscv64", target_os = "none")))]
    {
        0
    }
}

// Group accumulator table. Each foldable subsystem gets one slot (total markers, failures, first
// marker time). Plain atomics, const-constructible — no heap. Add a group by extending the indices,
// `GROUP_NAMES`, and `group_of`, then wire its flush where the group has emitted all its markers.
//
// NOTE: only the kernel SELFTEST folds cleanly. Other kernel topics do NOT: `boot`/`traps` markers
// fire BEFORE the fw_cfg mode probe (fold flag not set yet); `as`/`exec` fire per service-spawn
// (they belong in the per-service verdicts, S4); `sched`/`mm` are already DEBUG-gated off. So the
// kernel grid is essentially `kself`; the rest of the grid lives in the per-service aggregator.
const GROUP_KSELF: usize = 0;
const GROUP_COUNT: usize = 1;
const GROUP_NAMES: [&str; GROUP_COUNT] = ["kself"];

struct GroupAcc {
    tally: AtomicU32,
    fails: AtomicU32,
    first_ns: AtomicU64,
}

impl GroupAcc {
    const fn new() -> Self {
        Self { tally: AtomicU32::new(0), fails: AtomicU32::new(0), first_ns: AtomicU64::new(0) }
    }
}

static GROUPS: [GroupAcc; GROUP_COUNT] = [GroupAcc::new()];

/// Map a diag target tag to its verdict group, if it folds. Unlisted targets print normally
/// (subject only to the level/topic gate).
const fn group_of(target: &str) -> Option<usize> {
    match target.as_bytes() {
        b"selftest" => Some(GROUP_KSELF),
        _ => None,
    }
}

/// Tally one marker into group `g`; returns `true` when the caller should SUPPRESS the line
/// (routine marker folded into the verdict). Only suppresses in interactive boots. WARN/ERROR are
/// tallied as failures but still print live — a problem is never hidden.
fn group_fold(g: usize, level: Level) -> bool {
    if !crate::boot_mode::fold_verdicts() {
        return false;
    }
    let acc = &GROUPS[g];
    if acc.first_ns.load(Ordering::Relaxed) == 0 {
        acc.first_ns.store(now_ns(), Ordering::Relaxed);
    }
    acc.tally.fetch_add(1, Ordering::Relaxed);
    if (level as u8) <= (Level::Warn as u8) {
        acc.fails.fetch_add(1, Ordering::Relaxed);
        return false;
    }
    true
}

/// Emit one group's verdict as a single atomic grid line. No-op in proof boots / when empty.
/// Call where the group has emitted all its markers — pairing flush with the suppression in
/// [`emit`] guarantees no folded marker is ever dropped without a verdict.
fn flush_group(g: usize) {
    if !crate::boot_mode::fold_verdicts() {
        return;
    }
    let acc = &GROUPS[g];
    let total = acc.tally.load(Ordering::Relaxed);
    if total == 0 {
        return;
    }
    let fails = acc.fails.load(Ordering::Relaxed);
    let first = acc.first_ns.load(Ordering::Relaxed);
    let now = now_ns();
    // RFC-0068: the verdict math (passed/total, ms, OK/WARN-slow/ERROR) is the shared SSOT in
    // nexus-event — the kernel groups now get the same soft-real-time slow flag the services have.
    let started_at = (first != 0).then_some(first);
    let v = nexus_event::verdict_from(total, fails, started_at, now);
    let mut uart = crate::uart::KernelUart::lock();
    let _ = write!(
        &mut *uart,
        "[{:>5}.{:06}]  {:<6} {:<14} {}/{}   {}ms{}\n",
        now / 1_000_000_000,
        (now % 1_000_000_000) / 1000,
        v.tag.label(),
        GROUP_NAMES[g],
        v.passed,
        v.total,
        v.ms,
        if v.tag.is_slow() { "  slow" } else { "" }
    );
}

/// Flush the kernel selftest group verdict. Call at the end of the kernel selftest run.
pub fn verdict_flush_kself() {
    flush_group(GROUP_KSELF);
}

const fn topic_bit(target: &str) -> u32 {
    // Cheap, const-friendly classification by target tag.
    match target.as_bytes() {
        b"cap" => TOPIC_CAP,
        b"vm" | b"as" | b"mm" => TOPIC_VM,
        b"sched" => TOPIC_SCHED,
        _ => TOPIC_GENERAL,
    }
}

fn gate_open(level: Level, target: &str) -> bool {
    if (level as u8) > MAX_LEVEL.load(Ordering::Relaxed) {
        return false;
    }
    let bit = topic_bit(target);
    (TOPIC_MASK.load(Ordering::Relaxed) & bit) == bit
}

/// Cheap predicate for raw/early emit sites that must keep their own writer (e.g. the
/// satp-switch path, which deliberately avoids the UART lock and the heap). Such sites
/// guard their raw write with this instead of routing through [`emit`].
pub fn would_log(level: Level, target: &str) -> bool {
    gate_open(level, target)
}

/// Emits a structured log line if the level + topic are enabled at runtime.
pub fn emit(level: Level, target: &'static str, args: Arguments<'_>) {
    if !gate_open(level, target) {
        return;
    }

    // Verdict folding: in interactive boots a foldable subsystem's routine markers are tallied into
    // its `<group> N/N` verdict (flushed where the group ends) instead of printed; FAIL/WARN still
    // print. Proof boots never fold, so `verify-uart` sees every raw marker.
    if let Some(g) = group_of(target) {
        if group_fold(g, level) {
            return;
        }
    }

    let mut uart = crate::uart::KernelUart::lock();
    let mut writer = &mut *uart;
    let _ = Write::write_fmt(&mut writer, format_args!("[{} {}] ", level.tag(), target));
    let _ = Write::write_fmt(&mut writer, args);
    let _ = Write::write_char(&mut writer, '\n');
}

#[macro_export]
macro_rules! log_error {
    (target: $target:expr, $($arg:tt)+) => {{
        $crate::log::emit($crate::log::Level::Error, $target, format_args!($($arg)+));
    }};
    ($($arg:tt)+) => {{
        $crate::log::emit($crate::log::Level::Error, module_path!(), format_args!($($arg)+));
    }};
}

#[macro_export]
macro_rules! log_warn {
    (target: $target:expr, $($arg:tt)+) => {{
        $crate::log::emit($crate::log::Level::Warn, $target, format_args!($($arg)+));
    }};
    ($($arg:tt)+) => {{
        $crate::log::emit($crate::log::Level::Warn, module_path!(), format_args!($($arg)+));
    }};
}

#[macro_export]
macro_rules! log_info {
    (target: $target:expr, $($arg:tt)+) => {{
        $crate::log::emit($crate::log::Level::Info, $target, format_args!($($arg)+));
    }};
    ($($arg:tt)+) => {{
        $crate::log::emit($crate::log::Level::Info, module_path!(), format_args!($($arg)+));
    }};
}

#[macro_export]
macro_rules! log_debug {
    (target: $target:expr, $($arg:tt)+) => {{
        $crate::log::emit($crate::log::Level::Debug, $target, format_args!($($arg)+));
    }};
    ($($arg:tt)+) => {{
        $crate::log::emit($crate::log::Level::Debug, module_path!(), format_args!($($arg)+));
    }};
}

#[macro_export]
macro_rules! log_trace {
    (target: $target:expr, $($arg:tt)+) => {{
        $crate::log::emit($crate::log::Level::Trace, $target, format_args!($($arg)+));
    }};
    ($($arg:tt)+) => {{
        $crate::log::emit($crate::log::Level::Trace, module_path!(), format_args!($($arg)+));
    }};
}
