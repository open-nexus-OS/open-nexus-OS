// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use nexus_cursor::hotspot_for;

#[test]
fn test_hotspot_left_ptr() {
    let (hx, hy) = hotspot_for("left_ptr", 32, 32);
    assert_eq!(hx, 5); // 0.15 * 32 = 4.8 → 5
    assert_eq!(hy, 5);

    let (hx, hy) = hotspot_for("left_ptr", 64, 64);
    assert_eq!(hx, 10); // 0.15 * 64 = 9.6 → 10
    assert_eq!(hy, 10);
}

#[test]
fn test_hotspot_cross() {
    let (hx, hy) = hotspot_for("cross", 48, 48);
    assert_eq!(hx, 24);
    assert_eq!(hy, 24);
}

#[test]
fn test_hotspot_unknown_defaults_to_center() {
    let (hx, hy) = hotspot_for("unknown_cursor", 100, 80);
    assert_eq!(hx, 50);
    assert_eq!(hy, 40);
}

#[test]
fn test_hotspot_determinism() {
    let (a_x, a_y) = hotspot_for("watch", 32, 32);
    let (b_x, b_y) = hotspot_for("watch", 32, 32);
    assert_eq!(a_x, b_x);
    assert_eq!(a_y, b_y);
}
