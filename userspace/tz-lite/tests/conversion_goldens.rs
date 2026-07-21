// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! RFC-0076 tz-lite goldens: civil conversion + DST boundaries, pinned
//! against independently computed fixtures.

use tz_lite::{format_hm, to_civil, zone};

const NS: u64 = 1_000_000_000;

/// 2026-07-21T12:00:00Z (fixture; computed independently).
const T_2026_07_21_1200Z: u64 = 1_784_635_200 * NS;

#[test]
fn summer_fixture_across_zones() {
    let utc = to_civil(T_2026_07_21_1200Z, zone("UTC").unwrap());
    assert_eq!((utc.year, utc.month, utc.day, utc.hour, utc.minute), (2026, 7, 21, 12, 0));
    assert_eq!(utc.weekday, 1, "2026-07-21 is a Tuesday");

    // Berlin: CEST (UTC+2) in July.
    let berlin = to_civil(T_2026_07_21_1200Z, zone("Europe/Berlin").unwrap());
    assert_eq!((berlin.hour, berlin.minute), (14, 0));
    // London: BST (UTC+1).
    let london = to_civil(T_2026_07_21_1200Z, zone("Europe/London").unwrap());
    assert_eq!(london.hour, 13);
    // New York: EDT (UTC-4).
    let ny = to_civil(T_2026_07_21_1200Z, zone("America/New_York").unwrap());
    assert_eq!(ny.hour, 8);
    // Tokyo: fixed UTC+9.
    let tokyo = to_civil(T_2026_07_21_1200Z, zone("Asia/Tokyo").unwrap());
    assert_eq!((tokyo.hour, tokyo.day), (21, 21));
    // Sydney: AEST (UTC+10, no DST in July — southern winter).
    let sydney = to_civil(T_2026_07_21_1200Z, zone("Australia/Sydney").unwrap());
    assert_eq!((sydney.hour, sydney.day), (22, 21));
}

#[test]
fn eu_dst_boundaries_2026() {
    // EU 2026: DST starts Sun 2026-03-29 01:00 UTC, ends Sun 2026-10-25 01:00 UTC.
    let before = (1_774_745_940) * NS; // 2026-03-29T00:59:00Z
    let after = (1_774_746_060) * NS; // 2026-03-29T01:01:00Z
    let b = to_civil(before, zone("Europe/Berlin").unwrap());
    let a = to_civil(after, zone("Europe/Berlin").unwrap());
    assert_eq!((b.hour, b.minute), (1, 59), "still CET (+1)");
    assert_eq!((a.hour, a.minute), (3, 1), "sprang to CEST (+2)");

    let before_end = (1_792_889_940) * NS; // 2026-10-25T00:59:00Z
    let after_end = (1_792_890_060) * NS; // 2026-10-25T01:01:00Z
    let be = to_civil(before_end, zone("Europe/Berlin").unwrap());
    let ae = to_civil(after_end, zone("Europe/Berlin").unwrap());
    assert_eq!(be.hour, 2, "CEST until 01:00 UTC");
    assert_eq!(ae.hour, 2, "fell back to CET (02:xx again)");
    assert_eq!((be.minute, ae.minute), (59, 1));
}

#[test]
fn us_dst_boundaries_2026() {
    // US 2026: starts Sun 2026-03-08 02:00 EST (= 07:00 UTC),
    // ends Sun 2026-11-01 02:00 EDT (= 06:00 UTC).
    let before = (1_772_952_600) * NS; // 2026-03-08T06:50:00Z
    let after = (1_772_953_800) * NS; // 2026-03-08T07:10:00Z
    let b = to_civil(before, zone("America/New_York").unwrap());
    let a = to_civil(after, zone("America/New_York").unwrap());
    assert_eq!((b.hour, b.minute), (1, 50), "EST");
    assert_eq!((a.hour, a.minute), (3, 10), "EDT (spring forward)");
}

#[test]
fn hour_formatting_24h_and_12h() {
    let civil = to_civil(T_2026_07_21_1200Z, zone("Europe/Berlin").unwrap()); // 14:00
    let mut buf = [0u8; 8];
    let n = format_hm(&civil, true, &mut buf);
    assert_eq!(&buf[..n], b"14:00");
    let n = format_hm(&civil, false, &mut buf);
    assert_eq!(&buf[..n], b"2:00 PM");
    let midnight = to_civil(1_784_678_400 * NS, zone("UTC").unwrap()); // 2026-07-22T00:00Z
    let n = format_hm(&midnight, false, &mut buf);
    assert_eq!(&buf[..n], b"12:00 AM");
}

#[test]
fn zone_lookup_matches_table() {
    assert!(zone("Europe/Berlin").is_some());
    assert!(zone("Mars/Olympus").is_none());
    assert_eq!(tz_lite::ZONES.len(), 9);
}
