// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0076 client-side timezone conversion — a curated IANA subset
//! (const tables, incl. DST rules) turning `timed`'s UTC epoch into civil
//! time. The zone list is the `time.zone` settings-key validator SSOT
//! (settingsd pins it with a test). Pure const math: no_std, zero deps,
//! deterministic; no leap seconds, no historical transitions (post-2007
//! rules only — honest bounds for a shipped subset).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable for RFC-0076 Phase 2
//! TEST_COVERAGE: Conversion + DST-boundary goldens in `tests/`.
//! RFC: docs/rfcs/RFC-0076-wallclock-v1-rtcd-timed-tz.md

#![cfg_attr(all(nexus_env = "os", target_os = "none"), no_std)]
#![forbid(unsafe_code)]

/// Daylight-saving rule families (post-2007 rules).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DstRule {
    /// No DST.
    None,
    /// EU: +1h from last Sunday of March 01:00 UTC to last Sunday of
    /// October 01:00 UTC.
    Eu,
    /// US/Canada: +1h from second Sunday of March to first Sunday of
    /// November (transitions at 02:00 local standard time).
    Us,
    /// South-eastern Australia: +1h from first Sunday of October to first
    /// Sunday of April (transitions at 02:00 local standard time).
    AuSoutheast,
}

/// One curated zone: IANA name, base UTC offset (minutes), DST rule.
#[derive(Debug, Clone, Copy)]
pub struct Zone {
    pub name: &'static str,
    base_offset_min: i32,
    rule: DstRule,
}

/// The curated zone table (RFC-0076) — the `time.zone` validator SSOT.
pub const ZONES: &[Zone] = &[
    Zone { name: "UTC", base_offset_min: 0, rule: DstRule::None },
    Zone { name: "Europe/Berlin", base_offset_min: 60, rule: DstRule::Eu },
    Zone { name: "Europe/London", base_offset_min: 0, rule: DstRule::Eu },
    Zone { name: "America/New_York", base_offset_min: -300, rule: DstRule::Us },
    Zone { name: "America/Los_Angeles", base_offset_min: -480, rule: DstRule::Us },
    Zone { name: "Asia/Tokyo", base_offset_min: 540, rule: DstRule::None },
    Zone { name: "Asia/Shanghai", base_offset_min: 480, rule: DstRule::None },
    Zone { name: "Asia/Seoul", base_offset_min: 540, rule: DstRule::None },
    Zone { name: "Australia/Sydney", base_offset_min: 600, rule: DstRule::AuSoutheast },
];

/// Looks a zone up by its IANA name.
#[must_use]
pub fn zone(name: &str) -> Option<&'static Zone> {
    ZONES.iter().find(|z| z.name == name)
}

/// Civil date-time in a zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Civil {
    pub year: i32,
    /// 1–12.
    pub month: u8,
    /// 1–31.
    pub day: u8,
    /// 0 = Monday … 6 = Sunday (ISO).
    pub weekday: u8,
    pub hour: u8,
    pub minute: u8,
}

/// Days-from-epoch → (year, month, day). Howard Hinnant's algorithm
/// (proleptic Gregorian; exact for the supported range).
const fn civil_from_days(z: i64) -> (i32, u8, u8) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    ((y + if m <= 2 { 1 } else { 0 }) as i32, m as u8, d as u8)
}

