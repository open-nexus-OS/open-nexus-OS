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
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, AtomicU64, Ordering};

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

// Kernel verdict groups (RFC-0068 subject-grouped boot transcript). Each kernel subject that emits
// routine boot markers folds into ONE grid verdict instead of printing every line. Add a subject =
// one `KGroup` variant + one `GROUP_DEFS` row, then either (facade) a `group_of` arm so its
// `log_*(target: "x", …)` lines auto-fold, or (raw `write!`) a `kfold(KGroup::X, …)` guard at the
// emit site. `kflush_all()` before the idle loop guarantees no folded line is ever lost.
//
// Folding is gated on `boot_mode::fold_verdicts()` (false in proof/unknown) → proof boots print
// every raw marker so `verify-uart` is undisturbed. The only lines that stay raw in an interactive
// boot are the pre-paging `boot:`/`traps:` markers that fire BEFORE the fw_cfg mode probe (the fold
// flag is not resolvable before the address space is active).
//
// IMPORTANT: the `KGroup` discriminants are the `GROUPS`/`GROUP_DEFS` indices — keep both in order.
// Only BOUNDED kernel phases are verdict groups (they reach a deterministic flush). Perpetual
// runtime events (process spawn/exec, QoS audit, the idle loop) are honest DEBUG lines instead —
// off by default, recalled with `NEXUS_LOG=<topic>=debug` — not boot verdicts.
#[derive(Clone, Copy)]
pub enum KGroup {
    Kself = 0,
    Syscalls,
    Boot,
    As,
    Sched,
    Smp,
}

const KGROUP_COUNT: usize = 6;

struct GroupDef {
    /// Grid label for the verdict line — also the `NEXUS_LOG_EXPAND=<name>` recall flag.
    name: &'static str,
    /// `true`: a bounded phase whose verdict shows a real span `<ms>` + soft-real-time slow flag.
    /// `false`: a count of events spread across the whole boot (`as`, `cap`, spawns) where a span
    /// is meaningless — rendered `0ms` and never flagged slow (so the WARN signal stays honest).
    timed: bool,
}

// Order MUST match the `KGroup` discriminants above.
const GROUP_DEFS: [GroupDef; KGROUP_COUNT] = [
    GroupDef { name: "kself", timed: true },
    GroupDef { name: "syscalls", timed: true },
    GroupDef { name: "boot", timed: false },
    GroupDef { name: "as", timed: false },
    GroupDef { name: "sched", timed: false },
    GroupDef { name: "smp", timed: false },
];

struct GroupAcc {
    tally: AtomicU32,
    fails: AtomicU32,
    first_ns: AtomicU64,
    /// Set once the group's verdict is emitted; afterwards its lines print raw (boot is over, the
    /// fold window is closed) so a post-flush runtime marker is never silently swallowed.
    flushed: AtomicBool,
}

impl GroupAcc {
    const fn new() -> Self {
        Self {
            tally: AtomicU32::new(0),
            fails: AtomicU32::new(0),
            first_ns: AtomicU64::new(0),
            flushed: AtomicBool::new(false),
        }
    }
}

static GROUPS: [GroupAcc; KGROUP_COUNT] = [
    GroupAcc::new(),
    GroupAcc::new(),
    GroupAcc::new(),
    GroupAcc::new(),
    GroupAcc::new(),
    GroupAcc::new(),
];

/// True if a kernel group is named in `NEXUS_LOG_EXPAND` — then its markers print raw (still counted)
/// instead of folding, so `NEXUS_LOG_EXPAND=syscalls` shows that group's full detail. The displayed
/// group name IS the flag (RFC-0068 subject-keyed expand, kernel side).
fn group_expanded(g: usize) -> bool {
    match option_env!("NEXUS_LOG_EXPAND") {
        Some(list) => list.split(',').any(|x| x.trim() == GROUP_DEFS[g].name),
        None => false,
    }
}

