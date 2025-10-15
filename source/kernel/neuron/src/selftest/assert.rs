// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! UART-friendly selftest assertion helpers.

extern crate alloc;

use alloc::format;

use crate::uart;

/// Emits the failure message and panics, ensuring the panic handler prints
/// diagnostic state afterwards.
#[cold]
#[allow(dead_code)]
pub fn report_failure(message: &str) -> ! {
    let line = format!("SELFTEST: fail: {message}");
    uart::write_line(&line);
    panic!("{}", line);
}

#[cold]
#[allow(dead_code)]
pub fn report_failure_fmt(args: core::fmt::Arguments<'_>) -> ! {
    use core::fmt::Write;

    let mut buffer = alloc::string::String::new();
    let _ = write!(buffer, "{args}");
    report_failure(&buffer);
}

/// Asserts that the condition evaluates to true.
#[macro_export]
macro_rules! st_assert {
    ($cond:expr $(,)?) => {
        if !$cond {
            $crate::selftest::assert::report_failure(concat!("assertion failed: ", stringify!($cond)));
        }
    };
    ($cond:expr, $($arg:tt)+) => {
        if !$cond {
            $crate::selftest::assert::report_failure_fmt(format_args!($($arg)+));
        }
    };
}

/// Expects both expressions to be equal using `PartialEq`.
#[macro_export]
macro_rules! st_expect_eq {
    ($left:expr, $right:expr $(,)?) => {{
        let left = &$left;
        let right = &$right;
        if *left != *right {
            $crate::selftest::assert::report_failure_fmt(format_args!(
                "expected {} == {}: left={:?} right={:?}",
                stringify!($left),
                stringify!($right),
                left,
                right
            ));
        }
    }};
    ($left:expr, $right:expr, $($arg:tt)+) => {{
        let left = &$left;
        let right = &$right;
        if *left != *right {
            $crate::selftest::assert::report_failure_fmt(format_args!(
                concat!($($arg)+, ": left={:?} right={:?}"),
                left,
                right
            ));
        }
    }};
}

/// Ensures that the expression evaluates to `Err` matching the provided pattern.
#[macro_export]
macro_rules! st_expect_err {
    ($expr:expr, $pat:pat $(if $guard:expr)? $(,)?) => {{
        match $expr {
            Err(err) => {
                if !matches!(err, $pat $(if $guard)?) {
                    $crate::selftest::assert::report_failure_fmt(format_args!(
                        "unexpected error variant: got={:?}",
                        err
                    ));
                }
            }
            Ok(value) => {
                let ty = core::any::type_name_of_val(&value);
                $crate::selftest::assert::report_failure_fmt(format_args!(
                    "expected Err({}), got Ok(<{}>)",
                    stringify!($pat $(if $guard)?),
                    ty
                ));
            }
        }
    }};
}
