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

/// Logging severity used by the kernel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Level {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
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

    const fn enabled(self) -> bool {
        match self {
            Level::Debug | Level::Trace => cfg!(debug_assertions),
            _ => true,
        }
    }
}

/// Emits a structured log line if the level is enabled for the current build.
pub fn emit(level: Level, target: &'static str, args: Arguments<'_>) {
    if !level.enabled() {
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
