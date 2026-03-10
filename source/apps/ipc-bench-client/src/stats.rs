// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Statistical utilities for benchmark results

/// Format large numbers with separators for readability
pub fn format_number(n: u64) -> [u8; 32] {
    let mut buf = [0u8; 32];
    let mut pos = 0;
    let mut num = n;

    if num == 0 {
        buf[0] = b'0';
        return buf;
    }

    // Simple formatting without separators for no_std
    let mut digits = [0u8; 20];
    let mut digit_count = 0;

    while num > 0 {
        digits[digit_count] = b'0' + (num % 10) as u8;
        num /= 10;
        digit_count += 1;
    }

    // Reverse
    for i in 0..digit_count {
        buf[pos] = digits[digit_count - 1 - i];
        pos += 1;
    }

    buf
}

/// Format floating point number (simple, no_std)
pub fn format_float(f: f64, decimals: usize) -> [u8; 32] {
    let mut buf = [0u8; 32];
    let mut pos = 0;

    let integer_part = f as u64;
    let mut multiplier = 1u64;
    for _ in 0..decimals {
        multiplier *= 10;
    }
    let fractional_part = ((f - integer_part as f64) * multiplier as f64) as u64;

    // Format integer part
    let int_buf = format_number(integer_part);
    for &byte in &int_buf {
        if byte == 0 { break; }
        buf[pos] = byte;
        pos += 1;
    }

    // Decimal point
    buf[pos] = b'.';
    pos += 1;

    // Format fractional part
    let frac_buf = format_number(fractional_part);
    for &byte in &frac_buf {
        if byte == 0 { break; }
        buf[pos] = byte;
        pos += 1;
    }

    buf
}