/// Map a diag target tag to its verdict group, if it folds (the facade auto-fold path: a
/// `log_*(target: "x", …)` line whose target is listed here is tallied into that group instead of
/// printed). Unlisted targets print normally (subject only to the level/topic gate).
const fn group_of(target: &str) -> Option<usize> {
    match target.as_bytes() {
        b"selftest" => Some(KGroup::Kself as usize),
        b"boot" => Some(KGroup::Boot as usize),
        b"as" => Some(KGroup::As as usize),
        b"smp" => Some(KGroup::Smp as usize),
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
    if acc.flushed.load(Ordering::Relaxed) {
        return false; // verdict already emitted: boot is over, print this runtime line raw
    }
    if GROUP_DEFS[g].timed && acc.first_ns.load(Ordering::Relaxed) == 0 {
        acc.first_ns.store(now_ns(), Ordering::Relaxed);
    }
    acc.tally.fetch_add(1, Ordering::Relaxed);
    if (level as u8) <= (Level::Warn as u8) {
        acc.fails.fetch_add(1, Ordering::Relaxed);
        return false;
    }
    if group_expanded(g) {
        return false; // expanded for debugging: print raw (already counted in the verdict)
    }
    true
}

/// Fold a RAW (non-facade) kernel marker at a `write!`/`write_str` site into its subject group;
/// returns `true` to SUPPRESS the raw line (it was tallied into the verdict). The shared SSOT every
/// raw kernel emit site uses: `if !crate::log::kfold(KGroup::Sched, Level::Info) { write!(…) }`.
/// WARN/ERROR are tallied as failures but still print live — a problem is never hidden.
pub fn kfold(g: KGroup, level: Level) -> bool {
    group_fold(g as usize, level)
}

/// Back-compat alias: the per-process syscall-table install echo folds into the `syscalls` group.
pub fn syscalls_fold() -> bool {
    group_fold(KGroup::Syscalls as usize, Level::Info)
}

/// Flush one kernel group's verdict by name — e.g. the `as` group when its rate-limiter exhausts
/// (it is fed during the first userspace context switches, after the kernel-phase flush).
pub fn kflush(g: KGroup) {
    flush_group(g as usize);
}

/// Emit one group's verdict as a single atomic grid line, then close its fold window (idempotent —
/// a later `kflush_all` skips it, and post-flush lines print raw). No-op in proof boots / when the
/// group is empty. Pairing flush with the suppression in [`group_fold`] guarantees no folded marker
/// is ever dropped without a verdict.
fn flush_group(g: usize) {
    if !crate::boot_mode::fold_verdicts() {
        return;
    }
    let acc = &GROUPS[g];
    if acc.flushed.swap(true, Ordering::Relaxed) {
        return; // already flushed (early per-group flush + the kflush_all safety net are idempotent)
    }
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
    // RFC-0068: render via the shared SSOT (the SAME format the services use, so the grid columns
    // can never drift between kernel and userspace), then one atomic write under the UART lock.
    let mut buf = [0u8; 96];
    let n = nexus_event::render_verdict_line(&mut buf, now, GROUP_DEFS[g].name, v);
    let mut uart = crate::uart::KernelUart::lock();
    let _ = uart.write_str(core::str::from_utf8(&buf[..n]).unwrap_or(""));
}

/// Flush the bounded kernel-init phase groups (kself, syscalls, sched, boot, smp) in one call,
/// once after the kernel selftest and just before the idle loop hands off to userspace — by then
/// each of these phases has emitted all its markers, so no folded line is left without a verdict.
/// The `as` group flushes separately via [`kflush`] on its rate-limiter (it is fed later, during
/// the first userspace context switches).
pub fn kflush_kernel_phase() {
    flush_group(KGroup::Kself as usize);
    flush_group(KGroup::Syscalls as usize);
    flush_group(KGroup::Sched as usize);
    flush_group(KGroup::Boot as usize);
    flush_group(KGroup::Smp as usize);
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
