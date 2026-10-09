// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: screencapd's decisions on the host (RFC-0095, TASK-0068): which rectangle a shoot
//! saves (a selection clipped to the screen, the screen, a window by id, the front-most window
//! for a shot), how a stem becomes a file name in the screenshot folder, and the pointer blend
//! the saved image gets — `test_reject_*` for every bound.
//! OWNERS: @ui @runtime

use nexus_wire::screencapd::{
    self as wire, Window, KIND_AREA, KIND_SCREEN, KIND_WINDOW, STATUS_MALFORMED,
};
use screencapd::plan::{
    clip, file_name, file_path, shoot_rect, shot_rect, Frozen, Pointer, Rect, ATTEMPTS_MAX,
    NAME_BUF, PATH_BUF,
};
use screencapd::pointer::blend_row;

fn frozen() -> Frozen {
    let mut f = Frozen { w: 1280, h: 800, ..Frozen::default() };
    f.windows[0] = Window { id: 11, x: -40, y: 36, w: 640, h: 480 };
    f.windows[1] = Window { id: 12, x: 700, y: 600, w: 800, h: 400 };
    f.window_count = 2;
    f
}

fn name(stem: &str, attempt: u32) -> Option<String> {
    let mut out = [0u8; NAME_BUF];
    file_name(stem, attempt, &mut out).map(|n| String::from_utf8(out[..n].to_vec()).unwrap())
}

#[test]
fn a_selection_is_clipped_to_the_screen() {
    let f = frozen();
    assert_eq!(
        shoot_rect(KIND_AREA, 10, 20, 300, 200, &f),
        Ok(Rect { x: 10, y: 20, w: 300, h: 200 })
    );
    assert_eq!(
        shoot_rect(KIND_AREA, 1200, 700, 300, 300, &f),
        Ok(Rect { x: 1200, y: 700, w: 80, h: 100 }),
        "a selection past the edge keeps its visible part"
    );
    assert_eq!(shoot_rect(KIND_SCREEN, 0, 0, 0, 0, &f), Ok(Rect { x: 0, y: 0, w: 1280, h: 800 }));
}

#[test]
fn a_window_is_its_on_screen_rectangle() {
    let f = frozen();
    assert_eq!(
        shoot_rect(KIND_WINDOW, 11, 0, 0, 0, &f),
        Ok(Rect { x: 0, y: 36, w: 600, h: 480 }),
        "the part left of the screen is not in the frame"
    );
    assert_eq!(
        shoot_rect(KIND_WINDOW, 12, 0, 0, 0, &f),
        Ok(Rect { x: 700, y: 600, w: 580, h: 200 })
    );
    assert_eq!(shot_rect(KIND_WINDOW, &f), Ok(Rect { x: 0, y: 36, w: 600, h: 480 }), "front-most");
    assert_eq!(shot_rect(KIND_SCREEN, &f), Ok(f.screen()));
}

#[test]
fn test_reject_empty_foreign_and_unknown_shoots() {
    let f = frozen();
    assert_eq!(shoot_rect(KIND_AREA, 10, 10, 0, 50, &f), Err(STATUS_MALFORMED), "empty");
    assert_eq!(shoot_rect(KIND_AREA, 1280, 0, 10, 10, &f), Err(STATUS_MALFORMED), "off screen");
    assert_eq!(shoot_rect(KIND_WINDOW, 99, 0, 0, 0, &f), Err(STATUS_MALFORMED), "no such window");
    assert_eq!(shoot_rect(9, 0, 0, 10, 10, &f), Err(STATUS_MALFORMED), "unknown kind");
    let none = Frozen { w: 1280, h: 800, ..Frozen::default() };
    assert_eq!(shot_rect(KIND_WINDOW, &none), Err(STATUS_MALFORMED), "no window on screen");
    assert_eq!(shot_rect(KIND_AREA, &none), Err(STATUS_MALFORMED), "a shot has no selection");
    let huge = clip(i64::from(u32::MAX), 0, i64::from(u32::MAX), 1, none.screen());
    assert_eq!(huge, None, "no wrap on huge coordinates");
}

#[test]
fn names_take_a_suffix_when_taken_and_live_in_the_folder() {
    assert_eq!(name("Bildschirmfoto", 1).as_deref(), Some("Bildschirmfoto.png"));
    assert_eq!(name("Bildschirmfoto", 2).as_deref(), Some("Bildschirmfoto (2).png"));
    assert_eq!(name("Bildschirmfoto", 42).as_deref(), Some("Bildschirmfoto (42).png"));
    let longest = "x".repeat(wire::STEM_MAX_BYTES);
    let n = name(&longest, ATTEMPTS_MAX).expect("the longest name fits");
    assert_eq!(n.len(), wire::NAME_MAX_BYTES);
    let mut path = [0u8; PATH_BUF];
    let p = file_path(&n, &mut path).expect("the longest path fits");
    assert!(std::str::from_utf8(&path[..p]).unwrap().starts_with("/Bilder/Screenshots/"));
}

#[test]
fn test_reject_stems_and_attempts_out_of_bounds() {
    for bad in ["", "../x", "a/b", ".hidden", "tab\tstem"] {
        assert_eq!(name(bad, 1), None, "{bad:?}");
    }
    assert_eq!(name("ok", 0), None);
    assert_eq!(name("ok", ATTEMPTS_MAX + 1), None);
}

/// The pointer is blended exactly where it was, with the compositor's premultiplied rule.
#[test]
fn the_pointer_lands_where_it_was_with_the_compositors_blend() {
    // A 2×2 sprite: opaque white top-left, half-transparent black (premultiplied 0) elsewhere.
    let mut sprite = [0u8; 16];
    sprite[0..4].copy_from_slice(&[255, 255, 255, 255]);
    for px in 1..4 {
        sprite[px * 4 + 3] = 128;
    }
    let pointer = Pointer { x: 101, y: 51, hot_x: 1, hot_y: 1, w: 2, h: 2 };
    // Display row 50 of a crop starting at column 98, 6 pixels of grey 200.
    let mut row = [200u8; 24];
    blend_row(&mut row, 50, 98, &pointer, &sprite);
    // The sprite's top-left lands on display (100, 50) = crop column 2.
    assert_eq!(&row[8..11], &[255, 255, 255], "opaque white");
    let half = (200 * (255 - 128) / 255) as u8;
    assert_eq!(&row[12..15], &[half, half, half], "half-transparent black over grey");
    assert_eq!(&row[0..8], &[200; 8], "untouched left of the sprite");
    let mut other = [200u8; 24];
    blend_row(&mut other, 49, 98, &pointer, &sprite);
    assert_eq!(other, [200u8; 24], "a row above the sprite stays as it was");
}

#[test]
fn test_reject_a_sprite_shorter_than_it_claims() {
    let pointer = Pointer { x: 0, y: 0, hot_x: 0, hot_y: 0, w: 4, h: 4 };
    let mut row = [7u8; 16];
    blend_row(&mut row, 0, 0, &pointer, &[255u8; 8]);
    assert_eq!(row, [7u8; 16], "nothing is read past the sprite");
}
