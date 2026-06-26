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
use core::sync::atomic::{AtomicU32, AtomicU8, Ordering};

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
