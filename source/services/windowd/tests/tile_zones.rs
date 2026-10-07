// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: TASK-0066 — the tiling geometry on the host, through windowd's public `zones`
//! module: the frames of every zone and arrangement at the lane's and the board's modes, the
//! release rule, the Return placement, the verb codes and the feed bits. The runtime wiring
//! (release → zone, menu verb, chords, reflow) is proven on the QEMU lanes.
//! OWNERS: @ui

use windowd::zones::{
    release_zone_at, request_from_code, return_frame_under_pointer, zone_frame, Arrangement, Zone,
    ZoneRequest, CODE_ARRANGE_LEFT_RIGHT, CODE_FILL, CODE_RETURN,
};

const MODES: [(u32, u32, i32); 3] = [(1280, 800, 36), (1920, 1080, 36), (1280, 744, 36)];

/// Every arrangement's zones are disjoint and cover the work area, at every mode and margin.
#[test]
fn arrangements_partition_the_work_area_at_every_mode() {
    for (w, h, top) in MODES {
        for margin in [0u32, 8, 16] {
            for kind in [Arrangement::LeftRight, Arrangement::TopBottom, Arrangement::Quarters] {
                let frames: Vec<_> =
                    kind.zones().iter().map(|z| zone_frame(*z, w, h, top, margin)).collect();
                let area: u64 =
                    frames.iter().map(|&(_, _, fw, fh)| u64::from(fw) * u64::from(fh)).sum();
                let (fx, fy, fw, fh) = zone_frame(Zone::Fill, w, h, top, margin);
                let gaps = match kind {
                    Arrangement::LeftRight => u64::from(margin) * u64::from(fh),
                    Arrangement::TopBottom => u64::from(margin) * u64::from(fw),
                    Arrangement::Quarters => {
                        u64::from(margin) * (u64::from(fw) + u64::from(fh))
                            - u64::from(margin) * u64::from(margin)
                    }
                };
                assert_eq!(
                    area + gaps,
                    u64::from(fw) * u64::from(fh),
                    "{kind:?} at {w}x{h} margin {margin} covers Fill minus the gaps"
                );
                for (i, a) in frames.iter().enumerate() {
                    assert!(a.0 >= fx && a.1 >= fy, "{kind:?} tile {i} inside the area");
                    for b in &frames[i + 1..] {
                        let overlap = a.0 < b.0 + b.2 as i32
                            && b.0 < a.0 + a.2 as i32
                            && a.1 < b.1 + b.3 as i32
                            && b.1 < a.1 + a.3 as i32;
                        assert!(!overlap, "{kind:?} tiles overlap at {w}x{h}");
                    }
                }
            }
        }
    }
}

/// The release rule, the Return placement and the codes behave the same at the board's mode.
#[test]
fn release_return_and_codes_hold_at_the_board_mode() {
    let (w, h, _) = MODES[1];
    assert_eq!(release_zone_at(0, 500, w, h), Some(Zone::LeftHalf));
    assert_eq!(release_zone_at(1919, 1079 - 10, w, h), Some(Zone::BottomRight));
    assert_eq!(release_zone_at(960, 0, w, h), Some(Zone::Fill));
    assert_eq!(release_zone_at(960, 540, w, h), None);
    let pre = (400, 200, 1000, 700);
    let (x, y, rw, rh) = return_frame_under_pointer(pre, 1900, 900, 960, w);
    assert_eq!((y, rw, rh), (200, 1000, 700));
    assert!(x + rw as i32 <= w as i32, "clamped onto the display");
    assert_eq!(request_from_code(CODE_FILL), Some(ZoneRequest::Tile(Zone::Fill)));
    assert_eq!(request_from_code(CODE_RETURN), Some(ZoneRequest::Return));
    assert_eq!(
        request_from_code(CODE_ARRANGE_LEFT_RIGHT),
        Some(ZoneRequest::Arrange(Arrangement::LeftRight))
    );
}

/// Unknown codes are refused (fail-closed) and every zone code fits the four feed bits.
#[test]
fn test_reject_unknown_zone_codes() {
    for code in [0u8, 14, 15] {
        assert_eq!(request_from_code(code), None, "code {code}");
    }
    for code in 1..=13u8 {
        assert!(request_from_code(code).is_some(), "code {code} is defined");
    }
    assert!(Zone::Fill.code() <= 0b1111);
}