/// Days-from-epoch for a civil date (inverse of `civil_from_days`).
const fn days_from_civil(y: i32, m: u8, d: u8) -> i64 {
    let y = (y as i64) - if m <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + (d as i64) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// ISO weekday (0 = Monday) for days-from-epoch (1970-01-01 was a Thursday).
const fn weekday_from_days(days: i64) -> u8 {
    (((days % 7) + 10) % 7) as u8
}

/// Days-from-epoch of the LAST `weekday` (ISO) in `year`/`month`.
fn last_weekday_of_month(year: i32, month: u8, weekday: u8) -> i64 {
    let next_month_first = if month == 12 {
        days_from_civil(year + 1, 1, 1)
    } else {
        days_from_civil(year, month + 1, 1)
    };
    let last = next_month_first - 1;
    last - i64::from((weekday_from_days(last) + 7 - weekday) % 7)
}

/// Days-from-epoch of the Nth (1-based) `weekday` (ISO) in `year`/`month`.
fn nth_weekday_of_month(year: i32, month: u8, weekday: u8, nth: u8) -> i64 {
    let first = days_from_civil(year, month, 1);
    let first_match = first + i64::from((weekday + 7 - weekday_from_days(first)) % 7);
    first_match + i64::from(nth - 1) * 7
}

const SUNDAY: u8 = 6;
const MIN_NS: u64 = 60_000_000_000;
const DAY_MIN: i64 = 24 * 60;

/// DST offset (minutes) active at `epoch_min` (UTC minutes since epoch).
fn dst_offset_min(rule: DstRule, base_offset_min: i32, epoch_min: i64) -> i32 {
    let utc_days = epoch_min.div_euclid(DAY_MIN);
    let (year, _, _) = civil_from_days(utc_days);
    match rule {
        DstRule::None => 0,
        DstRule::Eu => {
            // Transitions at 01:00 UTC, last Sundays of March/October.
            let start = last_weekday_of_month(year, 3, SUNDAY) * DAY_MIN + 60;
            let end = last_weekday_of_month(year, 10, SUNDAY) * DAY_MIN + 60;
            if epoch_min >= start && epoch_min < end {
                60
            } else {
                0
            }
        }
        DstRule::Us => {
            // Transitions at 02:00 LOCAL STANDARD time.
            let std_min = epoch_min + i64::from(base_offset_min);
            let start = nth_weekday_of_month(year, 3, SUNDAY, 2) * DAY_MIN + 120;
            let end = nth_weekday_of_month(year, 11, SUNDAY, 1) * DAY_MIN + 120;
            if std_min >= start && std_min < end {
                60
            } else {
                0
            }
        }
        DstRule::AuSoutheast => {
            // Southern hemisphere: DST spans the NEW year (Oct → Apr),
            // transitions at 02:00 local standard time.
            let std_min = epoch_min + i64::from(base_offset_min);
            let start = nth_weekday_of_month(year, 10, SUNDAY, 1) * DAY_MIN + 120;
            let end = nth_weekday_of_month(year, 4, SUNDAY, 1) * DAY_MIN + 120;
            if std_min >= start || std_min < end {
                60
            } else {
                0
            }
        }
    }
}

/// Converts a UTC epoch (nanoseconds) to civil time in `zone`.
#[must_use]
pub fn to_civil(epoch_ns: u64, zone: &Zone) -> Civil {
    let epoch_min = (epoch_ns / MIN_NS) as i64;
    let offset = i64::from(zone.base_offset_min)
        + i64::from(dst_offset_min(zone.rule, zone.base_offset_min, epoch_min));
    let local_min = epoch_min + offset;
    let days = local_min.div_euclid(DAY_MIN);
    let minute_of_day = local_min.rem_euclid(DAY_MIN);
    let (year, month, day) = civil_from_days(days);
    Civil {
        year,
        month,
        day,
        weekday: weekday_from_days(days),
        hour: (minute_of_day / 60) as u8,
        minute: (minute_of_day % 60) as u8,
    }
}

/// Formats hour/minute as `HH:MM` (24h) or `H:MM AM/PM` (12h) into a fixed
/// buffer; returns the used slice.
#[must_use]
pub fn format_hm(civil: &Civil, twenty_four: bool, out: &mut [u8; 8]) -> usize {
    let (h, suffix): (u8, &[u8]) = if twenty_four {
        (civil.hour, b"")
    } else {
        let h12 = match civil.hour % 12 {
            0 => 12,
            h => h,
        };
        (h12, if civil.hour < 12 { b" AM" } else { b" PM" })
    };
    let mut n = 0;
    if twenty_four || h >= 10 {
        out[n] = b'0' + h / 10;
        n += 1;
    }
    out[n] = b'0' + h % 10;
    n += 1;
    out[n] = b':';
    n += 1;
    out[n] = b'0' + civil.minute / 10;
    n += 1;
    out[n] = b'0' + civil.minute % 10;
    n += 1;
    out[n..n + suffix.len()].copy_from_slice(suffix);
    n + suffix.len()
}
